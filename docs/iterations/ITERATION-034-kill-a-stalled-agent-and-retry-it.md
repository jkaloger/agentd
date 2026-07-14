---
title: Kill a stalled agent and retry it
type: iteration
status: review
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-011
---

## Objective
Reconcile terminates a running worker whose elapsed time since its last agent event (or `started_at` if none) exceeds `stall_timeout_ms` and queues a retry; a non-positive timeout skips the check, and stall detection runs before status refresh.

## Context
- Implements: STORY-011 (ACs there). Extends STORY-010's `reconcile_running` pass ([[ITERATION-033]]) with a stall check at its head.
- Architecture: [[ADR-001]] — the daemon is the orchestrator authority over worker liveness; [[ADR-006]] — new policy knobs live in `.agentd/config.toml`.
- Precondition/dependency: same substrate as [[ITERATION-033]] — today `spawn_orchestrator` (`src/daemon.rs`) awaits a single `run_tick` inline; this depends on the inline-await → spawn/track/abort worker conversion and the periodic reconcile loop (STORY-002 / [[ITERATION-023]]). The `reconcile_running` pass and the abortable stop handle are introduced by [[ITERATION-033]], not here.
- Running-worker representation: `src/daemon.rs` `RunningItem { id, identifier, started_at_ms }` (in `DaemonState.running`), extended by [[ITERATION-033]] with worktree path + stop handle. Agent events arrive batched in `TurnReport` at turn end, so `RunningItem` has no mid-flight progress signal — this slice adds one.
- Touch: `src/config.rs` (`Config`, `RawConfig`, `resolve` — add `stall_timeout_ms: i64`), `src/templates/config.toml` (document the key), `src/daemon.rs` `RunningItem` (add `last_event_at_ms: Option<u64>` and `attempt: u32`, plus the streaming seam that updates `last_event_at_ms` while a turn runs), `src/tick.rs` `reconcile_running` (stall pass at the head, before the status refresh), `src/store.rs` `schedule_retry` (queue the retry).

## Satisfies
STORY-011 AC1–AC3.

## Tasks
1. `src/config.rs`: add `stall_timeout_ms: i64` to `Config`/`RawConfig`, default it in `resolve`. Unlike `poll_interval_ms`, do NOT call `require_positive` — `<= 0` is the valid "disabled" sentinel (AC2).
2. `src/templates/config.toml`: document `stall_timeout_ms` with its default and the `<= 0` = disabled semantics.
3. `src/daemon.rs`: add `last_event_at_ms: Option<u64>` and `attempt: u32` (default `1`) to `RunningItem`, and update `last_event_at_ms` from the worker's event stream while its turn runs (the progress signal AC1's elapsed reads). Keep `running_view`/`on_activated` compiling.
4. `src/tick.rs` `reconcile_running`: at the head of the pass, before the status refresh, when `stall_timeout_ms > 0`, for each running item compute elapsed = `now` − `last_event_at_ms.unwrap_or(started_at_ms)`; if elapsed `> stall_timeout_ms`, terminate the worker (abort + `AgentAdapter::stop`) and `store.schedule_retry(id, item.attempt, "stalled", now)` — `due_at = now` for immediate re-check (AC1). Skip the whole pass when `stall_timeout_ms <= 0` (AC2). Ordering is load-bearing: the stall pass runs first (AC3).
5. Config test: `<= 0` parses and round-trips (no validation error). `reconcile_running` tests per AC using the existing `src/daemon.rs`/`src/tick.rs` fakes: stalled item (`last_event_at_ms` old) → terminated + retry queued at `now`; non-positive timeout → no termination; a fresh `last_event_at_ms` resets elapsed so a live worker survives.

## Out of scope
- STORY-010's terminate-on-terminal and status-refresh behaviour, and the `reconcile_running` pass itself — introduced by [[ITERATION-033]], extended not rebuilt here.
- The inline-await → spawn/track/abort worker conversion and poll loop — substrate from STORY-002 / [[ITERATION-023]] and [[ITERATION-033]].
- Retry backoff and attempt-cap policy (delay curve, timer cancellation) — STORY-008; here `due_at = now` and `attempt` carries through unchanged.

## Principles/conventions
- [[ADR-001]] daemon owns worker liveness; [[ADR-006]] config surface.
- TDD; extend `src/daemon.rs`/`src/tick.rs` `mod tests` with the existing fakes.

## Verification
When both the stall pass and status refresh would act on the same tick, a stalled-and-still-active worker is terminated by the stall pass first (a retry queued at `now`) — assert ordering, not just the end state.
