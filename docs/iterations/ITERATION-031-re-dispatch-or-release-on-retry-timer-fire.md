---
title: Re-dispatch or release on retry-timer fire
type: iteration
status: review
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-009
---

## Objective
A fired retry re-fetches candidates before acting: item absent → release claim + clear retry; present+eligible+slot → dispatch carrying its attempt number; eligible but no slot → requeue with error "no available orchestrator slots"; re-fetch fails → requeue (not release).

## Context
- Implements: STORY-009 (ACs there). This iteration is the SOLE builder of the retry-timer-fire handler; ITERATION-029 (STORY-007) only schedules the clean-exit continuation entries this handler then consumes.
- Prereq: the retry store landed via ITERATION-019 (STORY-018, complete) — `RetryRecord { attempt, error, due_at }`, `schedule_retry`/`due_retries`/`clear_retry`/`release` in `src/store.rs`.
- Architecture: [[ADR-007]] scheduling policy — global cap (`Config::max_concurrent`); [[ADR-003]] lazyspec is the truth for what is dispatch-eligible; [[ADR-002]] the store is the truth for claims/retries.
- Touch: `src/tick.rs` — new retry-fire handler beside `run_tick`, re-fetching via `Tracker::fetch_dispatchable` and reusing the `claim_and_activate`/worktree/`assemble_prompt` dispatch path; `assemble_prompt(_, _, attempt, _)` already threads `attempt` (currently the hardcoded `FIRST_ATTEMPT`); `RetryRecord.attempt` supplies it on a retry. Slot gate reads `Config::max_concurrent` against the caller's in-flight count.

## Satisfies
STORY-009 AC1–AC4.

## Tasks
1. Add a retry-fire handler in `src/tick.rs` that, per due retry (`store.due_retries(now)`), re-fetches `tracker.fetch_dispatchable()` and branches on the result before any store mutation.
2. Re-fetch fails → leave the schedule intact (requeue), return, so the next cycle retries (AC4) — mirror `reconcile`'s conservative read-fails-first ordering.
3. Item absent from candidates → `store.release(id)` + `store.clear_retry(id)` (AC1): the work is stale/gone.
4. Present + eligible + a free slot (in-flight count `< config.max_concurrent`) → run the dispatch path carrying `RetryRecord.attempt` through `assemble_prompt` (not `FIRST_ATTEMPT`), clearing the retry once re-dispatched (AC2).
5. Present + eligible + no free slot → re-`schedule_retry` with error `"no available orchestrator slots"` (AC3), preserving the attempt.
6. Tests per AC using the existing `FakeTracker`/`FakeAdapter`/`Store` fakes in `tick.rs` `mod tests`: absent→released+cleared; present+slot→dispatched with the retry's attempt in the prompt; present+no-slot→requeued with the slot error; re-fetch error→schedule untouched.

## Out of scope
- Backoff-delay computation and scheduling failures onto the queue → STORY-008.
- Wiring the handler into the daemon poll loop and slot accounting there → daemon-loop iteration.

## Principles/conventions
- [[ADR-007]] global concurrency cap gates dispatch; [[ADR-003]] never re-dispatch an item lazyspec no longer offers.
- TDD; extend `src/tick.rs` tests with the fake tracker/adapter already there.

## Verification
Re-fetch failure must be conservative: the retry schedule is byte-for-byte unchanged and the claim untouched. A re-dispatched retry's prompt carries its own attempt number, not attempt 1.
