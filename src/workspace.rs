use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// An isolated working tree prepared for one dispatched iteration (ADR-005):
/// a git worktree on its own branch, sharing the base repo's object store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

/// How an iteration's worktree came to be ready for the run about to use it
/// (BUG-002). A re-dispatch is not always a fresh start: the stall path kills the
/// worker and keeps the tree (`RunningAction::TerminateAndRetry`), so the retry
/// that follows finds one already there. Keeping the two apart makes "a tree is
/// already here" a state the caller handles rather than a `worktree add` failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedWorktree {
    /// A new tree on a new branch, cut from the base repo's HEAD.
    Created(Worktree),
    /// The tree a previous run left behind, re-entered on the branch its work is
    /// on. The work in it is why the stall path kept it, so the retry resumes
    /// there rather than discarding it.
    Adopted(Worktree),
}

impl PreparedWorktree {
    pub fn into_worktree(self) -> Worktree {
        match self {
            PreparedWorktree::Created(worktree) | PreparedWorktree::Adopted(worktree) => worktree,
        }
    }
}

#[derive(Debug)]
pub enum WorkspaceError {
    PathEscapesRoot { iter_id: String, root: PathBuf },
    Spawn(io::Error),
    Root(io::Error),
    Git { code: Option<i32>, stderr: String },
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceError::PathEscapesRoot { iter_id, root } => write!(
                f,
                "worktree path for `{iter_id}` would escape the workspace root {}",
                root.display()
            ),
            WorkspaceError::Spawn(e) => write!(f, "cannot run git: {e}"),
            WorkspaceError::Root(e) => write!(f, "cannot prepare workspace root: {e}"),
            WorkspaceError::Git { code, stderr } => match code {
                Some(c) => write!(f, "git worktree add failed ({c}): {stderr}"),
                None => write!(f, "git worktree add terminated: {stderr}"),
            },
        }
    }
}

impl std::error::Error for WorkspaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WorkspaceError::Spawn(e) | WorkspaceError::Root(e) => Some(e),
            WorkspaceError::PathEscapesRoot { .. } | WorkspaceError::Git { .. } => None,
        }
    }
}

#[derive(Debug)]
pub enum WorktreeListError {
    Read(io::Error),
}

impl fmt::Display for WorktreeListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorktreeListError::Read(e) => write!(f, "cannot list worktrees on disk: {e}"),
        }
    }
}

impl std::error::Error for WorktreeListError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WorktreeListError::Read(e) => Some(e),
        }
    }
}

#[derive(Debug)]
pub enum WorktreeRemoveError {
    Spawn(io::Error),
    Git { code: Option<i32>, stderr: String },
}

impl fmt::Display for WorktreeRemoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorktreeRemoveError::Spawn(e) => write!(f, "cannot run git worktree remove: {e}"),
            WorktreeRemoveError::Git { code, stderr } => match code {
                Some(c) => write!(f, "git worktree remove failed ({c}): {stderr}"),
                None => write!(f, "git worktree remove terminated: {stderr}"),
            },
        }
    }
}

impl std::error::Error for WorktreeRemoveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WorktreeRemoveError::Spawn(e) => Some(e),
            WorktreeRemoveError::Git { .. } => None,
        }
    }
}

/// The worktrees reconcile finds on disk, so it can cross-check the store against
/// the trees that actually exist (ADR-002). Injected like the tracker so reconcile
/// tests can supply a present/absent/failing listing without touching git.
pub trait WorktreeLister {
    fn list_present(&self) -> Result<BTreeSet<String>, WorktreeListError>;
}

/// The live lister: a directory scan of the workspace root.
pub struct DiskWorktrees {
    root: PathBuf,
}

impl DiskWorktrees {
    pub fn new(root: PathBuf) -> Self {
        DiskWorktrees { root }
    }
}

impl WorktreeLister for DiskWorktrees {
    fn list_present(&self) -> Result<BTreeSet<String>, WorktreeListError> {
        list_worktrees(&self.root)
    }
}

