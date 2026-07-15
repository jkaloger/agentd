use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::adapter::{AgentAdapter, TurnOutcome};
use crate::agent::AgentEvent;
use crate::config::{Config, StateRole};
use crate::dispatch::{DispatchError, claim_and_activate};
use crate::mapping::RoleMapping;
use crate::prompt::assemble_prompt;
use crate::resolve::{OutcomeBranch, ResolvedTransition, resolve_outcome};
use crate::store::{ClaimRecord, Store, StoreError};
use crate::tracker::{Candidate, DependencyKind, DocLookup, Tracker, TrackerError};
use crate::workspace::{Worktree, WorktreeListError, WorktreeLister, prepare_worktree};

/// How long a claim's lease is held before it is considered orphaned and
/// reclaimable by `reconcile`. A single tick finishes well inside this window.
pub const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(3600);

const FIRST_ATTEMPT: u32 = 1;

/// The result of one orchestrator tick over the dispatch stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickReport {
    /// Nothing was dispatch-eligible.
    Idle,
    /// A candidate was claimed and carried through to a terminal transition. Only
    /// the inline `run_tick` composition (tests) produces this; the daemon splits
    /// dispatch from completion (ADR-008) and mirrors a `RunRecord` directly.
    #[allow(dead_code)]
    Dispatched(Box<RunRecord>),
    /// A candidate was fetched but not run — the claim was lost to another
    /// holder, or the advance-to-active was gated out (its claim was released).
    Skipped {
        id: String,
        reason: String,
        claim: SkipClaim,
    },
    /// Candidates were dispatch-eligible by role but every one was held back by
    /// the blocker gate — a `blocked-by` dependency or the parent work item is
    /// not yet terminal-complete, or an unresolvable ref (STORY-061). Nothing was
    /// claimed; the reasons are surfaced so the projection can record them.
    Blocked(Vec<BlockedCandidate>),
    /// The tick could not fetch candidates.
    Error(String),
}

/// A candidate the blocker gate held back, with the reason naming the specific
/// unfinished (or unresolvable) prerequisite (STORY-061 AC1/AC2/AC4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedCandidate {
    pub id: String,
    pub reason: String,
}

/// Whether a candidate has cleared the blocker gate (STORY-061).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateStatus {
    /// Every prerequisite is terminal-complete; the candidate may be dispatched.
    Eligible,
    /// A prerequisite is unfinished or unresolvable; the candidate is held.
    Blocked { reason: String },
}

/// What durably happened to the store claim behind a `Skipped` tick (STORY-019
/// AC1). The projection log records every durable state change, so the post-commit
/// seam needs to know whether `claim_and_activate` actually committed anything —
/// a lost claim never touches the store, but a gated-out advance commits a claim
/// and then releases it, both of which must reach the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipClaim {
    /// The store claim was never committed — another holder already owned it.
    NotClaimed,
    /// The store claim committed, then was durably released after the advance
    /// it gated on was rejected.
    ClaimedThenReleased,
    /// The store claim committed, but the subsequent release attempt itself
    /// failed; the store still authoritatively holds the claim.
    ClaimedReleaseFailed,
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

/// The synchronous half of a tick (ADR-008): either a claimed, activated
/// candidate ready to hand to a spawned worker, or the tick's terminal report
/// when nothing was dispatched.
pub enum DispatchOutcome {
    /// A candidate was durably claimed and advanced to the active state; spawn
    /// `run_worker` with this to run its turn.
    Dispatched(Dispatch),
    /// Nothing was dispatched this call — the tick's report (`Idle`/`Blocked`/
    /// `Skipped`/`Error`). There is no worker to spawn.
    NoDispatch(TickReport),
}

/// A claimed candidate handed from the dispatch step to a spawned worker: the
/// candidate, its committed claim, and when it was claimed.
pub struct Dispatch {
    pub candidate: Candidate,
    pub claim: ClaimRecord,
    pub claimed_at_ms: u64,
}

/// A finished worker turn resolved into a `RunRecord`, with the claim not yet
/// released: the store is single-owner in the orchestrator loop (ADR-008), so
/// `finalize` performs the release there. `release_claim` carries the inline
/// path's release/retain decision — a clean turn retains, a failure or a
/// pre-agent fault releases.
pub struct WorkerCompletion {
    pub record: RunRecord,
    pub release_claim: bool,
}

