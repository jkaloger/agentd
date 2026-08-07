use std::fmt;
use std::time::Duration;

use crate::store::{ClaimError, ClaimRecord, Store, StoreError};
use crate::tracker::{Candidate, Tracker, TrackerError};

/// The claim handshake for one candidate (ADR-003, STORY-025): durably claim it,
/// then advance it to the active state via the tracker *before* the agent runs.
///
/// `start_agent` is the success-only seam — it is reached solely after both the
/// store claim and the advance succeed, so an agent can never run against an
/// item that was not activated. On an advance failure the claim is released and
/// the agent hook is never invoked.
#[allow(clippy::too_many_arguments)]
pub fn claim_and_activate<T, F>(
    tracker: &T,
    store: &Store,
    candidate: &Candidate,
    holder: &str,
    now: u64,
    lease_ttl: Duration,
    active_state: &str,
    start_agent: F,
) -> Result<ClaimRecord, DispatchError>
where
    T: Tracker,
    F: FnOnce(),
{
    let record = store
        .claim(&candidate.id, holder, now, lease_ttl)
        .map_err(DispatchError::Claim)?;

    activate_or_release(tracker, store, &candidate.id, active_state)?;

    start_agent();
    Ok(record)
}

/// Advance a claimed `id` to the active state, releasing its claim when the
/// tracker refuses — an item that could not be activated must not be left held.
pub fn activate_or_release<T: Tracker>(
    tracker: &T,
    store: &Store,
    id: &str,
    active_state: &str,
) -> Result<(), DispatchError> {
    if let Err(advance_err) = tracker.advance(id, active_state) {
        store.release(id).map_err(DispatchError::Release)?;
        return Err(DispatchError::Advance(advance_err));
    }
    Ok(())
}

#[derive(Debug)]
pub enum DispatchError {
    Claim(ClaimError),
    Advance(TrackerError),
    Release(StoreError),
}

impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DispatchError::Claim(e) => write!(f, "cannot claim item: {e:?}"),
            DispatchError::Advance(e) => write!(f, "cannot advance claimed item: {e}"),
            DispatchError::Release(e) => {
                write!(f, "cannot release claim after a failed advance: {e:?}")
            }
        }
    }
}

