use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::adapter::{AgentAdapter, TurnOutcome};
use crate::agent::AgentEvent;
use crate::config::Config;
use crate::dispatch::claim_and_activate;
use crate::prompt::assemble_prompt;
use crate::resolve::{OutcomeBranch, ResolvedTransition, resolve_outcome};
use crate::store::{ClaimRecord, Store, StoreError};
use crate::tracker::{Candidate, Tracker, TrackerError};
use crate::workspace::{Worktree, prepare_worktree};

/// How long a claim's lease is held before it is considered orphaned and
/// reclaimable by `reconcile`. A single tick finishes well inside this window.
pub const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(3600);

const FIRST_ATTEMPT: u32 = 1;

/// The result of one orchestrator tick over the dispatch stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickReport {
    /// Nothing was dispatch-eligible.
    Idle,
    /// A candidate was claimed and carried through to a terminal transition.
    Dispatched(Box<RunRecord>),
    /// A candidate was fetched but not run — the claim was lost to another
    /// holder, or the advance-to-active was gated out (its claim was released).
    Skipped { id: String, reason: String },
    /// The tick could not fetch candidates.
    Error(String),
}

/// The full trace of one dispatched item: claimed -> running -> terminal, plus
/// the events observed, the runtime, and the resolved lazyspec transition. This
/// is what `status`/`log` render (STORY-060 AC2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub holder: String,
    pub claimed_at_ms: u64,
    pub worktree: Option<PathBuf>,
    pub branch: Option<String>,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub events: Vec<AgentEvent>,
    pub outcome: TurnOutcome,
    pub transition: Option<ResolvedTransition>,
    pub transition_error: Option<String>,
    pub claim_released: bool,
}

impl RunRecord {
    pub fn runtime_ms(&self) -> u64 {
        self.ended_at_ms.saturating_sub(self.started_at_ms)
    }

    /// The trace as human lines: claimed, running, each agent event, the terminal
    /// transition, and the run totals.
    pub fn log_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!(
            "{} claimed by {} at {}ms",
            self.id, self.holder, self.claimed_at_ms
        ));
        match (&self.worktree, &self.branch) {
            (Some(path), Some(branch)) => lines.push(format!(
                "{} running in {} on {}",
                self.id,
                path.display(),
                branch
            )),
            _ => lines.push(format!("{} running (no worktree prepared)", self.id)),
        }
        for event in &self.events {
            lines.push(format!("  {}", event_label(event)));
        }
        match &self.transition {
            Some(t) => lines.push(format!(
                "{} terminal: {} -> {} (runtime {}ms)",
                self.id,
                branch_label(t.branch),
                t.target_state,
                self.runtime_ms()
            )),
            None => lines.push(format!(
                "{} terminal: transition failed: {} (runtime {}ms)",
                self.id,
                self.transition_error.as_deref().unwrap_or("unknown"),
                self.runtime_ms()
            )),
        }
        lines.push(format!(
            "totals: runtime={}ms events={} tokens=unknown",
            self.runtime_ms(),
            self.events.len()
        ));
        lines
    }

    /// The single-line state label for `status`: the terminal outcome branch.
    pub fn state_label(&self) -> String {
        match &self.transition {
            Some(t) => format!("terminal:{}", branch_label(t.branch)),
            None => "terminal:error".to_string(),
        }
    }

    pub fn transition_target(&self) -> String {
        self.transition
            .as_ref()
            .map(|t| t.target_state.clone())
            .unwrap_or_else(|| "-".to_string())
    }
}

fn branch_label(branch: OutcomeBranch) -> &'static str {
    match branch {
        OutcomeBranch::Success => "success",
        OutcomeBranch::Failure => "failure",
    }
}

fn event_label(event: &AgentEvent) -> String {
    match event {
        AgentEvent::SessionStarted { session_id } => format!("session-started {session_id}"),
        AgentEvent::Notification { text, .. } => format!("notification {text}"),
        AgentEvent::OtherMessage { text, .. } => format!("message {text}"),
        AgentEvent::TurnCompleted { .. } => "turn-completed".to_string(),
        AgentEvent::TurnFailed { reason, .. } => format!("turn-failed {reason}"),
        AgentEvent::TurnCancelled { .. } => "turn-cancelled".to_string(),
        AgentEvent::Malformed { reason, .. } => format!("malformed {reason}"),
    }
}

