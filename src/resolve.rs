use crate::adapter::TurnOutcome;
use crate::config::Transitions;
use crate::tracker::{Tracker, TrackerError};

/// Which config-mapped transition a finished run resolved to (ADR-003). Returned
/// so the caller can log/report which branch was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeBranch {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTransition {
    pub branch: OutcomeBranch,
    pub target_state: String,
}

/// Resolve a finished run's outcome into the config-mapped lazyspec transition
/// and apply it via `Tracker::advance` (ADR-003). A clean exit takes the SUCCESS
/// target — a terminal state or a configured handoff state; any failure,
/// timeout, or stall (surfaced by the adapter as `Failed`/`LaunchError`,
/// ADR-004) takes the FAILURE target.
///
/// A handoff is just a SUCCESS target that is not classified `dispatch`: the
/// candidate filter (`fetch_dispatchable`) only re-offers `dispatch`-role
/// states, so a handoff state — mapped terminal or left unmapped — is never
/// re-offered. This resolver does not need to special-case it.
pub fn resolve_outcome<T: Tracker>(
    tracker: &T,
    id: &str,
    outcome: &TurnOutcome,
    transitions: &Transitions,
) -> Result<ResolvedTransition, TrackerError> {
    let (branch, target) = match outcome {
        TurnOutcome::Completed => (OutcomeBranch::Success, &transitions.success),
        TurnOutcome::Failed { .. } | TurnOutcome::LaunchError { .. } => {
            (OutcomeBranch::Failure, &transitions.failure)
        }
    };
    tracker.advance(id, target)?;
    Ok(ResolvedTransition {
        branch,
        target_state: target.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::config::load_str;
    use crate::mapping::RoleMapping;
    use crate::tracker::{CliFailure, CommandRunner, LazyspecTracker};

    /// A recording runner: answers `status`/`show`/`update` and records the exact
    /// argv of every call. The call log is a shared handle so a test can inspect
    /// it after the runner is moved into the tracker.
    struct FakeCli {
        listing: String,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl FakeCli {
        fn new() -> Self {
            FakeCli {
                listing: r#"{"documents":[]}"#.to_string(),
                calls: Rc::new(RefCell::new(Vec::new())),
            }
        }

        fn with_listing(mut self, listing: &str) -> Self {
            self.listing = listing.to_string();
            self
        }

        fn calls(&self) -> Rc<RefCell<Vec<String>>> {
            Rc::clone(&self.calls)
        }
    }

    impl CommandRunner for FakeCli {
        fn run(&self, args: &[&str]) -> Result<Vec<u8>, CliFailure> {
            self.calls.borrow_mut().push(args.join(" "));
            match args {
                ["status", "--json"] => Ok(self.listing.clone().into_bytes()),
                ["show", _, "--json"] => Ok(br#"{"body":""}"#.to_vec()),
                ["update", _, "--status", _] => Ok(Vec::new()),
                other => panic!("unexpected args: {other:?}"),
            }
        }
    }

    fn default_transitions() -> Transitions {
        load_str("").unwrap().transitions
    }

    #[test]
    fn clean_success_runs_the_mapped_success_transition() {
        let fake = FakeCli::new();
        let calls = fake.calls();
        let tracker = LazyspecTracker::new(fake, RoleMapping::adr003_default());

        let resolved = resolve_outcome(
            &tracker,
            "ITER-013",
            &TurnOutcome::Completed,
            &default_transitions(),
        )
        .unwrap();

        assert_eq!(resolved.branch, OutcomeBranch::Success);
        assert_eq!(resolved.target_state, "complete");
        assert_eq!(
            calls.borrow().as_slice(),
            ["update ITER-013 --status complete".to_string()],
            "success must issue only the success transition, never the failure one",
        );
    }

    #[test]
    fn agent_failure_runs_the_mapped_failure_transition() {
        let fake = FakeCli::new();
        let calls = fake.calls();
        let tracker = LazyspecTracker::new(fake, RoleMapping::adr003_default());

        let resolved = resolve_outcome(
            &tracker,
            "ITER-013",
            &TurnOutcome::Failed {
                reason: "agent exited with status 3".to_string(),
            },
            &default_transitions(),
        )
        .unwrap();

        assert_eq!(resolved.branch, OutcomeBranch::Failure);
        assert_eq!(resolved.target_state, "rejected");
        assert_eq!(
            calls.borrow().as_slice(),
            ["update ITER-013 --status rejected".to_string()],
            "failure must issue only the failure transition, never the success one",
        );
    }

    #[test]
    fn launch_error_runs_the_mapped_failure_transition() {
        let fake = FakeCli::new();
        let calls = fake.calls();
        let tracker = LazyspecTracker::new(fake, RoleMapping::adr003_default());

        let resolved = resolve_outcome(
            &tracker,
            "ITER-013",
            &TurnOutcome::LaunchError {
                message: "cannot launch `claude`".to_string(),
            },
            &default_transitions(),
        )
        .unwrap();

        assert_eq!(resolved.branch, OutcomeBranch::Failure);
        assert_eq!(resolved.target_state, "rejected");
        assert_eq!(
            calls.borrow().as_slice(),
            ["update ITER-013 --status rejected".to_string()],
        );
    }

    #[test]
    fn a_timeout_or_stall_surfaced_as_failed_takes_the_failure_branch() {
        let tracker = LazyspecTracker::new(FakeCli::new(), RoleMapping::adr003_default());

        let resolved = resolve_outcome(
            &tracker,
            "ITER-013",
            &TurnOutcome::Failed {
                reason: "turn timed out after 900s (stalled)".to_string(),
            },
            &default_transitions(),
        )
        .unwrap();

        assert_eq!(resolved.branch, OutcomeBranch::Failure);
        assert_eq!(resolved.target_state, "rejected");
    }

    #[test]
    fn a_gate_rejected_advance_surfaces_a_typed_error() {
        struct RejectingCli;
        impl CommandRunner for RejectingCli {
            fn run(&self, _args: &[&str]) -> Result<Vec<u8>, CliFailure> {
                Err(CliFailure::Exit {
                    code: Some(1),
                    stderr: "gate rejected".to_string(),
                })
            }
        }
        let tracker = LazyspecTracker::new(RejectingCli, RoleMapping::adr003_default());

        let err = resolve_outcome(
            &tracker,
            "ITER-013",
            &TurnOutcome::Completed,
            &default_transitions(),
        )
        .unwrap_err();

        assert!(matches!(err, TrackerError::Command { .. }), "{err}");
    }

    /// AC3: a run lands in a configured handoff state on success, and that state
    /// is treated terminal-for-dispatch — the item is not re-offered as a fresh
    /// candidate by the same tracker's `fetch_dispatchable`.
    #[test]
    fn a_handoff_success_state_is_reached_and_not_re_offered() {
        let config = load_str(
            r#"
[transitions]
success = "handoff"

[dispatch]
types = ["iteration"]

[dispatch.states]
accepted = "dispatch"
"in-progress" = "active"
handoff = "terminal"
complete = "terminal"
rejected = "terminal"
"#,
        )
        .unwrap();
        let mapping = RoleMapping::from_config(&config);
        let handoff_listing = r#"{"documents":[
            {"id":"ITER-013","path":"docs/iterations/ITER-013-x.md",
             "title":"X","status":"handoff","type":"iteration","related":[]}
        ]}"#;
        let fake = FakeCli::new().with_listing(handoff_listing);
        let calls = fake.calls();
        let tracker = LazyspecTracker::new(fake, mapping);

        let resolved = resolve_outcome(
            &tracker,
            "ITER-013",
            &TurnOutcome::Completed,
            &config.transitions,
        )
        .unwrap();

        assert_eq!(resolved.branch, OutcomeBranch::Success);
        assert_eq!(resolved.target_state, "handoff");
        assert!(
            calls
                .borrow()
                .contains(&"update ITER-013 --status handoff".to_string()),
            "success must advance the item to the configured handoff state",
        );

        let fresh = tracker.fetch_dispatchable().unwrap();
        assert!(
            fresh.is_empty(),
            "an item in the handoff state must be terminal-for-dispatch, not re-offered: {fresh:?}",
        );
    }
}