/// Enumerate the worktrees present under `root`, keyed by the iteration id each
/// was created for — `prepare_worktree` lays them out at `<root>/<iter-id>`, so a
/// directory name is exactly the claim id it belongs to.
///
/// A missing root is an empty listing, not a failure: the workspace simply holds
/// no trees yet. Only a genuine I/O fault errors, keeping a transient read failure
/// distinguishable from "nothing is there" so reconcile can retry rather than
/// mistake an unreadable directory for an emptied one.
pub fn list_worktrees(root: &Path) -> Result<BTreeSet<String>, WorktreeListError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(WorktreeListError::Read(e)),
    };

    let mut ids = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(WorktreeListError::Read)?;
        if entry.file_type().map_err(WorktreeListError::Read)?.is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            ids.insert(name.to_string());
        }
    }
    Ok(ids)
}

/// Prepare an isolated worktree for `iter_id` under `root`, branching off the
/// current HEAD of `repo` (`git worktree add <root>/<iter-id> -b <branch>`).
///
/// A tree a previous run left at that path is adopted rather than collided with
/// (BUG-002): the stall path kills a hung worker and keeps its tree, so the retry
/// it schedules arrives here to find one already present, and the work in it is
/// what the retry resumes. Only a path git does not recognise as a worktree — a
/// stray directory in the way — still fails.
///
/// The branch defaults to `agentd/<iter-id>`; a lazyspec `branch_name` override
/// is honoured when supplied. The target path is checked to stay under `root`
/// before git is touched, and a failed attempt cleans up any artifact it created
/// so a retry sees no half-built, claimable tree.
pub fn prepare_worktree(
    repo: &Path,
    root: &Path,
    iter_id: &str,
    branch_name: Option<&str>,
) -> Result<PreparedWorktree, WorkspaceError> {
    let path =
        resolve_under_root(root, iter_id).ok_or_else(|| WorkspaceError::PathEscapesRoot {
            iter_id: iter_id.to_string(),
            root: root.to_path_buf(),
        })?;
    if let Some(existing) = worktree_at(&path) {
        return Ok(PreparedWorktree::Adopted(existing));
    }

    let branch = branch_name
        .map(str::to_string)
        .unwrap_or_else(|| format!("agentd/{iter_id}"));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(WorkspaceError::Root)?;
    }

    let target_existed = path.exists();
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "add"])
        .arg(&path)
        .arg("-b")
        .arg(&branch)
        .output()
        .map_err(WorkspaceError::Spawn)?;

    if !output.status.success() {
        clean_up_partial(repo, &path, target_existed);
        return Err(WorkspaceError::Git {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(PreparedWorktree::Created(Worktree { path, branch }))
}

/// The worktree already at `path`, if there is one. Git reports the top level of
/// the working tree containing `path`, so a stray directory inside the repo names
/// the repo's own root and is correctly not adoptable — only a path that is its
/// own top level is a worktree. The branch is read back rather than recomputed:
/// the branch a previous run's work sits on is the one to resume, whatever the
/// caller would have named a fresh one.
fn worktree_at(path: &Path) -> Option<Worktree> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let toplevel = PathBuf::from(lines.next()?);
    let branch = lines.next()?.to_string();
    same_file(&toplevel, path).then(|| Worktree {
        path: path.to_path_buf(),
        branch,
    })
}

/// Whether two paths name the same existing directory. Git reports a fully
/// resolved path, so a lexical comparison would miss a symlinked workspace root.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// On failure, drop only what this attempt created: remove the target directory
/// iff we brought it into existence (never a pre-existing sibling worktree), and
/// prune any orphaned worktree admin entry git may have left registered.
fn clean_up_partial(repo: &Path, path: &Path, target_existed: bool) {
    if !target_existed && path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "prune"])
        .output();
}

