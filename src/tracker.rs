use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::config::StateRole;
use crate::mapping::{DagError, DagSource, RoleMapping, states_from_config};

/// A dispatch-eligible work item, normalized from the tracker's own document
/// shape into the fields the orchestrator needs. Dependencies are exposed for
/// later blocker gating; this iteration does not act on them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub body: String,
    pub state: String,
    pub parent: Option<String>,
    pub dependencies: Vec<DependencyRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyRef {
    pub target: String,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    BlockedBy,
    Blocks,
}

/// The context of a single document as fetched by `lazyspec show <id> --json`,
/// used to enrich a prompt with a candidate's immediate parent (ADR-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocView {
    pub id: String,
    pub doc_type: String,
    pub title: String,
    pub body: String,
    pub status: String,
}

/// The outcome of looking a claim's document up in lazyspec during reconcile.
///
/// `Absent` (lazyspec authoritatively reports no such document) and a read
/// failure (a `TrackerError` — CLI spawn/exec/parse fault) are deliberately
/// distinct: an absent doc means the work is gone and its claim can be dropped,
/// whereas a read failure tells us nothing about the work, so reconcile must
/// leave every claim untouched and retry next cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocLookup {
    Present(DocView),
    Absent,
}

/// The work-source seam. lazyspec is the only implementation today; the SPEC's
/// tracker-adapter shape is preserved so other trackers remain future adapters.
pub trait Tracker {
    fn fetch_dispatchable(&self) -> Result<Vec<Candidate>, TrackerError>;

    /// Fetch one document's context (`lazyspec show <id> --json`) — the seam the
    /// prompt assembler uses to pull an iteration's immediate parent.
    fn fetch_doc(&self, id: &str) -> Result<DocView, TrackerError>;

    /// Look a claim's document up for reconcile, distinguishing a doc that
    /// lazyspec reports as not-found (`DocLookup::Absent`) from a read failure
    /// (`Err`). The distinction is the whole point of the seam: reconcile drops
    /// a claim for an absent doc but stays conservative on a read failure.
    fn lookup_doc(&self, id: &str) -> Result<DocLookup, TrackerError>;

    /// Move a document to `target_state` through lazyspec's gated lifecycle
    /// (the daemon owns transitions, per ADR-003). Modelled as
    /// `lazyspec update <id> --status <target>`.
    fn advance(&self, id: &str, target_state: &str) -> Result<(), TrackerError>;
}

/// Runs one lazyspec subcommand and returns its stdout, or a typed failure.
/// Injecting this seam lets the tracker be exercised without a live CLI.
pub trait CommandRunner {
    fn run(&self, args: &[&str]) -> Result<Vec<u8>, CliFailure>;
}

#[derive(Debug)]
pub enum CliFailure {
    Spawn(io::Error),
    Exit { code: Option<i32>, stderr: String },
}

/// The production runner: shells out to `lazyspec` in a project directory.
pub struct LazyspecRunner {
    dir: PathBuf,
}

impl LazyspecRunner {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        LazyspecRunner { dir: dir.into() }
    }
}

