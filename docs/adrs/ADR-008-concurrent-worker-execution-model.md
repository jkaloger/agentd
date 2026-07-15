---
title: Concurrent worker execution model
type: adr
status: accepted
author: Jack Kaloger
date: 2026-07-15
tags: []
related:
- related-to: ADR-001
- related-to: ADR-007
- related-to: ADR-002
---

## Context

[[ADR-001]] fixes the orchestrator as the single authority that supervises per-ticket workers, and [[ADR-007]] sets the scheduling policy — priority ordering plus a global and per-status concurrency cap — that authority applies when work outstrips capacity. Both presume *multiple agents in flight at once*: a cap that bounds concurrency, a supervisor that can count running workers and abort a specific one.

The daemon does not have that substrate. Today `run_tick` (src/tick.rs) claims one candidate and **awaits its agent turn inline** before returning; the poll loop (STORY-002 / ITERATION-023) calls `run_tick` sequentially, one item per tick, sleeping between ticks. Only ever one agent runs. `max_concurrent` governs nothing, there is no set of live workers to count for a cap, and nothing to abort when an item goes terminal or stalls.

So the caps (STORY-005/006), stop-on-terminal (STORY-010), and stall-kill (STORY-011) each name a concurrent-worker model as a precondition, but no story owns the conversion. It has been an implicit architectural gap rather than a decision on the record.

## Decision

The daemon runs a **bounded pool of tracked, concurrent per-ticket workers**, owned by the orchestrator.

- **Dispatch spawns; it does not await.** A tick claims an eligible candidate, spawns a worker task that runs the agent turn to completion, and returns immediately. The turn no longer blocks the tick or the poll loop.
- **Workers are tracked in a registry** keyed by work-item id, holding at least the abort handle, the active-state, and the start time. This registry is the single source of truth for *how many agents are running* and *which handle to abort*.
- **A tick fills free slots.** Free slots = the applicable cap minus the count of live workers in the registry. A tick dispatches up to that many candidates in priority order, then returns; the rest wait for a later tick.
- **Completion resolves the outcome.** When a worker finishes, its outcome is resolved into the mapped lazyspec transition and its claim released or retained exactly as the inline path did today, then it is removed from the registry.
- The pool bound is advisory over future dispatch only, never applied to in-flight runs ([[ADR-007]]).

`run_tick` therefore splits into a **dispatch step** (fetch/gate/claim/spawn) and a **completion step** (resolve outcome), connected by the registry.

## Consequences

- The concurrency substrate becomes a recorded, testable contract. STORY-069 implements it; STORY-005/006 (caps), STORY-010 (stop-on-terminal), STORY-011 (stall-kill) build on the registry it exposes and trace here as well as to [[ADR-007]].
- Ticks stop blocking on agent turns, so the poll cadence is honoured under load.
- Crash recovery is unchanged: durable claims plus restart reconcile ([[ADR-002]]) still cover orphaned work; the registry is in-memory runtime state, not truth.
- Shutdown must account for in-flight workers (drain or abort); the detailed drain policy is left to STORY-067 and is out of scope here.
- Rejected alternative: keeping the single inline worker and faking caps as sequential arithmetic — it makes the caps green in tests but hollow in production (only one agent ever runs), and gives stop/stall nothing to abort.

