---
title: Exponential backoff retry on failure
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-008
---

## Objective
On an abnormal worker exit, schedule a retry with `due_at = now + min(10000 * 2^(attempt-1), max_retry_backoff_ms)`, replacing any prior schedule for the item, and surface it in the retry queue with its attempt and error.

## Context
- Implements: STORY-008 (ACs there). Builds on STORY-007's clean-exit continuation path (running-entry removal + retry-scheduling seam).
- Architecture: [[ADR-002]] durable state — the retry schedule is re-derived from `due_at`, never held as an in-memory timer; [[ADR-007]] scheduling policy.
- Reuses (ITERATION-019 → STORY-018, complete): `src/store.rs` `Store::schedule_retry(id, attempt, error, due_at)` (durably replaces any prior entry for the id → this alone satisfies "cancel prior"), `RetryRecord { attempt, error, due_at }`.
- Touch: `src/config.rs` `resolve`/`require_positive` (add `max_retry_backoff_ms`, does not exist); `src/daemon.rs` completion routing — post-substrate ([[ITERATION-037]]) the abnormal-exit signal is the released-claim `RunRecord` (the failed arm, `clean == !release_claim` is false) in `handle_completion`, symmetric with STORY-007's clean-exit continuation added there; add the backoff computation on the failed arm. The attempt number is derived from the prior `RetryRecord` for the id (via the store), defaulting to 1 on the first failure; the error string is the run's failure detail. `src/daemon.rs` `DaemonState`/`ItemView` — retry-queue view exposing attempt + error for `status`.

## Satisfies
STORY-008 AC1–AC3.

## Tasks
1. Add `max_retry_backoff_ms` to `src/config.rs`: `RawConfig` field, `resolve` default, `require_positive` validation (mirror `poll_interval_ms`).
2. In the abnormal-exit branch of STORY-007's worker-exit handler, compute the delay `min(10000 * 2^(attempt-1), max_retry_backoff_ms)` (saturating shift) and call `Store::schedule_retry` with `due_at = now + delay`; the replace semantics mean a prior schedule for the id is superseded (AC1, AC2).
3. Extend `DaemonState` retry queue + `ItemView` so `status` renders the item with its attempt and error (AC3).
4. Tests per AC: backoff at attempts 1/2/N clamps to `max_retry_backoff_ms`; re-scheduling a pending item leaves exactly one entry; status shows the queued item's attempt + error. Use the existing `FakeAdapter`/`FakeTracker` seams in `tick.rs` `mod tests`.

## Out of scope
- Fired-timer re-dispatch vs release decision → STORY-009.
- Restart re-derivation of `due_at` → ITERATION-019 / STORY-018 (already complete).
- The clean-exit continuation path itself → STORY-007.

## Principles/conventions
- [[ADR-002]] retry schedule is durable; [[ADR-007]] backoff is the scheduling policy.
- TDD; extend `tick.rs` `mod tests` with the existing fakes.

## Verification
Backoff clamps: attempt large enough that `10000 * 2^(attempt-1)` overflows must yield exactly `max_retry_backoff_ms`, not panic. Re-scheduling leaves exactly one durable retry entry for the id.
