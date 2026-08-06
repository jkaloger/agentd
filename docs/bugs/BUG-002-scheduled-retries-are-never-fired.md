---
title: Scheduled retries are never fired
type: bug
status: reported
author: Jack Kaloger
date: 2026-08-06
tags: []
related:
- related-to: MILESTONE-003
- related-to: ADR-008
---

## Expected vs actual

**Expected:** the daemon consumes the retry schedules it writes. A clean worker exit schedules a continuation and the daemon re-dispatches when it comes due (STORY-007); a failed exit schedules a backoff and the daemon re-dispatches after the delay (STORY-008/STORY-009); a stall kill schedules an immediate retry and the daemon re-dispatches it (STORY-011). MILESTONE-003's releasable outcome is a daemon that "dispatches, retries, and cleans up correctly with no operator action".

**Actual:** nothing ever fires a due retry. `fire_due_retries` (`src/tick.rs`) carries `#[allow(dead_code)]` and is called only from tests; the orchestrator loop (`src/daemon.rs`) never calls it, nor `store.due_retries`. Three producers write schedules — continuation, failure backoff, stall kill — and no consumer exists. Retry entries accumulate in redb for the life of the daemon.

Second, independent defect: even once wired, the handler can never re-dispatch. It decides an item is "present" by finding its id in `fetch_dispatchable()`, which offers **dispatch-role** states only. At fire time none of the three producers leave the item in a dispatch role — a clean exit advanced it to the success target (terminal), a failed exit to the failure target (terminal `rejected`), and a stall-killed item still sits in the **active** state because its worker was aborted before any transition. All three take the `Released` arm; `Redispatched`/`Requeued` are unreachable under any shipped role mapping.

## Repro

1. Run the daemon against a backlog with one eligible iteration.
2. Let the agent exit cleanly. The claim is retained and an attempt-1 continuation is scheduled ~1s out.
3. Wait any number of poll intervals. No re-dispatch occurs; the claim is held until the daemon restarts.
4. Same for a failed exit (backoff never honoured) and a stalled agent (killed, then stranded in its active state with no agent and no claim).

## Why the tests did not catch it

`tick.rs`'s retry tests pass because `FakeTracker` offers the retried item as an `accepted` candidate regardless of provenance — a state a real retried item cannot occupy. The unit tests exercise a shape production never produces.

## Notes

ITERATION-031 deferred the wiring to "a daemon-loop iteration"; no document ever owned it. The handler also awaits `run_worker` inline, which contradicts [[ADR-008]] ("dispatch spawns; it does not await"), so it needs reshaping, not just calling.

Related defects in the same pipeline, found in the same pass:
- `handle_completion` derives the failure attempt from the prior durable retry entry rather than the run's own `attempt`, so once firing is wired a re-dispatched run resets to attempt 1 and backoff never escalates; a clean exit also clobbers a pending backoff entry.
- `DaemonState.queued` (the STORY-008 AC3 operator surface) is never cleared when an item recovers and is never written on the stall path.

Found by the MILESTONE-003 end-of-chunk review.

