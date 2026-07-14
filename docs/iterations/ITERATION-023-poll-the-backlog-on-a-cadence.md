---
title: Poll the backlog on a cadence
type: iteration
status: review
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-002
---

## Objective
The orchestrator worker re-runs a tick every `poll_interval_ms` after the previous one finishes; ticks never overlap, and the interval is re-read each cycle so a reload governs the next wait.

## Context
- Implements: STORY-002 (ACs there).
- Architecture: [[ADR-006]] interval is hot-reloadable and applies to future dispatch only, in-flight untouched; [[ADR-007]] interval/caps re-applied on reload, never to in-flight runs.
- Touch: `src/daemon.rs` `spawn_orchestrator` — its worker task currently reconciles once then runs a single `run_tick` and exits (no loop); `src/config.rs` `poll_interval_ms`; `src/tick.rs` `run_tick`.

## Satisfies
STORY-002 AC1–AC3.

## Tasks
1. Wrap the worker's single-tick body (reconcile stays a one-time startup step) in a poll loop: after each `run_tick` and its post-commit projection seam complete, wait then tick again. One sequential task means ticks cannot overlap — the next wait starts only on completion (AC1, AC2).
2. Read `poll_interval_ms` fresh at the top of each wait from a shared, reloadable config source (e.g. seed a `watch::Receiver<Config>` in `spawn_orchestrator`) rather than the value captured once, so a new interval takes effect on the next wait (AC3). The reload producer is STORY-050 — seed the handle here only.
3. Make the wait cancellable: `tokio::select!` the `poll_interval_ms` sleep against the shutdown watch (reuse `wait_true` + `shutdown_tx`) so the loop exits promptly.
4. Tests in `src/daemon.rs` `mod tests` (reuse `FakeTracker`/`BlockingAdapter`, short `poll_interval_ms`, real sleeps — no `tokio` test-util): a second tick runs after the first completes (AC1); records accumulate sequentially with no concurrent turn (AC2); updating the shared config between cycles makes the next wait use the new interval (AC3).

## Out of scope
- The config-file reload/watch mechanism itself → STORY-050.
- Forcing an out-of-cadence poll → STORY-056.

## Principles/conventions
- [[ADR-006]]/[[ADR-007]]: a reloaded interval affects only the next wait; the in-flight tick runs to completion untouched.
- TDD; extend the `src/daemon.rs` fake-based orchestrator tests.

## Verification
No two ticks ever run concurrently, and a reloaded interval never interrupts an in-flight tick — it applies to the next wait only.