/// The dispatch half of a tick (ADR-008, STORY-069): fetch the dispatch-eligible
/// candidates, order them (STORY-004), and select the first that is free of a
/// live store claim and past the blocker gate (STORY-061), then durably claim it
/// and advance it to the active state.
///
/// On success the claimed candidate is returned for a worker to run; otherwise
/// the tick's terminal report is returned. The turn no longer runs here — that is
/// `run_worker`, spawned by the caller without awaiting it.
///
/// A live claim means the item is already running or claimed, so dispatching it
/// again would double-run it (STORY-003 AC3); the store is the truth for claims
/// (ADR-002). The gate holds an item until its `blocked-by` dependencies and
/// parent are terminal-complete (STORY-061); a lookup failure is conservative —
/// surface it, never dispatch.
///
/// `on_activated` fires exactly once, after the claim and advance-to-active both
/// succeed and before any turn runs — the seam the daemon uses to publish the
/// item as Running.
///
/// `at_status_cap` is the per-status concurrency gate (STORY-006, ADR-007): it is
/// asked whether the active state a claimed worker would occupy
/// (`config.transitions.claim`, lowercased) is already at its configured cap. A
/// capped candidate is passed over exactly like a live-claimed one — not claimed,
/// left for a future tick — so an uncapped status is never starved by a capped one.
/// The daemon tallies its live-worker registry to answer; the inline path passes a
/// predicate that never caps.
pub fn dispatch_one<T, R, S>(
    tracker: &T,
    store: &Store,
    config: &Config,
    now: u64,
    holder: &str,
    on_activated: R,
    at_status_cap: S,
) -> DispatchOutcome
where
    T: Tracker,
    R: FnOnce(&Candidate),
    S: Fn(&str) -> bool,
{
    let mut candidates = match tracker.fetch_dispatchable() {
        Ok(candidates) => candidates,
        Err(e) => {
            return DispatchOutcome::NoDispatch(TickReport::Error(format!(
                "cannot fetch dispatchable candidates: {e}"
            )));
        }
    };
    candidates.sort_by(dispatch_order);
    let mapping = RoleMapping::from_config(config);
    // The active state every claimed worker occupies (ADR-008): one target today
    // (`config.transitions.claim`), so every candidate's prospective active state
    // is the same. Lowercased to match the normalized per-status cap keys (AC3).
    let active_state = config.transitions.claim.to_lowercase();
    let mut blocked = Vec::new();
    let mut selected = None;
    for candidate in candidates {
        if has_live_claim(store, &candidate.id, now) {
            continue;
        }
        // Per-status cap (STORY-006): pass over — but do not claim — a candidate
        // whose active state is already at its cap, so the next candidate is still
        // considered. In-flight runs are never affected (ADR-007).
        if at_status_cap(&active_state) {
            continue;
        }
        match dispatch_gate(tracker, &mapping, &candidate) {
            Ok(GateStatus::Eligible) => {
                selected = Some(candidate);
                break;
            }
            Ok(GateStatus::Blocked { reason }) => blocked.push(BlockedCandidate {
                id: candidate.id,
                reason,
            }),
            Err(e) => {
                return DispatchOutcome::NoDispatch(TickReport::Error(format!(
                    "cannot evaluate blocker gate for {}: {e}",
                    candidate.id
                )));
            }
        }
    }
    let Some(candidate) = selected else {
        return DispatchOutcome::NoDispatch(if blocked.is_empty() {
            TickReport::Idle
        } else {
            TickReport::Blocked(blocked)
        });
    };

    let claim = match claim_and_activate(
        tracker,
        store,
        &candidate,
        holder,
        now,
        DEFAULT_LEASE_TTL,
        &config.transitions.claim,
        || on_activated(&candidate),
    ) {
        Ok(claim) => claim,
        Err(e) => {
            let claim = match &e {
                DispatchError::Claim(_) => SkipClaim::NotClaimed,
                DispatchError::Advance(_) => SkipClaim::ClaimedThenReleased,
                DispatchError::Release(_) => SkipClaim::ClaimedReleaseFailed,
            };
            return DispatchOutcome::NoDispatch(TickReport::Skipped {
                id: candidate.id,
                reason: e.to_string(),
                claim,
            });
        }
    };

    DispatchOutcome::Dispatched(Dispatch {
        candidate,
        claim,
        claimed_at_ms: now,
    })
}

