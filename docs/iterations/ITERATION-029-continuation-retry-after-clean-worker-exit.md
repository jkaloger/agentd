---
title: Continuation retry after clean worker exit
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-007
---

## Objective
On a clean worker exit, schedule a continuation instead of finalizing: remove the running entry, update totals, and queue a continuation retry (attempt 1, ~1000ms) holding the claim.

## Context
- Implements: STORY-007 (ACs there).
- Architecture: [[ADR-004]] — the claude adapter runs one-shot turns; continuation is re-dispatch, and only "if the doc is still active". [[ADR-002]] — retry schedules are durable, eligibility re-derived from `due_at` (no serialized timer).
- The retry store — `RetryRecord`, `schedule_retry`, `due_retries`, `clear_retry`, `release` — landed in `src/store.rs` via ITERATION-019 (STORY-018, complete). This slice is the first writer of a continuation schedule; the fire handler that reads it is ITERATION-031 (STORY-009).
- Touch: `src/store.rs` `schedule_retry` + `RetryRecord`; `src/daemon.rs` completion routing — post-substrate ([[ITERATION-037]]) the clean-exit signal is the `WorkerCompletion`/`RunRecord` (`clean`/`claim_released`) handled in `handle_completion`, where the running-entry removal (`state.running.retain`) and terminal-record push (totals) already live; add the `schedule_retry` there on a clean exit. (`run_tick` is now a `#[cfg(test)]` composition; the production path is `dispatch_one`/`run_worker`/`finalize`/`handle_completion`.)

## Satisfies
STORY-007 AC1 directly. AC2/AC3 (the fire outcomes — re-dispatch a fresh session when still active with a slot, release otherwise) are realized by ITERATION-031's retry-timer-fire handler consulting the schedule this slice writes.

## Tasks
1. Add a `CONTINUATION_RETRY_MS` const (~1000).
2. On a clean worker exit, alongside the existing running-entry removal + terminal-record push (totals), schedule a continuation retry: `store.schedule_retry(id, 1, "", now + CONTINUATION_RETRY_MS)`; the claim stays held (`claim_released` remains false) (AC1).
3. Tests per AC1 with the existing `FakeTracker`/`FakeAdapter`/`Store` fakes: a clean exit clears the running entry, records the run (totals), and leaves a durable attempt-1 retry due ~1000ms out with the claim still held.

## Out of scope
- The retry-timer-fire handler itself (reads `due_retries`, re-checks candidates, dispatches/releases/requeues) → ITERATION-031 (STORY-009). AC2/AC3's observable outcomes land there.
- Exponential-backoff delay formula, `max_retry_backoff_ms` config, and the status retry-queue view → STORY-008. This slice uses attempt 1 and a fixed ~1000ms continuation delay only.
- Stopping/killing a running agent on terminal or stall → STORY-010/STORY-011.

## Principles/conventions
- [[ADR-004]] continuation = re-dispatch only while the doc is still active; a clean turn does not itself finalize.
- [[ADR-002]] the retry schedule is durable; the fired-timer decision (ITERATION-031) re-derives from `due_at`, never a live timer handle.
- TDD; extend `daemon.rs`/`tick.rs` `mod tests` with the existing fakes.

## Verification
Continuation is not a finalize: after a clean exit the claim is still held and a durable attempt-1 retry is scheduled ~1000ms out; nothing in this slice releases or re-dispatches — that is ITERATION-031's job.