impl CommandRunner for LazyspecRunner {
    fn run(&self, args: &[&str]) -> Result<Vec<u8>, CliFailure> {
        let output = Command::new("lazyspec")
            .args(args)
            .current_dir(&self.dir)
            .output()
            .map_err(CliFailure::Spawn)?;
        if !output.status.success() {
            return Err(CliFailure::Exit {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(output.stdout)
    }
}

/// Reads dispatch candidates from lazyspec: `status --json` for the listing,
/// then `show <id> --json` per eligible document for its body.
pub struct LazyspecTracker<R: CommandRunner> {
    runner: R,
    mapping: RoleMapping,
}

impl<R: CommandRunner> LazyspecTracker<R> {
    pub fn new(runner: R, mapping: RoleMapping) -> Self {
        LazyspecTracker { runner, mapping }
    }
}

impl<R: CommandRunner> Tracker for LazyspecTracker<R> {
    fn fetch_dispatchable(&self) -> Result<Vec<Candidate>, TrackerError> {
        let listing = self.runner.run(&["status", "--json"])?;
        let mut candidates = parse_and_filter(&listing, &self.mapping)?;
        for candidate in &mut candidates {
            let shown = self.runner.run(&["show", &candidate.id, "--json"])?;
            candidate.body = parse_body(&shown)?;
        }
        Ok(candidates)
    }

    fn fetch_doc(&self, id: &str) -> Result<DocView, TrackerError> {
        let shown = self.runner.run(&["show", id, "--json"])?;
        parse_doc_view(id, &shown)
    }

    fn lookup_doc(&self, id: &str) -> Result<DocLookup, TrackerError> {
        match self.runner.run(&["show", id, "--json"]) {
            Ok(shown) => Ok(DocLookup::Present(parse_doc_view(id, &shown)?)),
            // lazyspec's only signal for a missing document is a non-zero exit
            // whose stderr names it; anything else is a genuine read failure.
            Err(CliFailure::Exit { stderr, .. }) if is_not_found(&stderr) => Ok(DocLookup::Absent),
            Err(failure) => Err(failure.into()),
        }
    }

    fn advance(&self, id: &str, target_state: &str) -> Result<(), TrackerError> {
        self.runner.run(&["update", id, "--status", target_state])?;
        Ok(())
    }
}

/// One lazyspec seam for the daemon: the tracker also answers lifecycle-DAG
/// queries via `config show --json`, so config validation and candidate fetch
/// share a single CLI-invocation path.
impl<R: CommandRunner> DagSource for LazyspecTracker<R> {
    fn states_for_type(&self, doc_type: &str) -> Result<Vec<String>, DagError> {
        let stdout = self.runner.run(&["config", "show", "--json"])?;
        states_from_config(&stdout, doc_type)
    }
}

fn parse_and_filter(json: &[u8], mapping: &RoleMapping) -> Result<Vec<Candidate>, TrackerError> {
    let listing: StatusListing = serde_json::from_slice(json).map_err(TrackerError::Parse)?;
    Ok(listing
        .documents
        .into_iter()
        .filter(|doc| mapping.classify(&doc.doc_type, &doc.status) == Some(StateRole::Dispatch))
        .map(normalize)
        .collect())
}

fn normalize(doc: RawDoc) -> Candidate {
    let identifier = identifier_from_path(&doc.path, &doc.id);
    let mut parent = None;
    let mut dependencies = Vec::new();
    for rel in doc.related {
        match rel.kind.as_str() {
            "implements" => parent = Some(rel.target),
            "blocked-by" => dependencies.push(DependencyRef {
                target: rel.target,
                kind: DependencyKind::BlockedBy,
            }),
            "blocks" => dependencies.push(DependencyRef {
                target: rel.target,
                kind: DependencyKind::Blocks,
            }),
            _ => {}
        }
    }
    Candidate {
        id: doc.id,
        identifier,
        title: doc.title,
        body: String::new(),
        state: doc.status,
        parent,
        dependencies,
    }
}

/// The human ref is the filename slug with its `<id>-` prefix removed, matching
/// how lazyspec names document files.
fn identifier_from_path(path: &str, id: &str) -> String {
    match Path::new(path).file_stem().and_then(|s| s.to_str()) {
        Some(stem) => stem
            .strip_prefix(id)
            .and_then(|rest| rest.strip_prefix('-'))
            .unwrap_or(stem)
            .to_string(),
        None => id.to_string(),
    }
}

fn parse_body(json: &[u8]) -> Result<String, TrackerError> {
    let shown: ShownDoc = serde_json::from_slice(json).map_err(TrackerError::Parse)?;
    Ok(shown.body)
}

fn parse_doc_view(id: &str, json: &[u8]) -> Result<DocView, TrackerError> {
    let shown: ShownDoc = serde_json::from_slice(json).map_err(TrackerError::Parse)?;
    Ok(DocView {
        id: id.to_string(),
        doc_type: shown.doc_type,
        title: shown.title,
        body: shown.body,
        status: shown.status,
    })
}

fn is_not_found(stderr: &str) -> bool {
    stderr.contains("not found")
}

#[derive(Deserialize)]
struct StatusListing {
    documents: Vec<RawDoc>,
}

#[derive(Deserialize)]
struct RawDoc {
    id: String,
    path: String,
    title: String,
    status: String,
    #[serde(rename = "type")]
    doc_type: String,
    #[serde(default)]
    related: Vec<RawRelation>,
}

#[derive(Deserialize)]
struct RawRelation {
    target: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct ShownDoc {
    #[serde(default)]
    title: String,
    #[serde(rename = "type", default)]
    doc_type: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    status: String,
}

#[derive(Debug)]
pub enum TrackerError {
    Spawn(io::Error),
    Command { code: Option<i32>, stderr: String },
    Parse(serde_json::Error),
}

impl From<CliFailure> for TrackerError {
    fn from(failure: CliFailure) -> Self {
        match failure {
            CliFailure::Spawn(e) => TrackerError::Spawn(e),
            CliFailure::Exit { code, stderr } => TrackerError::Command { code, stderr },
        }
    }
}

impl From<CliFailure> for DagError {
    fn from(failure: CliFailure) -> Self {
        match failure {
            CliFailure::Spawn(e) => DagError::Spawn(e),
            CliFailure::Exit { code, stderr } => DagError::Command { code, stderr },
        }
    }
}

impl fmt::Display for TrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrackerError::Spawn(e) => write!(f, "cannot run lazyspec: {e}"),
            TrackerError::Command { code, stderr } => match code {
                Some(c) => write!(f, "lazyspec exited {c}: {stderr}"),
                None => write!(f, "lazyspec terminated: {stderr}"),
            },
            TrackerError::Parse(e) => write!(f, "cannot parse lazyspec JSON: {e}"),
        }
    }
}

impl std::error::Error for TrackerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TrackerError::Spawn(e) => Some(e),
            TrackerError::Parse(e) => Some(e),
            TrackerError::Command { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeCli {
        status: Result<String, CliFailure>,
        bodies: HashMap<String, String>,
    }

    impl FakeCli {
        fn ok(status: &str) -> Self {
            FakeCli {
                status: Ok(status.to_string()),
                bodies: HashMap::new(),
            }
        }

        fn with_body(mut self, id: &str, body_json: &str) -> Self {
            self.bodies.insert(id.to_string(), body_json.to_string());
            self
        }
    }

    impl CommandRunner for FakeCli {
        fn run(&self, args: &[&str]) -> Result<Vec<u8>, CliFailure> {
            match args {
                ["status", "--json"] => match &self.status {
                    Ok(json) => Ok(json.clone().into_bytes()),
                    Err(CliFailure::Exit { code, stderr }) => Err(CliFailure::Exit {
                        code: *code,
                        stderr: stderr.clone(),
                    }),
                    Err(CliFailure::Spawn(_)) => Err(CliFailure::Exit {
                        code: None,
                        stderr: "spawn".to_string(),
                    }),
                },
                ["show", id, "--json"] => Ok(self
                    .bodies
                    .get(*id)
                    .cloned()
                    .unwrap_or_else(|| r#"{"body":""}"#.to_string())
                    .into_bytes()),
                other => panic!("unexpected args: {other:?}"),
            }
        }
    }

    const MIXED: &str = r#"{
      "documents": [
        {"id":"ITERATION-006","path":"docs/iterations/ITERATION-006-fetch-dispatchable.md",
         "title":"Fetch dispatchable","status":"accepted","type":"iteration","related":[]},
        {"id":"ITERATION-005","path":"docs/iterations/ITERATION-005-mapping.md",
         "title":"Mapping","status":"in-progress","type":"iteration","related":[]},
        {"id":"ITERATION-004","path":"docs/iterations/ITERATION-004-config.md",
         "title":"Config","status":"complete","type":"iteration","related":[]},
        {"id":"ITERATION-003","path":"docs/iterations/ITERATION-003-draft.md",
         "title":"Draft","status":"draft","type":"iteration","related":[]},
        {"id":"STORY-022","path":"docs/stories/STORY-022-surface.md",
         "title":"Surface","status":"accepted","type":"story","related":[]}
      ]
    }"#;

    #[test]
    fn only_dispatch_eligible_documents_are_returned() {
        let candidates =
            parse_and_filter(MIXED.as_bytes(), &RoleMapping::adr003_default()).unwrap();

        let ids: Vec<_> = candidates.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["ITERATION-006"]);
    }