/// The worker half of a tick (ADR-008, STORY-069): prepare the claimed
/// candidate's isolated worktree, assemble its prompt, run the agent turn in that
/// worktree, and resolve the outcome into a `RunRecord`.
///
/// The claim is released by the orchestrator loop, not here (the store is
/// single-owner there); this returns the release/retain decision alongside the
/// record via `WorkerCompletion`. Every inline branch is preserved exactly: a
/// clean turn retains the claim, a failed turn takes the failure transition and
/// releases, and a fault before the agent runs takes `fail_before_agent` (failure
/// transition, release, any worktree left on disk).
///
/// Every seam is injected: the tracker, adapter, template source, and clock, so
/// the whole composition is exercised without a live daemon.
pub async fn run_worker<T, A>(
    tracker: &T,
    adapter: &A,
    config: &Config,
    template_src: &str,
    repo: &Path,
    dispatch: Dispatch,
) -> WorkerCompletion
where
    T: Tracker,
    A: AgentAdapter,
{
    let Dispatch {
        candidate,
        claim,
        claimed_at_ms,
    } = dispatch;

    let root = repo.join(&config.workspace.root);
    let started_at_ms = now_ms();

    let worktree = match prepare_worktree(repo, &root, &candidate.id, None) {
        Ok(worktree) => worktree,
        Err(e) => {
            return fail_before_agent(
                tracker,
                config,
                &candidate,
                &claim,
                claimed_at_ms,
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
                config,
                &candidate,
                &claim,
                claimed_at_ms,
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

    WorkerCompletion {
        record: RunRecord {
            id: candidate.id,
            identifier: candidate.identifier,
            title: candidate.title,
            holder: claim.holder,
            claimed_at_ms,
            worktree: Some(worktree.path),
            branch: Some(worktree.branch),
            started_at_ms,
            ended_at_ms,
            events: report.events,
            outcome: report.outcome,
            transition,
            transition_error,
            claim_released: false,
        },
        // A clean turn retains its claim (the item is terminal and never
        // re-offered); every other branch releases it.
        release_claim: !clean,
    }
}

/// Apply a finished worker's release/retain decision against the store and return
/// the completed `RunRecord` (ADR-008). The store is single-owner in the
/// orchestrator loop, so the release happens here rather than in the spawned
/// worker; a clean turn retains its claim, every other branch releases.
pub fn finalize(store: &Store, completion: WorkerCompletion) -> RunRecord {
    let WorkerCompletion {
        mut record,
        release_claim,
    } = completion;
    if release_claim {
        record.claim_released = store.release(&record.id).is_ok();
    }
    record
}

/// Run one orchestrator tick end-to-end, inline (STORY-060): dispatch a candidate
/// and, if one was claimed, run its worker turn to completion and resolve the
/// outcome. This is the inline composition of `dispatch_one` + `run_worker` +
/// `finalize`; the daemon instead spawns `run_worker` concurrently (ADR-008), but
/// the whole path stays exercisable without a live daemon here.
///
/// `on_activated` fires exactly once, after the claim and advance-to-active both
/// succeed and before the agent runs.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub async fn run_tick<T, A, R>(
    tracker: &T,
    store: &Store,
    adapter: &A,
    config: &Config,
    template_src: &str,
    repo: &Path,
    now: u64,
    holder: &str,
    on_activated: R,
) -> TickReport
where
    T: Tracker,
    A: AgentAdapter,
    R: FnOnce(&Candidate),
{
    let dispatch = match dispatch_one(tracker, store, config, now, holder, on_activated, |_| false)
    {
        DispatchOutcome::Dispatched(dispatch) => dispatch,
        DispatchOutcome::NoDispatch(report) => return report,
    };
    let completion = run_worker(tracker, adapter, config, template_src, repo, dispatch).await;
    TickReport::Dispatched(Box::new(finalize(store, completion)))
}

/// Decide whether `candidate` has cleared the blocker gate (STORY-061, the
/// ADR-003 / SPEC §8.2 rule). The prerequisites are the parent work item plus
/// every `blocked-by` dependency; the reverse `blocks` edge does not gate here.
///
/// For each prerequisite: a `DocLookup::Absent` means the ref cannot be resolved
/// in lazyspec, so the candidate is blocked and the ref named (AC4); a present
/// document whose `(type, status)` does not classify as `Terminal` is unfinished,
/// so the candidate is blocked (AC1/AC2). Only when every prerequisite is
/// terminal-complete is the candidate `Eligible` (AC3).
///
/// A `lookup_doc` failure propagates as `Err` so the caller reports it rather
/// than dispatching on incomplete knowledge (never silently dispatch).
fn dispatch_gate<T: Tracker>(
    tracker: &T,
    mapping: &RoleMapping,
    candidate: &Candidate,
) -> Result<GateStatus, TrackerError> {
    let prerequisites = candidate.parent.iter().cloned().chain(
        candidate
            .dependencies
            .iter()
            .filter(|dep| dep.kind == DependencyKind::BlockedBy)
            .map(|dep| dep.target.clone()),
    );
    for prereq in prerequisites {
        match tracker.lookup_doc(&prereq)? {
            DocLookup::Absent => {
                return Ok(GateStatus::Blocked {
                    reason: format!("prerequisite {prereq} could not be resolved in lazyspec"),
                });
            }
            DocLookup::Present(view) => {
                // Terminality is a per-state fact across types: an iteration's
                // parent is a story/bug, not a dispatchable type, so `classify`
                // (type-aware) would wrongly return None for it. `role_of_state`
                // reads the shared lifecycle role directly (STORY-061).
                if mapping.role_of_state(&view.status) != Some(StateRole::Terminal) {
                    return Ok(GateStatus::Blocked {
                        reason: format!(
                            "prerequisite {prereq} is not terminal-complete (status {})",
                            view.status
                        ),
                    });
                }
            }
        }
    }
    Ok(GateStatus::Eligible)
}

/// A post-claim fault before the agent ran (worktree or prompt failure): apply
/// the failure transition and preserve any worktree. The claim release is
/// deferred to `finalize` in the orchestrator loop (store single-owner, ADR-008),
/// signalled by `release_claim: true`.
#[allow(clippy::too_many_arguments)]
fn fail_before_agent<T: Tracker>(
    tracker: &T,
    config: &Config,
    candidate: &Candidate,
    claim: &ClaimRecord,
    claimed_at: u64,
    started_at: u64,
    worktree: Option<Worktree>,
    reason: String,
) -> WorkerCompletion {
    let outcome = TurnOutcome::Failed { reason };
    let (transition, transition_error) =
        match resolve_outcome(tracker, &candidate.id, &outcome, &config.transitions) {
            Ok(transition) => (Some(transition), None),
            Err(e) => (None, Some(e.to_string())),
        };
    let (worktree_path, branch) = match worktree {
        Some(w) => (Some(w.path), Some(w.branch)),
        None => (None, None),
    };

    WorkerCompletion {
        record: RunRecord {
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
            claim_released: false,
        },
        release_claim: true,
    }
}

/// What a restart's reconcile resolved (STORY-060 AC4, STORY-017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Expired orphan claims that were released (STORY-016).
    pub released: Vec<String>,
    /// Live-lease claims whose document lazyspec no longer has: dropped as work
    /// that has vanished (STORY-017 AC1).
    pub dropped: Vec<String>,
    /// Live-lease claims whose document lazyspec reports terminal-complete:
    /// finalized, released without re-dispatch (STORY-017 AC2).
    pub finalized: Vec<String>,
    /// Live leases whose document is still active, left in place (STORY-017 AC3,
    /// STORY-065 AC1) — the working tree is present, so the run can resume.
    pub retained: Vec<String>,
    /// Live-lease claims whose on-disk worktree has vanished: released, since the
    /// run cannot resume without its tree (STORY-065 AC2).
    pub abandoned: Vec<String>,
    /// Worktrees on disk with no matching claim, reported for cleanup — never
    /// deleted here (deletion is STORY-044) (STORY-065 AC3).
    pub orphaned_worktrees: Vec<String>,
    /// Released ids the tracker would still offer as fresh — the double-dispatch
    /// risk. Must be empty: an item mid-run sits in an active (non-dispatch)
    /// state, so the role filter never re-offers it.
    pub re_offered: Vec<String>,
}

#[derive(Debug)]
pub enum ReconcileError {
    Store(StoreError),
    Tracker(TrackerError),
    Worktree(WorktreeListError),
}

impl std::fmt::Display for ReconcileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReconcileError::Store(e) => write!(f, "reconcile store error: {e:?}"),
            ReconcileError::Tracker(e) => write!(f, "reconcile tracker error: {e}"),
            ReconcileError::Worktree(e) => write!(f, "reconcile worktree error: {e}"),
        }
    }
}

impl std::error::Error for ReconcileError {}

