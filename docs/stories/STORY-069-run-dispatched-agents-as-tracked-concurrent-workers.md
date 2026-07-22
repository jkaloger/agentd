---
title: Run dispatched agents as tracked concurrent workers
type: story
status: complete
author: Jack Kaloger
date: 2026-07-15
tags: []
related:
- related-to: ADR-008
- targets: MILESTONE-003
- blocks: STORY-005
- blocks: STORY-006
- blocks: STORY-010
- blocks: STORY-011
---

## User Story

As the daemon, I want each dispatched iteration to run as its own tracked worker task rather than being awaited inline, so that multiple agents run concurrently up to a cap and the orchestrator can count and abort them.

## Acceptance Criteria

- Given free slots and several eligible candidates, When a tick runs, Then it dispatches more than one — each as its own worker — without waiting for an earlier worker's agent turn to finish.
- Given a worker has been dispatched, When the tick returns, Then the poll loop continues on cadence (it does not block on the agent turn) and the worker is recorded in a live in-memory registry keyed by work-item id.
- Given a worker finishes, whether the turn was clean or failed, Then its outcome is resolved into the same mapped lazyspec transition and claim release/retention as the previous inline path, and it is removed from the registry.
- Given the orchestrator is asked how many agents are running, Then it reports the count of live workers in the registry — the substrate the concurrency-cap and stop/stall stories build on.
- Given two ticks in a row, When the first fills every free slot, Then the second dispatches nothing until a worker completes and frees a slot.

## Notes

- Implements [[ADR-008]] (concurrent worker execution model); the cap policy that governs "free slots" is [[ADR-007]].
- Converts `run_tick` (src/tick.rs) from claim-and-await-inline into a dispatch step (claim + spawn a tracked worker) plus a completion step (resolve outcome); the daemon (src/daemon.rs) owns the registry and the fill-to-slots loop.
- Runtime registry is in-memory only; durable claims plus restart reconcile ([[ADR-002]]) remain the recovery mechanism.
- Shutdown drain policy is out of scope here (STORY-067). Global/per-status cap arithmetic is STORY-005/006; this story only exposes the live-worker count they consume.