    #[test]
    fn candidate_fields_are_normalized_from_the_listing_and_body() {
        let status = r#"{
          "documents": [
            {"id":"ITERATION-006",
             "path":"docs/iterations/ITERATION-006-fetch-dispatchable-iterations.md",
             "title":"Fetch dispatchable iterations","status":"accepted","type":"iteration",
             "related":[
               {"target":"STORY-022","type":"implements"},
               {"target":"ITERATION-005","type":"blocked-by"},
               {"target":"STORY-030","type":"blocks"},
               {"target":"ADR-003","type":"related-to"}
             ]}
          ]
        }"#;
        let tracker = LazyspecTracker::new(
            FakeCli::ok(status).with_body("ITERATION-006", r#"{"body":"Objective: fetch."}"#),
            RoleMapping::adr003_default(),
        );

        let candidates = tracker.fetch_dispatchable().unwrap();
        assert_eq!(candidates.len(), 1);
        let c = &candidates[0];
        assert_eq!(c.id, "ITERATION-006");
        assert_eq!(c.identifier, "fetch-dispatchable-iterations");
        assert_eq!(c.title, "Fetch dispatchable iterations");
        assert_eq!(c.state, "accepted");
        assert_eq!(c.body, "Objective: fetch.");
        assert_eq!(c.parent.as_deref(), Some("STORY-022"));
        assert_eq!(
            c.dependencies,
            vec![
                DependencyRef {
                    target: "ITERATION-005".to_string(),
                    kind: DependencyKind::BlockedBy,
                },
                DependencyRef {
                    target: "STORY-030".to_string(),
                    kind: DependencyKind::Blocks,
                },
            ]
        );
    }