impl std::error::Error for DispatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DispatchError::Advance(e) => Some(e),
            DispatchError::Claim(_) | DispatchError::Release(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use tempfile::TempDir;

    use crate::mapping::RoleMapping;
    use crate::tracker::{CliFailure, CommandRunner, LazyspecTracker};

    const TTL: Duration = Duration::from_secs(60);
    const ACTIVE: &str = "in-progress";

    /// Shared observations of what the runner was asked to do, kept outside the
    /// runner so the test can inspect them after the runner is moved into the
    /// tracker.
    #[derive(Clone, Default)]
    struct Recorder {
        calls: Arc<Mutex<Vec<String>>>,
        claim_present_at_advance: Arc<Mutex<Option<bool>>>,
    }

    /// A CommandRunner backed by the same store the handshake writes to, so the
    /// advance call can witness whether the claim already exists (proving order).
    struct FakeCli {
        store: Arc<Store>,
        listing: String,
        advance_ok: bool,
        recorder: Recorder,
    }

    impl FakeCli {
        fn new(store: Arc<Store>, advance_ok: bool) -> Self {
            FakeCli {
                store,
                listing: r#"{"documents":[]}"#.to_string(),
                advance_ok,
                recorder: Recorder::default(),
            }
        }

        fn with_listing(mut self, listing: &str) -> Self {
            self.listing = listing.to_string();
            self
        }

        fn recorder(&self) -> Recorder {
            self.recorder.clone()
        }
    }

    impl CommandRunner for FakeCli {
        fn run(&self, args: &[&str]) -> Result<Vec<u8>, CliFailure> {
            self.recorder.calls.lock().unwrap().push(args.join(" "));
            match args {
                ["status", "--json"] => Ok(self.listing.clone().into_bytes()),
                ["show", _, "--json"] => Ok(br#"{"body":""}"#.to_vec()),
                ["update", id, "--status", _] => {
                    let present = self.store.get(id).unwrap().is_some();
                    *self.recorder.claim_present_at_advance.lock().unwrap() = Some(present);
                    if self.advance_ok {
                        Ok(Vec::new())
                    } else {
                        Err(CliFailure::Exit {
                            code: Some(1),
                            stderr: r#"invalid transition for type "iteration": no edge from "accepted" to "in-progress" (allowed targets: review, superseded)"#.to_string(),
                        })
                    }
                }
                other => panic!("unexpected args: {other:?}"),
            }
        }
    }

    fn store(dir: &TempDir) -> Arc<Store> {
        Arc::new(Store::open(&dir.path().join("claims.redb")).unwrap())
    }

    fn candidate(id: &str) -> Candidate {
        Candidate {
            id: id.to_string(),
            identifier: "claim-by-advancing".to_string(),
            title: "Claim by advancing".to_string(),
            body: String::new(),
            state: "accepted".to_string(),
            parent: None,
            dependencies: Vec::new(),
            priority: None,
            created_at: "2026-07-13".to_string(),
        }
    }

    #[test]
    fn success_claims_then_advances_before_the_agent_starts() {
        let dir = TempDir::new().unwrap();
        let store = store(&dir);
        let fake = FakeCli::new(store.clone(), true);
        let recorder = fake.recorder();
        let tracker = LazyspecTracker::new(fake, RoleMapping::adr003_default());
        let mut started = false;

        let record = claim_and_activate(
            &tracker,
            store.as_ref(),
            &candidate("ITER-008"),
            "agent-a",
            1000,
            TTL,
            ACTIVE,
            || started = true,
        )
        .unwrap();

        assert_eq!(record.holder, "agent-a");
        assert!(started, "agent-start hook must run on success");
        assert_eq!(
            recorder.calls.lock().unwrap().as_slice(),
            ["update ITER-008 --status in-progress".to_string()]
        );
        assert_eq!(
            *recorder.claim_present_at_advance.lock().unwrap(),
            Some(true),
            "the store claim must exist before the advance is issued"
        );
        assert_eq!(store.get("ITER-008").unwrap().unwrap().holder, "agent-a");
    }

    #[test]
    fn a_claimed_active_item_is_not_re_offered_as_a_fresh_candidate() {
        let dir = TempDir::new().unwrap();
        let store = store(&dir);
        let listing = r#"{"documents":[
            {"id":"ITER-008","path":"docs/iterations/ITER-008-x.md",
             "title":"X","status":"in-progress","type":"iteration","related":[]}
        ]}"#;
        let tracker = LazyspecTracker::new(
            FakeCli::new(store.clone(), true).with_listing(listing),
            RoleMapping::adr003_default(),
        );

        let fresh = tracker.fetch_dispatchable().unwrap();

        assert!(
            fresh.is_empty(),
            "an active (in-progress) item must not be dispatch-eligible: {fresh:?}"
        );
    }

    #[test]
    fn advance_failure_skips_the_agent_and_releases_the_claim() {
        let dir = TempDir::new().unwrap();
        let store = store(&dir);
        let tracker = LazyspecTracker::new(
            FakeCli::new(store.clone(), false),
            RoleMapping::adr003_default(),
        );
        let mut started = false;

        let err = claim_and_activate(
            &tracker,
            store.as_ref(),
            &candidate("ITER-008"),
            "agent-a",
            1000,
            TTL,
            ACTIVE,
            || started = true,
        )
        .unwrap_err();

        assert!(
            matches!(err, DispatchError::Advance(TrackerError::Gate { .. })),
            "a gate refusal must surface as an advance gate error: {err}"
        );
        assert!(!started, "agent must not start when the advance fails");
        assert_eq!(
            store.get("ITER-008").unwrap(),
            None,
            "the claim must be released after a failed advance"
        );
    }

    #[test]
    fn a_live_claim_blocks_a_second_claimant_without_advancing() {
        let dir = TempDir::new().unwrap();
        let store = store(&dir);
        store.claim("ITER-008", "agent-a", 1000, TTL).unwrap();
        let fake = FakeCli::new(store.clone(), true);
        let recorder = fake.recorder();
        let tracker = LazyspecTracker::new(fake, RoleMapping::adr003_default());
        let mut started = false;

        let err = claim_and_activate(
            &tracker,
            store.as_ref(),
            &candidate("ITER-008"),
            "agent-b",
            2000,
            TTL,
            ACTIVE,
            || started = true,
        )
        .unwrap_err();

        assert!(matches!(err, DispatchError::Claim(_)), "{err}");
        assert!(!started);
        assert!(
            recorder.calls.lock().unwrap().is_empty(),
            "no advance on a lost claim"
        );
    }
}
