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

/// Prepare an isolated worktree for `iter_id` under `root`, branching off the
/// current HEAD of `repo` (`git worktree add <root>/<iter-id> -b <branch>`).
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
) -> Result<Worktree, WorkspaceError> {
    let path =
        resolve_under_root(root, iter_id).ok_or_else(|| WorkspaceError::PathEscapesRoot {
            iter_id: iter_id.to_string(),
            root: root.to_path_buf(),
        })?;
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

    Ok(Worktree { path, branch })
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

        let wt = prepare_worktree(repo.path(), &root, "ITER-009", None).unwrap();

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

        let wt = prepare_worktree(repo.path(), &root, "ITER-009", Some("feature/custom")).unwrap();

        assert_eq!(wt.branch, "feature/custom");
        assert_eq!(
            git_ok(&wt.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "feature/custom"
        );
    }

    #[test]
    fn a_repeated_attempt_fails_cleanly_leaving_no_partial_tree() {
        let repo = init_repo();
        let root = repo.path().join(".agentd/workspaces");

        let first = prepare_worktree(repo.path(), &root, "ITER-009", None).unwrap();
        assert!(first.path.is_dir());

        let err = prepare_worktree(repo.path(), &root, "ITER-009", None).unwrap_err();

        assert!(matches!(err, WorkspaceError::Git { .. }), "{err}");
        assert!(!err.to_string().is_empty());
        assert_eq!(
            worktree_count(repo.path()),
            2,
            "only the main worktree and the first attempt must remain registered"
        );
        assert_eq!(
            git_ok(&first.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "agentd/ITER-009",
            "the first worktree must stay usable"
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