    #[test]
    fn fetch_doc_parses_type_title_and_body_for_a_parent() {
        let tracker = LazyspecTracker::new(
            FakeCli::ok(r#"{"documents":[]}"#).with_body(
                "STORY-028",
                r#"{"id":"STORY-028","type":"story","title":"Assemble prompt","body":"As an operator..."}"#,
            ),
            RoleMapping::adr003_default(),
        );

        let doc = tracker.fetch_doc("STORY-028").unwrap();

        assert_eq!(doc.id, "STORY-028");
        assert_eq!(doc.doc_type, "story");
        assert_eq!(doc.title, "Assemble prompt");
        assert_eq!(doc.body, "As an operator...");
    }

    /// A runner whose `show` call fails with a chosen exit, used to exercise the
    /// absent-vs-read-failure split in `lookup_doc`.
    struct FailingShow {
        failure: CliFailure,
    }

    impl CommandRunner for FailingShow {
        fn run(&self, _args: &[&str]) -> Result<Vec<u8>, CliFailure> {
            match &self.failure {
                CliFailure::Exit { code, stderr } => Err(CliFailure::Exit {
                    code: *code,
                    stderr: stderr.clone(),
                }),
                CliFailure::Spawn(_) => Err(CliFailure::Spawn(io::Error::other("spawn"))),
            }
        }
    }

    #[test]
    fn lookup_doc_returns_present_with_the_parsed_status() {
        let tracker = LazyspecTracker::new(
            FakeCli::ok(r#"{"documents":[]}"#).with_body(
                "ITER-014",
                r#"{"id":"ITER-014","type":"iteration","title":"T","body":"B","status":"in-progress"}"#,
            ),
            RoleMapping::adr003_default(),
        );

        let lookup = tracker.lookup_doc("ITER-014").unwrap();

        match lookup {
            DocLookup::Present(view) => assert_eq!(view.status, "in-progress"),
            DocLookup::Absent => panic!("expected Present"),
        }
    }

