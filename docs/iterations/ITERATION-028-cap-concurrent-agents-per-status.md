---
title: Cap concurrent agents per status
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-006
---

## Objective
The dispatch loop honours a per-status cap: a status at its configured cap is skipped even when global slots remain; a status with no cap falls back to the global limit; status keys match after lowercase normalization.

## Context
- Implements: STORY-006 (ACs there). Builds on the global cap, now delivered by the concurrent-worker substrate ([[ADR-008]] / [[ITERATION-037]]).
- Substrate (now landed): the daemon fill loop (`src/daemon.rs` `spawn_orchestrator`) dispatches up to `max_concurrent` minus the live-worker count per tick, spawning a tracked worker per claim; `DaemonState.running` (`RunningItem`) is the live-worker registry. The per-status cap is a second gate over that same fill loop. There is no longer an inline-await dispatch loop in `run_tick`.
- Architecture: [[ADR-007]] scheduling policy — per-status cap M with M running in that active state blocks that status; uncapped status defers to global; status key lookup normalized to lowercase. The per-status cap never raises the effective limit above the global cap.
- The status to cap on is the **active state a running worker occupies** — the state `claim_and_activate` advanced the item to (`config.transitions.claim`), matching the `per_status_caps` keys (e.g. `in-progress`). Record that active state on each `RunningItem` at dispatch so the tally reads from the registry rather than re-querying the tracker per tick.
- Touch: `src/config.rs` (`per_status_caps: BTreeMap<String,u32>` already parsed; `resolve_caps` — normalize keys to lowercase there), `src/daemon.rs` fill loop (extend `RunningItem` with its active `state`; before dispatching a candidate, tally live workers in that state and skip when at/over its cap, alongside the global slot check).

## Satisfies
STORY-006 AC1–AC3.

## Tasks
1. Lowercase-normalize per-status cap keys in `resolve_caps` (`src/config.rs`) so lookup matches regardless of case (AC3).
2. Extend `RunningItem` (`src/daemon.rs`) with the worker's active `state` (lowercased), set at dispatch from the active state the item was advanced to. In the fill loop, before dispatching a candidate, tally live workers already in that candidate's prospective active state; if that count is at/over the state's configured cap, skip the candidate this tick even when global slots remain (AC1). The just-dispatched worker increments the tally so a within-tick burst also respects the cap.
3. A candidate whose active state has no configured cap dispatches under the global limit only (AC2).
4. Tests per AC in `src/daemon.rs` `mod tests` with the existing fakes (blocking `FakeAdapter`): a status at its cap dispatches zero of that status while global slots remain (AC1); an uncapped status uses the global limit (AC2); a mixed-case config key matches a running worker's state (AC3).

## Out of scope
- Global cap and slot counting → STORY-005.
- Priority ordering across statuses → [[ADR-007]], separate story.

## Principles/conventions
- [[ADR-007]] per-status cap semantics; per-status cap never raises the effective limit above the global cap.
- TDD; extend the dispatch-loop tests with fakes.

## Verification
With global slots free, a status at its cap dispatches zero of that status while an uncapped status still dispatches.