/// Run one orchestrator tick end-to-end (STORY-060): fetch a dispatch-eligible
/// candidate, durably claim it and advance it to the active state, prepare its
/// isolated worktree, assemble the prompt, run the agent turn in that worktree,
/// then resolve the outcome into the mapped lazyspec transition.
///
/// On a non-clean turn the failure transition is applied, the store claim is
/// released, and the worktree is left on disk for inspection (AC3). On a clean
/// turn the claim is retained (the item is terminal and never re-offered by the
/// role filter) so `status`/`log` can attest that it was written.
///
/// Every seam is injected: the tracker, store, adapter, template source, and
/// clock, so the whole composition is exercised without a live daemon.
#[allow(clippy::too_many_arguments)]
pub async fn run_tick<T, A>(
    tracker: &T,
    store: &Store,
    adapter: &A,
    config: &Config,
    template_src: &str,
    repo: &Path,
    now: u64,
    holder: &str,
) -> TickReport
where
    T: Tracker,
    A: AgentAdapter,
{
    let candidates = match tracker.fetch_dispatchable() {
        Ok(candidates) => candidates,
        Err(e) => return TickReport::Error(format!("cannot fetch dispatchable candidates: {e}")),
    };
    let Some(candidate) = candidates.into_iter().next() else {
        return TickReport::Idle;
    };

    let claim = match claim_and_activate(
        tracker,
        store,
        &candidate,
        holder,
        now,
        DEFAULT_LEASE_TTL,
        &config.transitions.claim,
        || {},
    ) {
        Ok(claim) => claim,
        Err(e) => {
            return TickReport::Skipped {
                id: candidate.id,
                reason: e.to_string(),
            };
        }
    };

    let root = repo.join(&config.workspace.root);
    let started_at_ms = now_ms();

    let worktree = match prepare_worktree(repo, &root, &candidate.id, None) {
        Ok(worktree) => worktree,
        Err(e) => {
            return fail_before_agent(
                tracker,
                store,
                config,
                &candidate,
                &claim,
                now,
                started_at_ms,
                None,
                format!("cannot prepare worktree: {e}"),
            );
        }
    };

    let prompt = match assemble_prompt(template_src, &candidate, FIRST_ATTEMPT, tracker) {
        Ok(prompt) => prompt,
        Err(e) => {
            return fail_before_agent(
                tracker,
                store,
                config,
                &candidate,
                &claim,
                now,
                started_at_ms,
                Some(worktree),
                format!("cannot assemble prompt: {e}"),
            );
        }
    };

    let session = adapter.start_session(worktree.clone());
    let report = adapter.run_turn(&session, &prompt).await;
    adapter.stop(session).await;
    let ended_at_ms = now_ms();

    let (transition, transition_error) =
        match resolve_outcome(tracker, &candidate.id, &report.outcome, &config.transitions) {
            Ok(transition) => (Some(transition), None),
            Err(e) => (None, Some(e.to_string())),
        };

    let clean = matches!(report.outcome, TurnOutcome::Completed);
    let claim_released = if clean {
        false
    } else {
        store.release(&candidate.id).is_ok()
    };

    TickReport::Dispatched(Box::new(RunRecord {
        id: candidate.id,
        identifier: candidate.identifier,
        title: candidate.title,
        holder: claim.holder,
        claimed_at_ms: now,
        worktree: Some(worktree.path),
        branch: Some(worktree.branch),
        started_at_ms,
        ended_at_ms,
        events: report.events,
        outcome: report.outcome,
        transition,
        transition_error,
        claim_released,
    }))
}