    #[test]
    fn lookup_doc_maps_a_not_found_exit_to_absent() {
        let tracker = LazyspecTracker::new(
            FailingShow {
                failure: CliFailure::Exit {
                    code: Some(1),
                    stderr: "Error: document not found: ITER-014".to_string(),
                },
            },
            RoleMapping::adr003_default(),
        );

        assert_eq!(tracker.lookup_doc("ITER-014").unwrap(), DocLookup::Absent);
    }

    #[test]
    fn lookup_doc_surfaces_other_failures_as_read_errors() {
        let tracker = LazyspecTracker::new(
            FailingShow {
                failure: CliFailure::Exit {
                    code: Some(2),
                    stderr: "Error: config parse failed".to_string(),
                },
            },
            RoleMapping::adr003_default(),
        );

        let err = tracker.lookup_doc("ITER-014").unwrap_err();
        assert!(matches!(err, TrackerError::Command { .. }), "{err}");
    }

    #[test]
    fn fetch_doc_surfaces_a_non_zero_exit_as_a_typed_error() {
        let tracker = LazyspecTracker::new(RecordingCli::new(true), RoleMapping::adr003_default());

        let err = tracker.fetch_doc("STORY-028").unwrap_err();

        assert!(matches!(err, TrackerError::Command { .. }), "{err}");
    }

    #[test]
    fn non_zero_exit_yields_a_typed_error() {
        let tracker = LazyspecTracker::new(
            FakeCli {
                status: Err(CliFailure::Exit {
                    code: Some(1),
                    stderr: "boom".to_string(),
                }),
                bodies: HashMap::new(),
            },
            RoleMapping::adr003_default(),
        );

        let err = tracker.fetch_dispatchable().unwrap_err();
        assert!(
            matches!(err, TrackerError::Command { code: Some(1), .. }),
            "{err}"
        );
    }

    #[test]
    fn malformed_json_yields_a_typed_error() {
        let err = parse_and_filter(b"not json at all", &RoleMapping::adr003_default()).unwrap_err();
        assert!(matches!(err, TrackerError::Parse(_)), "{err}");
    }

    struct RecordingCli {
        calls: std::cell::RefCell<Vec<String>>,
        fail: bool,
    }

    impl RecordingCli {
        fn new(fail: bool) -> Self {
            RecordingCli {
                calls: std::cell::RefCell::new(Vec::new()),
                fail,
            }
        }
    }

    impl CommandRunner for RecordingCli {
        fn run(&self, args: &[&str]) -> Result<Vec<u8>, CliFailure> {
            self.calls.borrow_mut().push(args.join(" "));
            if self.fail {
                Err(CliFailure::Exit {
                    code: Some(1),
                    stderr: "gate rejected".to_string(),
                })
            } else {
                Ok(Vec::new())
            }
        }
    }

    #[test]
    fn advance_issues_update_with_the_target_status() {
        let tracker = LazyspecTracker::new(RecordingCli::new(false), RoleMapping::adr003_default());

        tracker.advance("ITERATION-008", "in-progress").unwrap();

        assert_eq!(
            tracker.runner.calls.borrow().as_slice(),
            ["update ITERATION-008 --status in-progress".to_string()]
        );
    }

    #[test]
    fn advance_failure_is_a_typed_command_error() {
        let tracker = LazyspecTracker::new(RecordingCli::new(true), RoleMapping::adr003_default());

        let err = tracker.advance("ITERATION-008", "in-progress").unwrap_err();

        assert!(
            matches!(err, TrackerError::Command { code: Some(1), .. }),
            "{err}"
        );
    }

    #[test]
    fn real_lazyspec_fetch_returns_well_formed_candidates_when_available() {
        let tracker = LazyspecTracker::new(LazyspecRunner::new("."), RoleMapping::adr003_default());
        // Skip when lazyspec is unavailable or the repo has no eligible work.
        if let Ok(candidates) = tracker.fetch_dispatchable() {
            for c in &candidates {
                assert!(!c.id.is_empty(), "{c:?}");
                assert!(!c.identifier.is_empty(), "{c:?}");
                assert_eq!(c.state, "accepted", "{c:?}");
            }
        }
    }
}