/// Reconcile the durable store on daemon start, before any tick (STORY-060 AC4,
/// STORY-017).
///
/// Every claim persisted across a restart was made by a now-dead daemon. A claim
/// whose lease has expired is an orphan and is released regardless (STORY-016).
/// A claim whose lease is still live is cross-checked against lazyspec, the truth
/// for what work exists (ADR-002), and against the worktrees on disk (STORY-065):
/// - the document is absent → drop the claim, its work has vanished (AC1);
/// - the document is terminal-complete → finalize, release without re-dispatch (AC2);
/// - the document is still active and its worktree is present → retain (AC3 / AC1);
/// - the document is still active but its worktree has vanished → release it as
///   abandoned, the run cannot resume without its tree (STORY-065 AC2).
///
/// A worktree on disk with no matching claim is reported as orphaned for cleanup,
/// never deleted here (STORY-065 AC3; deletion is STORY-044).
///
/// If lazyspec or the worktree listing cannot be read, reconcile mutates nothing
/// and returns an error so the next cycle retries (AC4): every read — dispatch
/// set, lazyspec, worktree scan — happens before any store write, so a read
/// failure leaves all claims — orphans included — untouched and reports no orphan.
///
/// The tracker's current dispatch set attests the invariant that a released id is
/// never simultaneously dispatch-eligible, so a restart cannot double-dispatch.
pub fn reconcile<T: Tracker, W: WorktreeLister>(
    store: &Store,
    tracker: &T,
    worktrees: &W,
    mapping: &RoleMapping,
    now: u64,
) -> Result<ReconcileReport, ReconcileError> {
    let dispatchable: BTreeSet<String> = tracker
        .fetch_dispatchable()
        .map_err(ReconcileError::Tracker)?
        .into_iter()
        .map(|c| c.id)
        .collect();

    let claims = store.claims().map_err(ReconcileError::Store)?;
    let claim_ids: BTreeSet<String> = claims.iter().map(|(id, _)| id.clone()).collect();

    // Read every live lease's lazyspec state and the on-disk worktrees before
    // touching the store: any read failure here must leave the whole store
    // untouched so the next cycle retries.
    let mut lookups: BTreeMap<String, DocLookup> = BTreeMap::new();
    for (id, record) in &claims {
        if record.due_at > now {
            let lookup = tracker.lookup_doc(id).map_err(ReconcileError::Tracker)?;
            lookups.insert(id.clone(), lookup);
        }
    }
    let present = worktrees.list_present().map_err(ReconcileError::Worktree)?;

    let mut released = Vec::new();
    let mut dropped = Vec::new();
    let mut finalized = Vec::new();
    let mut retained = Vec::new();
    let mut abandoned = Vec::new();
    let mut re_offered = Vec::new();

    for (id, record) in claims {
        if record.due_at <= now {
            store.release(&id).map_err(ReconcileError::Store)?;
            if dispatchable.contains(&id) {
                re_offered.push(id.clone());
            }
            released.push(id);
            continue;
        }
        match lookups.get(&id) {
            Some(DocLookup::Absent) => {
                store.release(&id).map_err(ReconcileError::Store)?;
                dropped.push(id);
            }
            Some(DocLookup::Present(view))
                if matches!(
                    mapping.classify(&view.doc_type, &view.status),
                    Some(StateRole::Terminal)
                ) =>
            {
                store.release(&id).map_err(ReconcileError::Store)?;
                finalized.push(id);
            }
            // Active, dispatch, unmapped, or an unlooked-up live lease: the work
            // is not done, so the claim stays put — but only if its worktree
            // survived; a vanished tree means the run cannot resume.
            _ => {
                if present.contains(&id) {
                    retained.push(id);
                } else {
                    store.release(&id).map_err(ReconcileError::Store)?;
                    abandoned.push(id);
                }
            }
        }
    }

    let orphaned_worktrees = present
        .into_iter()
        .filter(|id| !claim_ids.contains(id))
        .collect();

    Ok(ReconcileReport {
        released,
        dropped,
        finalized,
        retained,
        abandoned,
        orphaned_worktrees,
        re_offered,
    })
}

/// Dispatch ordering (STORY-004, ADR-007 / SPEC §8.2): a lower priority number
/// is more urgent and sorts first (AC1); ties fall to the oldest `created_at`,
/// then the `identifier` lexicographically for a deterministic total order (AC2);
/// a `None` priority sorts after every defined priority, however large (AC3).
fn dispatch_order(a: &Candidate, b: &Candidate) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let by_priority = match (a.priority, b.priority) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    by_priority
        .then_with(|| a.created_at.cmp(&b.created_at))
        .then_with(|| a.identifier.cmp(&b.identifier))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether the store holds a still-live claim for `id` at `now`. A live claim
