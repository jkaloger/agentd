---
title: Convert run_tick to a tracked concurrent worker pool
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-15
tags: []
related:
- implements: STORY-069
- supersedes: ITERATION-027
---

## Objective
Convert the orchestrator from one inline-awaited worker into a bounded pool of tracked concurrent workers: a tick claims eligible candidates and spawns a worker per claim without awaiting the agent turn, the daemon tracks live workers in a registry keyed by item id, and each worker's completion resolves its outcome exactly as the inline path does today. This is the substrate the concurrency caps and the stop/stall stories build on — it does not itself add the cap edge-cases (ITER-027/028) or abort behaviour (ITER-033/034).

## Context
- Implements: STORY-069 (ACs there). Architecture: [[ADR-008]] concurrent worker execution model; [[ADR-007]] the cap is advisory over future dispatch; [[ADR-002]] durable claims + reconcile remain recovery.
- Today (`src/tick.rs` `run_tick`) claims one candidate, `.await`s the agent turn, resolves the outcome, and returns a `TickReport::Dispatched`; the daemon poll loop (`src/daemon.rs` `spawn_orchestrator`) calls it sequentially and mirrors the report to projection/snapshot, maintaining `DaemonState.running` via the `on_activated` seam. So only one agent ever runs and `running` holds at most one item.
- The turn-running + outcome-resolution half of `run_tick` (prepare worktree → run agent → resolve transition → release/retain claim, `src/tick.rs` after `claim_and_activate`) must move into a spawned worker task; the claim/gate/select half stays synchronous in the tick.
- Completion must not race the store/projection: the worker reports its resolved outcome back to the orchestrator loop over an `mpsc` channel; the loop applies the projection/snapshot updates (the code already in the `TickReport::Dispatched` arm) and removes the worker from the registry. Keep `Store`/`Projection`/`Snapshot` single-owner in the loop.
- Touch: `src/tick.rs` (split `run_tick` into a dispatch step returning a claim + a spawnable turn future, and a completion step resolving the outcome — reuse the existing outcome/resolve helpers, do not duplicate them); `src/daemon.rs` `spawn_orchestrator` (registry of live workers id→abort handle+started_at reusing `DaemonState.running`; a fill-to-slots dispatch pass per tick bounded by `max_concurrent` minus live workers; `tokio::select!` the poll-interval wait against a worker-completion channel and the shutdown watch so completions are handled promptly and the loop never blocks on a turn).

## Satisfies
STORY-069 AC1–AC5.

## Tasks
1. Split `run_tick`: a `dispatch_one` that does fetch/order/live-claim/gate/select + `claim_and_activate` + `on_activated` and returns enough to spawn the turn (the `Candidate`, claim, worktree inputs) or the existing non-dispatch reports (`Idle`/`Blocked`/`Skipped`/`Error`); and a `run_worker` (async) that prepares the worktree, runs the agent turn, and resolves the outcome into the same `DispatchRecord` the inline path produces. Preserve every existing outcome branch (clean retains claim, failure releases + leaves worktree, pre-agent failure path).
2. In `spawn_orchestrator`: per tick, compute `slots = max_concurrent.saturating_sub(live_worker_count)`; call `dispatch_one` up to `slots` times, and for each dispatch `tokio::spawn` `run_worker`, recording the worker in the registry (id → JoinHandle/AbortHandle + started_at) and pushing to `DaemonState.running`. Do NOT await the turn.
3. Route completion: each worker sends its `DispatchRecord`/outcome to the loop via `mpsc`; the loop consumes completions (via `select!` alongside the poll timer + shutdown), applies the existing projection/snapshot mirroring, and removes the worker from the registry + `running`.
4. Expose the live-worker count from the registry (the number caps will consume) — a method/field on the orchestrator state.
5. On shutdown, stop dispatching new workers; awaiting/aborting in-flight workers cleanly is best-effort here (full drain is STORY-067) — at minimum do not leave the process hanging on live handles.
6. Tests (async, existing `FakeTracker`/`FakeAdapter`/`Store` fakes; a slow/blocking `FakeAdapter` turn to observe concurrency): several eligible + slots free → more than one worker spawned before any completes (AC1); tick returns without blocking on the turn and the worker is in the registry (AC2); a worker finishing resolves the same transition/claim outcome and leaves the registry (AC3); the live-worker count reflects the registry (AC4); slots full → a following tick dispatches nothing until one completes (AC5).

## Out of scope
- Global-cap edge semantics — zero-slot reports `Idle` without fetching, counting the durable `store.claims()` set, freed-slot resume tests → STORY-005 (ITER-027).
- Per-status caps → STORY-006 (ITER-028). Aborting a tracked worker on terminal/stall → STORY-010/011 (ITER-033/034). Full shutdown drain → STORY-067.

## Principles/conventions
- [[ADR-008]] dispatch spawns and does not await; the registry is in-memory runtime truth for what is running; durable claims remain recovery truth.
- TDD; extend the `daemon.rs`/`tick.rs` `mod tests` with the existing fakes. A `FakeAdapter` whose turn blocks on a signal is the seam for observing concurrency without wall-clock races.

## Verification
With `max_concurrent = 2` and three eligible candidates whose turns block, exactly two workers are spawned and tracked before any completes and the third is not claimed; releasing one blocked turn lets the next tick claim the third; each completion applies the same lazyspec transition and claim release/retain as the pre-conversion inline tick.

