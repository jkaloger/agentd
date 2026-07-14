---
title: Keep reconciling when dispatch config is invalid
type: iteration
status: review
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-012
---

## Objective
Each tick, safety reconcile runs first with the last-known-good config; a per-tick config preflight then gates dispatch — an invalid config skips dispatch with an operator-visible error, a later valid config resumes it, and a candidate-fetch failure skips dispatch too, all without freezing reconciliation.

## Context
- Implements: STORY-012 (ACs there).
- Architecture: [[ADR-001]] — the daemon is the orchestrator authority over the tick sequence; [[ADR-002]] — reconcile is durable-state safety, independent of dispatch eligibility, so it must never be gated by a dispatch-config fault.
- Touch: `src/daemon.rs` `spawn_orchestrator` worker (reconcile runs at the existing seam ~L251 before `run_tick` ~L281; add the preflight gate between them; note `TickReport::Error` is currently dropped by the `Idle | Error(_) => {}` arm), `src/config.rs` (`config::load` — the preflight re-reads and re-validates `config_path`, returning `ConfigError` whose `Display` is the operator-visible text; `config::load_str` for tests), `src/tick.rs` `run_tick` (candidate fetch via `Tracker::fetch_dispatchable` already yields `TickReport::Error` on failure), `src/projection.rs` `EventKind` (the operator-visible skip surface).

## Satisfies
STORY-012 AC1–AC3.

## Tasks
1. Per-tick preflight in `spawn_orchestrator`'s worker: after reconcile, re-read and re-validate config via `config::load(config_path)`. `Ok(fresh)` → dispatch with the fresh config; `Err(ConfigError)` → skip `run_tick` this tick. Keep the passed-in `config` as the last-known-good used by reconcile so safety reconciliation never freezes (AC1).
2. Preserve ordering + surface the skip: reconcile (existing seam) runs first and unconditionally, before the preflight/dispatch. On a preflight `Err`, route the `ConfigError` message to the operator-visible surface (a `src/projection.rs` `EventKind` line + daemon state), not the silently-dropped `Error` arm, and leave the worker healthy for the next poll (`poll_interval_ms`) (AC1). A later tick whose config validates dispatches normally (AC2).
3. Candidate-fetch failure: `run_tick` already returns `TickReport::Error` when `Tracker::fetch_dispatchable` errs, and reconcile ran before it. Route that `Error` to the same operator-visible surface instead of dropping it, confirm nothing was dispatched, and leave the worker healthy to reschedule (AC3).
4. Tests per AC via the socket-free `spawn_orchestrator` fakes in `src/daemon.rs` `mod tests` (`fake_tracker`, `blocking_adapter`, `init_project`, `write_config`): (a) an expired-orphan/live claim is reconciled while an invalid config skips dispatch with a visible error and zero claim/advance; (b) a valid config dispatches (contrast case for resume, AC2); (c) a `fetch_dispatchable` error still reconciled first, surfaced the error, and dispatched nothing.

## Out of scope
- Owning the repeating poll loop / `poll_interval_ms` scheduling itself — this slice adds the per-tick ordering and gate; reschedule cadence is the loop's.
- Startup validation in `start` (`config::load` + `RoleMapping::validate`) — unchanged.
- Config hot-reload semantics beyond validate-or-skip (no partial merge, no diffing).

## Principles/conventions
- [[ADR-001]] daemon owns the tick sequence; [[ADR-002]] reconcile = durable safety, independent of dispatch.
- TDD; extend the socket-free `spawn_orchestrator` fake-based tests in `src/daemon.rs` `mod tests`.

## Verification
The invariant is ordering, not end state: on an invalid-config tick assert both that reconcile's store mutation happened AND that zero claim/advance occurred — a bad config skips only dispatch, never reconciliation.