/// Remove the worktree at `path` from `repo` (`git worktree remove --force`),
/// the terminal-item cleanup of ADR-005 (SPEC §8.6). `--force` because an
/// interrupted run's tree may hold uncommitted or untracked changes git would
/// otherwise refuse to discard. Returns a distinct error the caller can log; a
/// failure here is non-fatal to the caller — the claim is released regardless.
pub fn remove_worktree(repo: &Path, path: &Path) -> Result<(), WorktreeRemoveError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "remove", "--force"])
        .arg(path)
        .output()
        .map_err(WorktreeRemoveError::Spawn)?;

    if !output.status.success() {
        return Err(WorktreeRemoveError::Git {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

/// Lexically resolve `<root>/<iter_id>` and confirm it stays strictly under
/// `root`; returns `None` for any id that would escape (e.g. `../evil`, an
/// absolute path, or one that normalizes back to the root itself).
fn resolve_under_root(root: &Path, iter_id: &str) -> Option<PathBuf> {
    if iter_id.is_empty() {
        return None;
    }
    let target = normalize(&root.join(iter_id));
    let root = normalize(root);
    (target != root && target.starts_with(&root)).then_some(target)
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap()
    }

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        let out = git(dir, args);
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn init_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        git_ok(dir.path(), &["init", "-q"]);
        git_ok(dir.path(), &["config", "user.email", "test@example.com"]);
        git_ok(dir.path(), &["config", "user.name", "agentd test"]);
        std::fs::write(dir.path().join("README.md"), "base").unwrap();
        git_ok(dir.path(), &["add", "."]);
        git_ok(dir.path(), &["commit", "-q", "-m", "base"]);
        dir
    }

    fn worktree_count(repo: &Path) -> usize {
        git_ok(repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count()
    }

    #[test]
    fn creates_worktree_on_its_branch_sharing_the_object_store() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");
        let base_head = git_ok(repo.path(), &["rev-parse", "HEAD"]);

        let prepared = prepare_worktree(repo.path(), &root, "ITER-009", None).unwrap();

        let PreparedWorktree::Created(wt) = prepared else {
            panic!("nothing was there to adopt: {prepared:?}");
        };
        assert!(wt.path.is_dir());
        assert!(wt.path.starts_with(normalize(&root)));
        assert_eq!(wt.branch, "agentd/ITER-009");
        assert_eq!(
            git_ok(&wt.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "agentd/ITER-009"
        );
        assert_eq!(git_ok(&wt.path, &["rev-parse", "HEAD"]), base_head);
    }

    #[test]
    fn honours_a_branch_name_override() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");

        let wt = prepare_worktree(repo.path(), &root, "ITER-009", Some("feature/custom"))
            .unwrap()
            .into_worktree();

        assert_eq!(wt.branch, "feature/custom");
        assert_eq!(
            git_ok(&wt.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "feature/custom"
        );
    }

    // BUG-002: a stall kill keeps the tree deliberately, so the retry that follows
    // prepares an id whose tree is already there. That collision must resolve to the
    // work the killed run left, not to a `worktree add` failure.
    #[test]
    fn a_repeated_attempt_adopts_the_tree_the_first_left_behind() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");
        let first = prepare_worktree(repo.path(), &root, "ITER-009", None)
            .unwrap()
            .into_worktree();
        std::fs::write(first.path.join("half-done.txt"), "what the killed run left").unwrap();

        let again = prepare_worktree(repo.path(), &root, "ITER-009", None).unwrap();

        let PreparedWorktree::Adopted(adopted) = again else {
            panic!("a tree already on disk must be adopted, not created: {again:?}");
        };
        assert_eq!(adopted, first);
        assert_eq!(
            std::fs::read_to_string(adopted.path.join("half-done.txt")).unwrap(),
            "what the killed run left",
            "the point of keeping the tree is the work in it"
        );
        assert_eq!(
            worktree_count(repo.path()),
            2,
            "adoption registers no second worktree"
        );
        assert_eq!(
            git_ok(&adopted.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "agentd/ITER-009",
            "the adopted tree stays on the branch its work is on"
        );
    }

    // A directory that is not a worktree of this repo is not adoptable: it stays a
    // reported failure rather than being mistaken for a previous run's tree.
    #[test]
    fn a_plain_directory_in_the_way_is_a_reported_failure_not_an_adoption() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");
        std::fs::create_dir_all(root.join("ITER-010")).unwrap();
        std::fs::write(root.join("ITER-010/stray.txt"), "not a worktree").unwrap();

        let err = prepare_worktree(repo.path(), &root, "ITER-010", None).unwrap_err();

        assert!(matches!(err, WorkspaceError::Git { .. }), "{err}");
        assert_eq!(
            worktree_count(repo.path()),
            1,
            "only the main worktree may remain registered"
        );
    }

    #[test]
    fn a_path_escaping_iter_id_is_rejected_before_touching_git() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");

        let err = prepare_worktree(repo.path(), &root, "../evil", None).unwrap_err();

        assert!(
            matches!(err, WorkspaceError::PathEscapesRoot { .. }),
            "{err}"
        );
        assert!(!repo.path().join(".agentd/evil").exists());
        assert_eq!(
            worktree_count(repo.path()),
            1,
            "no worktree may be registered when the id is rejected"
        );
    }

    #[test]
    fn lists_worktrees_present_under_the_root_by_iter_id() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");
        prepare_worktree(repo.path(), &root, "ITER-100", None).unwrap();
        prepare_worktree(repo.path(), &root, "ITER-200", None).unwrap();

        let present = list_worktrees(&root).unwrap();

        assert!(present.contains("ITER-100"));
        assert!(present.contains("ITER-200"));
        assert_eq!(present.len(), 2);
    }

    #[test]
    fn a_missing_root_lists_no_worktrees_without_erroring() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");

        assert!(list_worktrees(&root).unwrap().is_empty());
    }

    // STORY-010 AC1: the terminal-item cleanup removes the worktree directory and
    // unregisters it from git so a later prepare of the same id starts clean.
    #[test]
    fn remove_worktree_deletes_the_tree_and_unregisters_it() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");
        let wt = prepare_worktree(repo.path(), &root, "ITER-300", None)
            .unwrap()
            .into_worktree();
        assert!(wt.path.is_dir());
        assert_eq!(worktree_count(repo.path()), 2);

        remove_worktree(repo.path(), &wt.path).unwrap();

        assert!(!wt.path.exists(), "the worktree dir must be gone");
        assert_eq!(
            worktree_count(repo.path()),
            1,
            "only the main worktree may remain registered"
        );
    }

    // An interrupted run's tree can hold uncommitted work; `--force` removes it
    // anyway so terminal cleanup is never blocked by a dirty tree.
    #[test]
    fn remove_worktree_forces_removal_of_a_dirty_tree() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");
        let wt = prepare_worktree(repo.path(), &root, "ITER-301", None)
            .unwrap()
            .into_worktree();
        std::fs::write(wt.path.join("dirty.txt"), "uncommitted").unwrap();

        remove_worktree(repo.path(), &wt.path).unwrap();

        assert!(
            !wt.path.exists(),
            "a dirty tree must still be removed under --force"
        );
    }

    // A removal that git rejects surfaces a distinct, loggable error rather than
    // a silent success, so the caller can record why cleanup failed.
    #[test]
    fn remove_worktree_on_a_missing_tree_is_a_distinct_error() {
        let repo = init_repo();
        let missing = repo.path().join(".agentd/workspaces/ITER-404");

        let err = remove_worktree(repo.path(), &missing).unwrap_err();

        assert!(matches!(err, WorktreeRemoveError::Git { .. }), "{err}");
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn an_absolute_iter_id_is_rejected() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");

        let err = prepare_worktree(repo.path(), &root, "/etc/agentd", None).unwrap_err();

        assert!(
            matches!(err, WorkspaceError::PathEscapesRoot { .. }),
            "{err}"
        );
    }
}