/// A post-claim fault before the agent ran (worktree or prompt failure): apply
/// the failure transition, release the claim, and preserve any worktree.
#[allow(clippy::too_many_arguments)]
fn fail_before_agent<T: Tracker>(
    tracker: &T,
    store: &Store,
    config: &Config,
    candidate: &Candidate,
    claim: &ClaimRecord,
    claimed_at: u64,
    started_at: u64,
    worktree: Option<Worktree>,
    reason: String,
) -> TickReport {
    let outcome = TurnOutcome::Failed { reason };
    let (transition, transition_error) =
        match resolve_outcome(tracker, &candidate.id, &outcome, &config.transitions) {
            Ok(transition) => (Some(transition), None),
            Err(e) => (None, Some(e.to_string())),
        };
    let claim_released = store.release(&candidate.id).is_ok();
    let (worktree_path, branch) = match worktree {
        Some(w) => (Some(w.path), Some(w.branch)),
        None => (None, None),
    };

    TickReport::Dispatched(Box::new(RunRecord {
        id: candidate.id.clone(),
        identifier: candidate.identifier.clone(),
        title: candidate.title.clone(),
        holder: claim.holder.clone(),
        claimed_at_ms: claimed_at,
        worktree: worktree_path,
        branch,
        started_at_ms: started_at,
        ended_at_ms: now_ms(),
        events: Vec::new(),
        outcome,
        transition,
        transition_error,
        claim_released,
    }))
}

/// What a restart's reconcile resolved (STORY-060 AC4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Expired orphan claims that were released.
    pub released: Vec<String>,
    /// Still-live leases left in place.
    pub retained: Vec<String>,
    /// Released ids the tracker would still offer as fresh — the double-dispatch
    /// risk. Must be empty: an item mid-run sits in an active (non-dispatch)
    /// state, so the role filter never re-offers it.
    pub re_offered: Vec<String>,
}

#[derive(Debug)]
pub enum ReconcileError {
    Store(StoreError),
    Tracker(TrackerError),
}

impl std::fmt::Display for ReconcileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReconcileError::Store(e) => write!(f, "reconcile store error: {e:?}"),
            ReconcileError::Tracker(e) => write!(f, "reconcile tracker error: {e}"),
        }
    }
}

impl std::error::Error for ReconcileError {}

