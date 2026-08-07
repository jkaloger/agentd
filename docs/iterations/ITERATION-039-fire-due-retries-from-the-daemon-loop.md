---
title: Fire due retries from the daemon loop
type: iteration
status: complete
author: Jack Kaloger
date: 2026-08-06
tags: []
related:
- implements: BUG-002
---
## Objective
Wire due-retry firing into the daemon's orchestrator loop and reshape the handler so a fired retry can actually re-dispatch: decide from the item's real state, and spawn a tracked worker rather than awaiting one inline.

## Context
- Fixes: BUG-002 (both defects and the repro are there).
- Architecture: [[ADR-008]] — dispatch spawns a tracked worker; it does not await. A fired retry is a dispatch and must obey that. [[ADR-007]] — caps are a ceiling on the running set, so a retry competes for a slot like any other dispatch.
- `src/tick.rs` `fire_due_retries` exists, is `#[allow(dead_code)]`, decides presence via `fetch_dispatchable()`, and awaits `run_worker` inline. Two things are wrong: nobody calls it, and its presence check asks the wrong question.
- `fetch_dispatchable` only offers **dispatch-role** documents. At fire time a retried item is never in a dispatch role: continuation and backoff retries follow an advance to a terminal state, and a stall-killed item is still in the **active** state. Decide from `lookup_doc` plus the role mapping, the way `reconcile_running` already does — that is the established seam for "what state is this item actually in".
- The three producers are in `src/daemon.rs`: clean-exit continuation, failure backoff, and the stall kill.
- `src/tick.rs` retry tests currently pass because `FakeTracker` offers the retried item as an `accepted` candidate — a state a real retried item cannot occupy. Those fakes must be corrected to the real states, and the corrected tests must be shown to fail against the current handler before the fix.

### Decision: what a fired retry does for a terminal item

Recorded during execution of task 2. A terminal item's retry is a **release**, not a re-dispatch.

- Re-dispatch goes through `claim_and_activate`, which advances the doc to the active state. The lifecycle DAG has no edge from any terminal status back to a dispatch or active status (`.lazyspec.toml`: the only outbound edge from a terminal state is `* -> superseded`). Re-dispatching a terminal item would mean inventing a transition the DAG forbids; lazyspec would gate-reject it and the claim would be dropped anyway.
- It is what the stories promise. STORY-007 AC3: "Given the item is no longer active, When the timer fires, Then the claim is released without re-dispatch." STORY-009 AC1 says the same for an item absent from candidates. A terminal item is the "no longer active" case.
- The fire still *resolves*: claim released, durable retry entry cleared. BUG-002's symptom is the claim being held indefinitely, and release cures it. "Re-dispatches without operator action" is the promise for an item that is still live, not for one lazyspec says is finished.

Attempt escalation across re-dispatches (AC2) therefore rides the **active** arm, not the terminal one. A failed run whose advance to the failure target is gate-rejected — which is the shipped case here, since `in-progress -> rejected` has no edge — leaves the item in the active state, so its backoff fires, re-dispatches, and escalates. A project whose mapping does land failures in a dispatch role gets the same escalation through the dispatch arm.

The fire decision by role, from `lookup_doc` + `RoleMapping::classify` (the seam `reconcile_running` uses):

| lookup | role | action |
| --- | --- | --- |
| `Present` | `Active` | re-dispatch, no advance — the item is already in the active state (stall case, and gate-rejected failure) |
| `Present` | `Dispatch` | re-dispatch through the normal claim-and-activate path |
| `Present` | `Terminal` or unmapped | release the claim, clear the retry entry |
| `Absent` | — | release the claim, clear the retry entry |
| `Err` | — | requeue, leave the schedule untouched (STORY-009 AC4) |

Re-dispatchable but no free slot requeues with "no available orchestrator slots" (STORY-009 AC3).

## Tasks
1. First, fix the fakes so they model reality: a continuation retry's item is in the success target, a backoff retry's item is in the failure target, a stall retry's item is still in the active state. Confirm the existing retry tests now fail — that failure is BUG-002's second defect.
2. Reshape the fire decision to consult `lookup_doc` + role rather than the dispatch candidate list, and define what each role means at fire time: an item still in the active state is the stall case and is re-dispatchable; a terminal item is the continuation/backoff case — decide per role whether it re-dispatches or the claim is released, and say why in the doc if it differs from the naive reading.
3. Make re-dispatch spawn a tracked worker through the same path a normal dispatch uses, competing for a slot under the same caps. No inline await. If no slot is free, requeue rather than dropping the retry.
4. Call it from the orchestrator loop each tick, and drop the `#[allow(dead_code)]`.
5. Thread the run's own `attempt` through the completion path so failure backoff escalates from the attempt that actually ran, instead of reading it back off the prior durable entry. Cover the escalate-across-re-dispatch case with a test.
6. Clear the durable retry entry and the matching `DaemonState.queued` row when a retry resolves, and write a `queued` row on the stall path so the operator surface matches the failure path.
7. Guard the completion-racing-abort case: a completion arriving after `stop_running` released the claim must not schedule a continuation for an item the daemon no longer owns.

## Acceptance Criteria
- A clean exit's continuation fires and re-dispatches without operator action; the claim is not held indefinitely.
- A failed exit's backoff fires after its delay and escalates attempt-over-attempt across re-dispatches.
- A stall-killed item is re-dispatched, not stranded.
- Firing spawns a tracked worker under the concurrency caps; nothing in the loop awaits a worker turn.
- Retry entries do not accumulate in redb across a daemon's life; `queued` rows appear for stalls and disappear on recovery.
- Tests assert against the states items are really in at fire time, and the pre-fix versions of those tests fail.
- `cargo test` green, `cargo clippy --all-targets` clean.

## Out of scope
- The agent-process kill (BUG-003 / its own iteration).
- Per-tick reconcile ordering, the progress-event storm, and the `dispatch_error` socket surface — chunk-review nits, handled separately.