/// means the item is already running or claimed and must not be dispatched
/// again; `claim_and_activate` remains the atomic guard against the race where
/// a claim appears after this check.
fn has_live_claim(store: &Store, id: &str, now: u64) -> bool {
    matches!(store.get(id), Ok(Some(record)) if record.due_at > now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    use tempfile::TempDir;

    use crate::adapter::TurnReport;
    use crate::config::load_str;
    use crate::tracker::{DependencyRef, DocView};
    use crate::workspace::DiskWorktrees;

    const HOLDER: &str = "agentd-1";
    const NOW: u64 = 1_000_000;

    /// A tracker fake: offers a fixed candidate listing, answers `fetch_doc` with
    /// a canned parent, records every advance, and can flip a candidate to an
    /// active state once claimed (so a re-fetch drops it, as lazyspec would). For
    /// reconcile, `lookups` supplies each claim's lazyspec state and `lookup_fails`
    /// simulates lazyspec being unreadable.
    struct FakeTracker {
        candidate: Mutex<Option<Candidate>>,
        parent: DocView,
        advances: Arc<Mutex<Vec<(String, String)>>>,
        drop_after_claim: bool,
        fail_advance: bool,
        lookups: HashMap<String, DocLookup>,
        lookup_fails: bool,
    }

    impl FakeTracker {
        fn new(candidate: Candidate, parent: DocView) -> Self {
            FakeTracker {
                candidate: Mutex::new(Some(candidate)),
                parent,
                advances: Arc::new(Mutex::new(Vec::new())),
                drop_after_claim: false,
                fail_advance: false,
                lookups: HashMap::new(),
                lookup_fails: false,
            }
        }

        fn advances(&self) -> Arc<Mutex<Vec<(String, String)>>> {
            self.advances.clone()
        }

        /// Seed the lazyspec state a prerequisite (`blocked-by` dep or parent)
        /// resolves to when the blocker gate looks it up.
        fn with_lookup(mut self, id: &str, lookup: DocLookup) -> Self {
            self.lookups.insert(id.to_string(), lookup);
            self
        }
    }

    impl Tracker for FakeTracker {
        fn fetch_dispatchable(&self) -> Result<Vec<Candidate>, TrackerError> {
            Ok(self.candidate.lock().unwrap().clone().into_iter().collect())
        }

        fn fetch_doc(&self, _id: &str) -> Result<DocView, TrackerError> {
            Ok(self.parent.clone())
        }

        fn lookup_doc(&self, id: &str) -> Result<DocLookup, TrackerError> {
            if self.lookup_fails {
                return Err(TrackerError::Command {
                    code: Some(1),
                    stderr: "lazyspec unreadable".to_string(),
                });
            }
            // Default an unlisted id to a live active iteration so live-lease
            // tests that don't care about lazyspec keep retaining.
            Ok(self.lookups.get(id).cloned().unwrap_or_else(|| {
                DocLookup::Present(DocView {
                    id: id.to_string(),
                    doc_type: "iteration".to_string(),
                    title: String::new(),
                    body: String::new(),
                    status: "in-progress".to_string(),
                })
            }))
        }

        fn advance(&self, id: &str, target: &str) -> Result<(), TrackerError> {
            self.advances
                .lock()
                .unwrap()
                .push((id.to_string(), target.to_string()));
            if self.fail_advance {
                return Err(TrackerError::Command {
                    code: Some(1),
                    stderr: "gate rejected".to_string(),
                });
            }
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

    /// A worktree lister fake: reports a fixed set of on-disk trees, or a scan
    /// failure, without touching git — the seam reconcile cross-checks claims
    /// against.
    struct FakeWorktrees {
        present: BTreeSet<String>,
        fails: bool,
    }

    impl WorktreeLister for FakeWorktrees {
        fn list_present(&self) -> Result<BTreeSet<String>, WorktreeListError> {
            if self.fails {
                return Err(WorktreeListError::Read(io::Error::other(
                    "worktree scan failed",
                )));
            }
            Ok(self.present.clone())
        }
    }

    fn worktrees_present(ids: &[&str]) -> FakeWorktrees {
        FakeWorktrees {
            present: ids.iter().map(|s| s.to_string()).collect(),
            fails: false,
        }
    }

    fn no_worktrees() -> FakeWorktrees {
        FakeWorktrees {
            present: BTreeSet::new(),
            fails: false,
        }
    }

    fn failing_worktrees() -> FakeWorktrees {
        FakeWorktrees {
            present: BTreeSet::new(),
            fails: true,
        }
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
            priority: None,
            created_at: "2026-07-13".to_string(),
        }
    }

    fn parent() -> DocView {
        DocView {
            id: "STORY-060".to_string(),
            doc_type: "story".to_string(),
            title: "Execute one iteration end-to-end".to_string(),
            body: "As an operator, I want one eligible iteration to flow through.".to_string(),
            status: "in-progress".to_string(),
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
        let tracker = FakeTracker::new(candidate(), parent())
            .with_lookup("STORY-060", story_doc("STORY-060", "complete"));
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
            |_| {},
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
        let tracker = FakeTracker::new(candidate(), parent())
            .with_lookup("STORY-060", story_doc("STORY-060", "complete"));
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
            |_| {},
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
            fail_advance: false,
            lookups: HashMap::new(),
            lookup_fails: false,
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
            |_| {},
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
            fail_advance: false,
            lookups: HashMap::new(),
            lookup_fails: false,
        };

        let after_expiry = NOW + DEFAULT_LEASE_TTL.as_millis() as u64 + 1;
        let report = reconcile(
            &store,
            &tracker,
            &no_worktrees(),
            &RoleMapping::adr003_default(),
            after_expiry,
        )
        .unwrap();

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

    // STORY-016 AC3 (store side): a single pass releases every expired orphan,
    // so reconcile hands the daemon N ids to project one release line each.
    #[test]
    fn reconcile_releases_all_expired_orphans() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("store.redb")).unwrap();
        store
            .claim("ITER-100", HOLDER, NOW, DEFAULT_LEASE_TTL)
            .unwrap();
        store
            .claim("ITER-101", HOLDER, NOW, DEFAULT_LEASE_TTL)
            .unwrap();

        // The tracker offers nothing, so no released id is dispatch-eligible.
        let tracker = FakeTracker {
            candidate: Mutex::new(None),
            parent: parent(),
            advances: Arc::new(Mutex::new(Vec::new())),
            drop_after_claim: false,
            fail_advance: false,
            lookups: HashMap::new(),
            lookup_fails: false,
        };

        let after_expiry = NOW + DEFAULT_LEASE_TTL.as_millis() as u64 + 1;
        let mut report = reconcile(
            &store,
            &tracker,
            &no_worktrees(),
            &RoleMapping::adr003_default(),
            after_expiry,
        )
        .unwrap();
        report.released.sort();

        assert_eq!(
            report.released,
            vec!["ITER-100".to_string(), "ITER-101".to_string()]
        );
        assert!(report.retained.is_empty());
        assert!(report.re_offered.is_empty(), "{report:?}");
        assert_eq!(store.get("ITER-100").unwrap(), None);
        assert_eq!(store.get("ITER-101").unwrap(), None);
    }

    #[test]
    fn reconcile_leaves_a_live_lease_in_place() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("store.redb")).unwrap();
        store
            .claim("ITER-014", HOLDER, NOW, DEFAULT_LEASE_TTL)
            .unwrap();
        let tracker = FakeTracker::new(candidate(), parent());

        let report = reconcile(
            &store,
            &tracker,
            &worktrees_present(&["ITER-014"]),
            &RoleMapping::adr003_default(),
            NOW + 1,
        )
        .unwrap();

        assert_eq!(report.retained, vec!["ITER-014".to_string()]);
        assert!(report.released.is_empty());
        assert!(store.get("ITER-014").unwrap().is_some());
    }

    fn iteration_doc(id: &str, status: &str) -> DocLookup {
        DocLookup::Present(DocView {
            id: id.to_string(),
            doc_type: "iteration".to_string(),
            title: String::new(),
            body: String::new(),
            status: status.to_string(),
        })
    }

    /// A parent work item as lazyspec really reports it: a story, not the
    /// dispatchable `iteration` type. The gate must treat its terminal status as
    /// terminal even though `story` is not in `dispatch.types` (STORY-061).
    fn story_doc(id: &str, status: &str) -> DocLookup {
        DocLookup::Present(DocView {
            id: id.to_string(),
            doc_type: "story".to_string(),
            title: String::new(),
            body: String::new(),
            status: status.to_string(),
        })
    }

    fn tracker_with_lookups(lookups: HashMap<String, DocLookup>) -> FakeTracker {
        FakeTracker {
            // Offer nothing: a released id must never also be dispatch-eligible.
            candidate: Mutex::new(None),
            parent: parent(),
            advances: Arc::new(Mutex::new(Vec::new())),
            drop_after_claim: false,
            fail_advance: false,
            lookups,
            lookup_fails: false,
        }
    }

    fn live_claim_store() -> (TempDir, Store) {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("store.redb")).unwrap();
        store
            .claim("ITER-014", HOLDER, NOW, DEFAULT_LEASE_TTL)
            .unwrap();
        (dir, store)
    }

    // STORY-017 AC1: a live-lease claim whose document lazyspec no longer has is
    // dropped and recorded.
    #[test]
    fn reconcile_drops_a_claim_whose_doc_is_absent() {
        let (_dir, store) = live_claim_store();
        let tracker =
            tracker_with_lookups(HashMap::from([("ITER-014".to_string(), DocLookup::Absent)]));

        let report = reconcile(
            &store,
            &tracker,
            &no_worktrees(),
            &RoleMapping::adr003_default(),
            NOW + 1,
        )
        .unwrap();

        assert_eq!(report.dropped, vec!["ITER-014".to_string()]);
        assert!(report.retained.is_empty());
        assert!(report.released.is_empty());
        assert_eq!(
            store.get("ITER-014").unwrap(),
            None,
            "claim must be dropped"
        );
    }

    // STORY-017 AC2: a live-lease claim lazyspec reports terminal-complete is
    // finalized (released) and never re-offered.
    #[test]
    fn reconcile_finalizes_a_terminal_complete_claim_without_re_offer() {
        let (_dir, store) = live_claim_store();
        let tracker = tracker_with_lookups(HashMap::from([(
            "ITER-014".to_string(),
            iteration_doc("ITER-014", "complete"),
        )]));

        let report = reconcile(
            &store,
            &tracker,
            &no_worktrees(),
            &RoleMapping::adr003_default(),
            NOW + 1,
        )
        .unwrap();

        assert_eq!(report.finalized, vec!["ITER-014".to_string()]);
        assert!(report.re_offered.is_empty(), "{report:?}");
        assert!(report.retained.is_empty());
        assert_eq!(
            store.get("ITER-014").unwrap(),
            None,
            "a completed item's claim must be released"
        );
        assert!(
            tracker.fetch_dispatchable().unwrap().is_empty(),
            "a completed item must not be dispatched again"
        );
    }

    // STORY-017 AC3: a live-lease claim lazyspec still reports active is retained.
    #[test]
    fn reconcile_retains_a_claim_whose_doc_is_active() {
        let (_dir, store) = live_claim_store();
        let tracker = tracker_with_lookups(HashMap::from([(
            "ITER-014".to_string(),
            iteration_doc("ITER-014", "in-progress"),
        )]));

        let report = reconcile(
            &store,
            &tracker,
            &worktrees_present(&["ITER-014"]),
            &RoleMapping::adr003_default(),
            NOW + 1,
        )
        .unwrap();

        assert_eq!(report.retained, vec!["ITER-014".to_string()]);
        assert!(report.dropped.is_empty());
        assert!(report.finalized.is_empty());
        assert!(store.get("ITER-014").unwrap().is_some());
    }

    // STORY-017 AC4: when lazyspec cannot be read, reconcile mutates nothing —
    // not even an expired orphan — and returns so the next cycle retries.
    #[test]
    fn reconcile_read_failure_retains_every_claim_and_returns() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("store.redb")).unwrap();
        // A live lease (forces a lazyspec lookup) and an expired orphan that
        // would otherwise be released.
        store
            .claim("ITER-014", HOLDER, NOW, DEFAULT_LEASE_TTL)
            .unwrap();
        store
            .claim("ITER-900", HOLDER, NOW - 10, Duration::from_millis(1))
            .unwrap();
        let tracker = FakeTracker {
            candidate: Mutex::new(None),
            parent: parent(),
            advances: Arc::new(Mutex::new(Vec::new())),
            drop_after_claim: false,
            fail_advance: false,
            lookups: HashMap::new(),
            lookup_fails: true,
        };

        let result = reconcile(
            &store,
            &tracker,
            &no_worktrees(),
            &RoleMapping::adr003_default(),
            NOW + 1,
        );

        assert!(
            matches!(result, Err(ReconcileError::Tracker(_))),
            "a read failure must surface as an error to retry: {result:?}"
        );
        assert!(
            store.get("ITER-014").unwrap().is_some(),
            "the live lease must be untouched"
        );
        assert!(
            store.get("ITER-900").unwrap().is_some(),
            "even the expired orphan must be untouched on a read failure"
        );
    }

    // STORY-065 AC1: a live-lease claim whose worktree is present on disk is
    // retained untouched, and its tree is not reported as orphaned.
    #[test]
    fn reconcile_retains_a_live_lease_whose_worktree_is_present() {
        let (_dir, store) = live_claim_store();
        let tracker = tracker_with_lookups(HashMap::from([(
            "ITER-014".to_string(),
            iteration_doc("ITER-014", "in-progress"),
        )]));

        let report = reconcile(
            &store,
            &tracker,
            &worktrees_present(&["ITER-014"]),
            &RoleMapping::adr003_default(),
            NOW + 1,
        )
        .unwrap();

        assert_eq!(report.retained, vec!["ITER-014".to_string()]);
        assert!(report.abandoned.is_empty());
        assert!(report.orphaned_worktrees.is_empty());
        assert!(
            store.get("ITER-014").unwrap().is_some(),
            "a live lease with its worktree must be left untouched"
        );
    }

    // STORY-065 AC2: a live-lease claim whose worktree has vanished cannot resume,
    // so it is released and recorded as abandoned.
    #[test]
    fn reconcile_releases_a_claim_whose_worktree_is_absent() {
        let (_dir, store) = live_claim_store();
        let tracker = tracker_with_lookups(HashMap::from([(
            "ITER-014".to_string(),
            iteration_doc("ITER-014", "in-progress"),
        )]));

        let report = reconcile(
            &store,
            &tracker,
            &no_worktrees(),
            &RoleMapping::adr003_default(),
            NOW + 1,
        )
        .unwrap();

        assert_eq!(report.abandoned, vec!["ITER-014".to_string()]);
        assert!(report.retained.is_empty());
        assert!(report.orphaned_worktrees.is_empty());
        assert_eq!(
            store.get("ITER-014").unwrap(),
            None,
            "a claim whose worktree vanished must be released"
        );
    }

    // STORY-065 AC3: a worktree on disk with no matching claim is reported for
    // cleanup and left on disk (deletion is STORY-044).
    #[test]
    fn reconcile_reports_an_orphaned_worktree_without_deleting_it() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("store.redb")).unwrap();
        let root = dir.path().join("workspaces");
        let orphan = root.join("ITER-777");
        std::fs::create_dir_all(&orphan).unwrap();

        // No claims and nothing dispatch-eligible: the tree stands alone.
        let tracker = tracker_with_lookups(HashMap::new());

        let report = reconcile(
            &store,
            &tracker,
            &DiskWorktrees::new(root.clone()),
            &RoleMapping::adr003_default(),
            NOW + 1,
        )
        .unwrap();

        assert_eq!(report.orphaned_worktrees, vec!["ITER-777".to_string()]);
        assert!(
            orphan.is_dir(),
            "an orphaned worktree must be reported, not deleted"
        );
    }

    // STORY-065 AC4: when the worktree listing fails, reconcile mutates no claim —
    // not even an expired orphan — reports no orphan, and returns so the next cycle
    // retries.
    #[test]
    fn reconcile_worktree_listing_failure_retains_every_claim_and_returns() {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("store.redb")).unwrap();
        store
            .claim("ITER-014", HOLDER, NOW, DEFAULT_LEASE_TTL)
            .unwrap();
        store
            .claim("ITER-900", HOLDER, NOW - 10, Duration::from_millis(1))
            .unwrap();
        let tracker = tracker_with_lookups(HashMap::from([(
            "ITER-014".to_string(),
            iteration_doc("ITER-014", "in-progress"),
        )]));

        let result = reconcile(
            &store,
            &tracker,
            &failing_worktrees(),
            &RoleMapping::adr003_default(),
            NOW + 1,
        );

        assert!(
            matches!(result, Err(ReconcileError::Worktree(_))),
            "a worktree listing failure must surface as an error to retry: {result:?}"
        );
        assert!(
            store.get("ITER-014").unwrap().is_some(),
            "the live lease must be untouched"
        );
        assert!(
            store.get("ITER-900").unwrap().is_some(),
            "even the expired orphan must be untouched on a listing failure"
        );
    }

    // AC3: a candidate the store already holds a live claim for is skipped at
    // selection and never dispatched again — the existing claim is left intact.
    #[tokio::test]
    async fn a_candidate_with_a_live_store_claim_is_not_dispatched() {
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
            |_| {},
        )
        .await;

        assert_eq!(
            report,
            TickReport::Idle,
            "an item with a live claim must not be dispatched again: {report:?}"
        );
        assert_eq!(
            store.get("ITER-014").unwrap().unwrap().holder,
            "other-agent",
            "the live claim must be left untouched"
        );
    }

    // STORY-019 AC1: a rejected advance still committed a claim (then released
    // it) before the tick reported Skipped — that must be distinguishable from
    // a lost claim so the daemon's projection can log both the claim and the
    // release, not neither (see daemon.rs's post-commit seam).
    #[tokio::test]
    async fn advance_gated_out_is_skipped_with_claim_committed_then_released() {
        let repo = init_repo();
        let store = store(repo.path());
        let tracker = FakeTracker {
            candidate: Mutex::new(Some(candidate())),
            parent: parent(),
            advances: Arc::new(Mutex::new(Vec::new())),
            drop_after_claim: false,
            fail_advance: true,
            lookups: HashMap::from([("STORY-060".to_string(), story_doc("STORY-060", "complete"))]),
            lookup_fails: false,
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
            |_| {},
        )
        .await;

        assert!(
            matches!(
                report,
                TickReport::Skipped {
                    claim: SkipClaim::ClaimedThenReleased,
                    ..
                }
            ),
            "a gated-out advance must report that the claim was committed then released: {report:?}"
        );
        assert_eq!(
            store.get("ITER-014").unwrap(),
            None,
            "the claim must have been durably released"
        );
    }

    /// A dispatch-role candidate (`ITER-014`) with a chosen parent link and
    /// dependency set, for exercising the blocker gate. Its parent DocView (for
    /// prompt assembly) is still `parent()`; the gate reads prerequisites via the
    /// tracker's `lookups` map.
    fn candidate_with(parent: Option<&str>, dependencies: Vec<DependencyRef>) -> Candidate {
        Candidate {
            id: "ITER-014".to_string(),
            identifier: "gated".to_string(),
            title: "Execute one iteration end-to-end".to_string(),
            body: "Objective: prove the gate.".to_string(),
            state: "accepted".to_string(),
            parent: parent.map(|p| p.to_string()),
            dependencies,
            priority: None,
            created_at: "2026-07-13".to_string(),
        }
    }

    /// A minimal candidate carrying only the fields `dispatch_order` reads, for
    /// exercising the ordering directly over a constructed set (STORY-004).
    fn ranked(identifier: &str, priority: Option<u32>, created_at: &str) -> Candidate {
        Candidate {
            id: identifier.to_string(),
            identifier: identifier.to_string(),
            title: String::new(),
            body: String::new(),
            state: "accepted".to_string(),
            parent: None,
            dependencies: Vec::new(),
            priority,
            created_at: created_at.to_string(),
        }
    }

    fn order(mut candidates: Vec<Candidate>) -> Vec<String> {
        candidates.sort_by(dispatch_order);
        candidates.into_iter().map(|c| c.identifier).collect()
    }

    // STORY-004 AC1: with mixed priorities, the lowest priority number dispatches
    // first.
    #[test]
    fn lowest_priority_number_sorts_first() {
        let ordered = order(vec![
            ranked("c", Some(5), "2026-07-13"),
            ranked("a", Some(1), "2026-07-13"),
            ranked("b", Some(3), "2026-07-13"),
        ]);

        assert_eq!(ordered, vec!["a", "b", "c"]);
    }

    // STORY-004 AC2: equal priority breaks to the oldest created_at, then the
    // identifier lexicographically when created_at is also equal.
    #[test]
    fn equal_priority_breaks_by_age_then_identifier() {
        let ordered = order(vec![
            ranked("z", Some(2), "2026-07-13"),
            ranked("a", Some(2), "2026-07-13"),
            ranked("older", Some(2), "2026-07-10"),
        ]);

        assert_eq!(
            ordered,
            vec!["older", "a", "z"],
            "oldest first, then identifier for the created_at tie"
        );
    }

    // STORY-004 AC3: a None priority sorts after every defined priority, even a
    // large one.
    #[test]
    fn missing_priority_sorts_after_all_defined() {
        let ordered = order(vec![
            ranked("none", None, "2020-01-01"),
            ranked("big", Some(9999), "2026-07-13"),
            ranked("small", Some(1), "2026-07-13"),
        ]);

        assert_eq!(
            ordered,
            vec!["small", "big", "none"],
            "an older, unprioritised item must still sort after a large defined priority"
        );
    }

    // STORY-004: two unprioritised candidates still order deterministically by
    // age then identifier.
    #[test]
    fn two_missing_priorities_fall_through_to_age_and_identifier() {
        let ordered = order(vec![
            ranked("b", None, "2026-07-13"),
            ranked("a", None, "2026-07-13"),
            ranked("older", None, "2026-07-01"),
        ]);

        assert_eq!(ordered, vec!["older", "a", "b"]);
    }

    fn blocked_by(target: &str) -> DependencyRef {
        DependencyRef {
            target: target.to_string(),
            kind: DependencyKind::BlockedBy,
        }
    }

    async fn tick(store: &Store, tracker: &FakeTracker, repo: &Path) -> TickReport {
        let adapter = FakeAdapter::new(completed_report());
        run_tick(
            tracker,
            store,
            &adapter,
            &config(),
            crate::prompt::DEFAULT_TEMPLATE,
            repo,
            NOW,
            HOLDER,
            |_| {},
        )
        .await
    }

    // STORY-061 AC1: a candidate with a `blocked-by` dependency that is not
    // terminal is held back, not dispatched, and the reason names the dependency.
    #[tokio::test]
    async fn a_non_terminal_dependency_blocks_dispatch_with_a_reason() {
        let repo = init_repo();
        let store = store(repo.path());
        let tracker =
            FakeTracker::new(candidate_with(None, vec![blocked_by("ITER-050")]), parent())
                .with_lookup("ITER-050", iteration_doc("ITER-050", "in-progress"));

        let report = tick(&store, &tracker, repo.path()).await;

        let blocked = match report {
            TickReport::Blocked(blocked) => blocked,
            other => panic!("expected Blocked, got {other:?}"),
        };
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].id, "ITER-014");
        assert!(
            blocked[0].reason.contains("ITER-050"),
            "reason must name the blocking dependency: {}",
            blocked[0].reason
        );
        assert_eq!(
            store.get("ITER-014").unwrap(),
            None,
            "a blocked candidate must never be claimed"
        );
    }

    // STORY-061 AC2: a candidate whose parent work item is not terminal-complete
    // is held back as blocked.
    #[tokio::test]
    async fn a_non_terminal_parent_blocks_dispatch() {
        let repo = init_repo();
        let store = store(repo.path());
        let tracker = FakeTracker::new(candidate_with(Some("STORY-060"), Vec::new()), parent())
            .with_lookup("STORY-060", story_doc("STORY-060", "in-progress"));

        let report = tick(&store, &tracker, repo.path()).await;

        let blocked = match report {
            TickReport::Blocked(blocked) => blocked,
            other => panic!("expected Blocked, got {other:?}"),
        };
        assert!(
            blocked[0].reason.contains("STORY-060"),
            "reason must name the blocking parent: {}",
            blocked[0].reason
        );
        assert_eq!(store.get("ITER-014").unwrap(), None);
    }

    // STORY-061 AC3: when every `blocked-by` dependency and the parent are
    // terminal-complete, the candidate becomes dispatch-eligible and is claimed.
    #[tokio::test]
    async fn all_prerequisites_terminal_dispatches_the_candidate() {
        let repo = init_repo();
        let store = store(repo.path());
        let tracker = FakeTracker::new(
            candidate_with(Some("STORY-060"), vec![blocked_by("ITER-050")]),
            parent(),
        )
        .with_lookup("STORY-060", story_doc("STORY-060", "complete"))
        .with_lookup("ITER-050", iteration_doc("ITER-050", "complete"));

        let report = tick(&store, &tracker, repo.path()).await;

        assert!(
            matches!(report, TickReport::Dispatched(_)),
            "all prerequisites terminal must dispatch: {report:?}"
        );
        assert_eq!(
            store.get("ITER-014").unwrap().unwrap().holder,
            HOLDER,
            "a dispatched candidate must hold the claim"
        );
    }

    // STORY-061 AC4: a dependency reference lazyspec cannot resolve is treated as
    // blocked, not silently dispatched, and the unresolved ref is surfaced.
    #[tokio::test]
    async fn an_unresolvable_dependency_blocks_and_surfaces_the_ref() {
        let repo = init_repo();
        let store = store(repo.path());
        let tracker =
            FakeTracker::new(candidate_with(None, vec![blocked_by("ITER-999")]), parent())
                .with_lookup("ITER-999", DocLookup::Absent);

        let report = tick(&store, &tracker, repo.path()).await;

        let blocked = match report {
            TickReport::Blocked(blocked) => blocked,
            other => panic!("expected Blocked, got {other:?}"),
        };
        assert!(
            blocked[0].reason.contains("ITER-999"),
            "an unresolved ref must be surfaced: {}",
            blocked[0].reason
        );
        assert_eq!(
            store.get("ITER-014").unwrap(),
            None,
            "an unresolvable prerequisite must never be dispatched"
        );
    }

    // A `lookup_doc` failure while gating is conservative: it surfaces as
    // TickReport::Error, never a dispatch (STORY-061 — never silently dispatch).
    #[tokio::test]
    async fn a_gate_lookup_failure_is_an_error_not_a_dispatch() {
        let repo = init_repo();
        let store = store(repo.path());
        let mut tracker = FakeTracker::new(candidate_with(Some("STORY-060"), Vec::new()), parent());
        tracker.lookup_fails = true;

        let report = tick(&store, &tracker, repo.path()).await;

        assert!(
            matches!(report, TickReport::Error(_)),
            "a gate lookup failure must surface as an error: {report:?}"
        );
        assert_eq!(
            store.get("ITER-014").unwrap(),
            None,
            "a lookup failure must never dispatch"
        );
    }

    // Out of scope guard: the reverse `blocks` edge does not gate dispatch. A
    // non-terminal `blocks` dependency, with the parent terminal, still dispatches.
    #[tokio::test]
    async fn a_blocks_edge_does_not_gate_dispatch() {
        let repo = init_repo();
        let store = store(repo.path());
        let blocks = DependencyRef {
            target: "ITER-051".to_string(),
            kind: DependencyKind::Blocks,
        };
        let tracker = FakeTracker::new(candidate_with(Some("STORY-060"), vec![blocks]), parent())
            .with_lookup("STORY-060", story_doc("STORY-060", "complete"))
            .with_lookup("ITER-051", iteration_doc("ITER-051", "in-progress"));

        let report = tick(&store, &tracker, repo.path()).await;

        assert!(
            matches!(report, TickReport::Dispatched(_)),
            "a `blocks` edge must not gate dispatch: {report:?}"
        );
    }
}