/// Reconcile the durable store on daemon start, before any tick (STORY-060 AC4).
///
/// Every claim persisted across a restart was made by a now-dead daemon, so any
/// whose lease has expired is an orphan and is released. The tracker's current
/// dispatch set is consulted to attest the invariant: a released id is never
/// simultaneously dispatch-eligible, so a restart cannot double-dispatch an
/// in-flight item.
pub fn reconcile<T: Tracker>(
    store: &Store,
    tracker: &T,
    now: u64,
) -> Result<ReconcileReport, ReconcileError> {
    let dispatchable: BTreeSet<String> = tracker
        .fetch_dispatchable()
        .map_err(ReconcileError::Tracker)?
        .into_iter()
        .map(|c| c.id)
        .collect();

    let claims = store.claims().map_err(ReconcileError::Store)?;
    let mut released = Vec::new();
    let mut retained = Vec::new();
    let mut re_offered = Vec::new();

    for (id, record) in claims {
        if record.due_at <= now {
            store.release(&id).map_err(ReconcileError::Store)?;
            if dispatchable.contains(&id) {
                re_offered.push(id.clone());
            }
            released.push(id);
        } else {
            retained.push(id);
        }
    }

    Ok(ReconcileReport {
        released,
        retained,
        re_offered,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    use tempfile::TempDir;

    use crate::adapter::TurnReport;
    use crate::config::load_str;
    use crate::tracker::DocView;

    const HOLDER: &str = "agentd-1";
    const NOW: u64 = 1_000_000;

    /// A tracker fake: offers a fixed candidate listing, answers `fetch_doc` with
    /// a canned parent, records every advance, and can flip a candidate to an
    /// active state once claimed (so a re-fetch drops it, as lazyspec would).
    struct FakeTracker {
        candidate: Mutex<Option<Candidate>>,
        parent: DocView,
        advances: Arc<Mutex<Vec<(String, String)>>>,
        drop_after_claim: bool,
    }

    impl FakeTracker {
        fn new(candidate: Candidate, parent: DocView) -> Self {
            FakeTracker {
                candidate: Mutex::new(Some(candidate)),
                parent,
                advances: Arc::new(Mutex::new(Vec::new())),
                drop_after_claim: false,
            }
        }

        fn advances(&self) -> Arc<Mutex<Vec<(String, String)>>> {
            self.advances.clone()
        }
    }

    impl Tracker for FakeTracker {
        fn fetch_dispatchable(&self) -> Result<Vec<Candidate>, TrackerError> {
            Ok(self.candidate.lock().unwrap().clone().into_iter().collect())
        }

        fn fetch_doc(&self, _id: &str) -> Result<DocView, TrackerError> {
            Ok(self.parent.clone())
        }

        fn advance(&self, id: &str, target: &str) -> Result<(), TrackerError> {
            self.advances
                .lock()
                .unwrap()
                .push((id.to_string(), target.to_string()));
            if self.drop_after_claim {
                *self.candidate.lock().unwrap() = None;
            }
            Ok(())
        }
    }

    /// An adapter fake: returns a chosen outcome + events and records the cwd and
    /// prompt it was handed, proving the turn ran against the worktree.
    struct FakeAdapter {
        report: TurnReport,
        ran: Arc<Mutex<Option<(PathBuf, String)>>>,
    }

    impl FakeAdapter {
        fn new(report: TurnReport) -> Self {
            FakeAdapter {
                report,
                ran: Arc::new(Mutex::new(None)),
            }
        }

        fn ran(&self) -> Arc<Mutex<Option<(PathBuf, String)>>> {
            self.ran.clone()
        }
    }

    impl AgentAdapter for FakeAdapter {
        type Session = Worktree;

        fn start_session(&self, worktree: Worktree) -> Worktree {
            worktree
        }

        async fn run_turn(&self, session: &Worktree, prompt: &str) -> TurnReport {
            *self.ran.lock().unwrap() = Some((session.path.clone(), prompt.to_string()));
            self.report.clone()
        }

        async fn stop(&self, _session: Worktree) {}
    }

    fn git_ok(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
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

    fn store(dir: &Path) -> Store {
        Store::open(&dir.join("store.redb")).unwrap()
    }

    fn candidate() -> Candidate {
        Candidate {
            id: "ITER-014".to_string(),
            identifier: "execute-one-iteration".to_string(),
            title: "Execute one iteration end-to-end".to_string(),
            body: "Objective: prove the full path.".to_string(),
            state: "accepted".to_string(),
            parent: Some("STORY-060".to_string()),
            dependencies: Vec::new(),
        }
    }

    fn parent() -> DocView {
        DocView {
            id: "STORY-060".to_string(),
            doc_type: "story".to_string(),
            title: "Execute one iteration end-to-end".to_string(),
            body: "As an operator, I want one eligible iteration to flow through.".to_string(),
        }
    }

    fn config() -> Config {
        load_str("").unwrap()
    }

    fn completed_report() -> TurnReport {
        TurnReport {
            outcome: TurnOutcome::Completed,
            events: vec![
                AgentEvent::SessionStarted {
                    session_id: "sess-1".to_string(),
                },
                AgentEvent::Notification {
                    pid: 1,
                    at_ms: NOW,
                    text: "working".to_string(),
                },
                AgentEvent::TurnCompleted { pid: 1, at_ms: NOW },
            ],
        }
    }

    // AC1: clean run composes fetch -> claim -> worktree -> run -> advance-terminal.
    #[tokio::test]
    async fn clean_run_flows_to_terminal_and_records_the_full_trace() {
        let repo = init_repo();
        let store = store(repo.path());
        let tracker = FakeTracker::new(candidate(), parent());
        let advances = tracker.advances();
        let adapter = FakeAdapter::new(completed_report());
        let ran = adapter.ran();

        let report = run_tick(
            &tracker,
            &store,
            &adapter,
            &config(),
            crate::prompt::DEFAULT_TEMPLATE,
            repo.path(),
            NOW,
            HOLDER,
        )
        .await;

        let record = match report {
            TickReport::Dispatched(record) => record,
            other => panic!("expected Dispatched, got {other:?}"),
        };

        // Claim written and retained on a clean run.
        assert_eq!(store.get("ITER-014").unwrap().unwrap().holder, HOLDER);
        assert!(!record.claim_released);

        // Worktree created on disk and used as the turn's cwd.
        let worktree = record.worktree.clone().unwrap();
        assert!(worktree.is_dir());
        let (ran_cwd, ran_prompt) = ran.lock().unwrap().clone().unwrap();
        assert_eq!(ran_cwd, worktree);
        assert!(ran_prompt.contains("Execute one iteration end-to-end"));
        assert!(ran_prompt.contains("(STORY-060)"), "{ran_prompt}");

        // Claim-to-active then terminal transition, in order.
        assert_eq!(
            advances.lock().unwrap().as_slice(),
            [
                ("ITER-014".to_string(), "in-progress".to_string()),
                ("ITER-014".to_string(), "complete".to_string()),
            ]
        );

        let transition = record.transition.clone().unwrap();
        assert_eq!(transition.branch, OutcomeBranch::Success);
        assert_eq!(transition.target_state, "complete");
        assert_eq!(record.outcome, TurnOutcome::Completed);
        assert_eq!(record.claimed_at_ms, NOW);
        assert!(record.ended_at_ms >= record.started_at_ms);
        assert!(
            record
                .events
                .iter()
                .any(|e| matches!(e, AgentEvent::TurnCompleted { .. }))
        );
    }

    // AC2: the run record renders the full trace (claimed, running, terminal,
    // runtime total, transition) without a socket.
    #[test]
    fn record_renders_the_full_trace() {
        let record = RunRecord {
            id: "ITER-014".to_string(),
            identifier: "execute-one-iteration".to_string(),
            title: "Execute one iteration end-to-end".to_string(),
            holder: HOLDER.to_string(),
            claimed_at_ms: 1000,
            worktree: Some(PathBuf::from("/w/ITER-014")),
            branch: Some("agentd/ITER-014".to_string()),
            started_at_ms: 2000,
            ended_at_ms: 5000,
            events: vec![
                AgentEvent::SessionStarted {
                    session_id: "sess-1".to_string(),
                },
                AgentEvent::TurnCompleted {
                    pid: 7,
                    at_ms: 4000,
                },
            ],
            outcome: TurnOutcome::Completed,
            transition: Some(ResolvedTransition {
                branch: OutcomeBranch::Success,
                target_state: "complete".to_string(),
            }),
            transition_error: None,
            claim_released: false,
        };

        assert_eq!(record.runtime_ms(), 3000);
        assert_eq!(record.state_label(), "terminal:success");
        assert_eq!(record.transition_target(), "complete");

        let rendered = record.log_lines().join("\n");
        assert!(
            rendered.contains("ITER-014 claimed by agentd-1 at 1000ms"),
            "{rendered}"
        );
        assert!(
            rendered.contains("running in /w/ITER-014 on agentd/ITER-014"),
            "{rendered}"
        );
        assert!(rendered.contains("session-started sess-1"), "{rendered}");
        assert!(rendered.contains("turn-completed"), "{rendered}");
        assert!(
            rendered.contains("terminal: success -> complete (runtime 3000ms)"),
            "{rendered}"
        );
        assert!(rendered.contains("runtime=3000ms events=2"), "{rendered}");
    }

    // AC3: a non-clean exit takes the failure transition, releases the claim, and
    // preserves the worktree for inspection.
    #[tokio::test]
    async fn failed_run_transitions_failure_releases_claim_and_preserves_worktree() {
        let repo = init_repo();
        let store = store(repo.path());
        let tracker = FakeTracker::new(candidate(), parent());
        let advances = tracker.advances();
        let adapter = FakeAdapter::new(TurnReport {
            outcome: TurnOutcome::Failed {
                reason: "agent exited with status 3".to_string(),
            },
            events: vec![AgentEvent::TurnFailed {
                pid: 1,
                at_ms: NOW,
                reason: "agent exited with status 3".to_string(),
            }],
        });

        let report = run_tick(
            &tracker,
            &store,
            &adapter,
            &config(),
            crate::prompt::DEFAULT_TEMPLATE,
            repo.path(),
            NOW,
            HOLDER,
        )
        .await;

        let record = match report {
            TickReport::Dispatched(record) => record,
            other => panic!("expected Dispatched, got {other:?}"),
        };

        let transition = record.transition.clone().unwrap();
        assert_eq!(transition.branch, OutcomeBranch::Failure);
        assert_eq!(transition.target_state, "rejected");
        assert_eq!(
            advances.lock().unwrap().last().unwrap(),
            &("ITER-014".to_string(), "rejected".to_string())
        );

        assert!(record.claim_released);
        assert_eq!(
            store.get("ITER-014").unwrap(),
            None,
            "claim must be released"
        );

        let worktree = record.worktree.clone().unwrap();
        assert!(
            worktree.is_dir(),
            "worktree must be preserved for inspection"
        );
    }

    #[tokio::test]
    async fn nothing_eligible_is_idle() {
        let repo = init_repo();
        let store = store(repo.path());
        let tracker = FakeTracker {
            candidate: Mutex::new(None),
            parent: parent(),
            advances: Arc::new(Mutex::new(Vec::new())),
            drop_after_claim: false,
        };
        let adapter = FakeAdapter::new(completed_report());

        let report = run_tick(
            &tracker,
            &store,
            &adapter,
            &config(),
            crate::prompt::DEFAULT_TEMPLATE,
            repo.path(),
            NOW,
            HOLDER,
        )
        .await;

        assert_eq!(report, TickReport::Idle);
    }

    // AC4: a claim persisted across a restart (store reopen) is reconciled without
    // double-dispatching the item.
    #[test]
    fn reconcile_releases_an_orphaned_claim_without_double_dispatch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("store.redb");

        // A prior daemon claimed the item, advanced it to in-progress, then died.
        {
            let store = Store::open(&path).unwrap();
            store
                .claim("ITER-014", HOLDER, NOW, DEFAULT_LEASE_TTL)
                .unwrap();
        }

        // Restart: reopen the store; the item now sits in-progress in lazyspec.
        let store = Store::open(&path).unwrap();
        assert!(
            store.get("ITER-014").unwrap().is_some(),
            "claim must survive the restart"
        );

        let tracker = FakeTracker {
            // in-progress is an active state -> not dispatch-eligible.
            candidate: Mutex::new(None),
            parent: parent(),
            advances: Arc::new(Mutex::new(Vec::new())),
            drop_after_claim: false,
        };

        let after_expiry = NOW + DEFAULT_LEASE_TTL.as_millis() as u64 + 1;
        let report = reconcile(&store, &tracker, after_expiry).unwrap();

        assert_eq!(report.released, vec!["ITER-014".to_string()]);
        assert!(
            report.re_offered.is_empty(),
            "must not be re-offered: {report:?}"
        );
        assert_eq!(
            store.get("ITER-014").unwrap(),
            None,
            "the orphan claim must be resolved"
        );
        assert!(
            tracker.fetch_dispatchable().unwrap().is_empty(),
            "an in-progress item must not be dispatched again"
        );
    }

    #[test]
    fn reconcile_leaves_a_live_lease_in_place() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("store.redb")).unwrap();
        store
            .claim("ITER-014", HOLDER, NOW, DEFAULT_LEASE_TTL)
            .unwrap();
        let tracker = FakeTracker::new(candidate(), parent());

        let report = reconcile(&store, &tracker, NOW + 1).unwrap();

        assert_eq!(report.retained, vec!["ITER-014".to_string()]);
        assert!(report.released.is_empty());
        assert!(store.get("ITER-014").unwrap().is_some());
    }

    // A lost claim (already held) is reported as skipped, not run.
    #[tokio::test]
    async fn a_lost_claim_is_skipped() {
        let repo = init_repo();
        let store = store(repo.path());
        store
            .claim("ITER-014", "other-agent", NOW, DEFAULT_LEASE_TTL)
            .unwrap();
        let tracker = FakeTracker::new(candidate(), parent());
        let adapter = FakeAdapter::new(completed_report());

        let report = run_tick(
            &tracker,
            &store,
            &adapter,
            &config(),
            crate::prompt::DEFAULT_TEMPLATE,
            repo.path(),
            NOW,
            HOLDER,
        )
        .await;

        assert!(matches!(report, TickReport::Skipped { .. }), "{report:?}");
        assert_eq!(
            store.get("ITER-014").unwrap().unwrap().holder,
            "other-agent"
        );
    }
}
