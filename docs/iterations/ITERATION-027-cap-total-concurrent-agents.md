---
title: Cap total concurrent agents
type: iteration
status: superseded
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-005
---

> **Superseded by [[ITERATION-037]].** The concurrent-worker substrate ([[ADR-008]] / STORY-069) delivered this global cap directly: the daemon fill loop dispatches up to `max_concurrent` minus the live-worker count per tick, breaks at zero slots, and resumes when a worker frees a slot (`a_full_pool_defers_the_third_until_a_worker_frees_a_slot`). STORY-005's three ACs are met there. This iteration's original plan — counting live `store.claims()` — became incorrect once completed items retain their claim, since it would count retained (non-running) claims against the ceiling; the in-memory worker registry is the correct running-count source. Retained for provenance only.

## Objective
Gate dispatch on a hard global ceiling: with `max_concurrent=N` and N already running, available slots is 0 so nothing dispatches; a freed slot lets the next tick resume; the dispatch loop breaks the instant slots hit zero and leaves the rest for a future tick.

## Context
- Implements: STORY-005 (ACs there).
- Architecture: [[ADR-007]] — global concurrency cap is a config-driven ceiling protecting the host; caps are advisory (freeing a slot re-evaluates the full eligible set), never applied to in-flight runs.
- Precondition: today `run_tick` claims one candidate and AWAITS its agent turn inline (`src/tick.rs`), so there is no dispatch loop and no real concurrency — `max_concurrent` governs nothing until `run_tick` is converted from inline-await to a spawn/track worker model, driven by the poll loop (STORY-002 / ITERATION-023). This iteration is the slot-arithmetic policy over that substrate; it depends on that structural change.
- Touch: `src/config.rs` `max_concurrent` (owned here, already resolved/validated), `src/tick.rs` `run_tick` — add the available-slots gate over the running set (live `store.claims()`) before claiming.

## Satisfies
STORY-005 AC1–AC3.

## Tasks
1. Compute available slots in `run_tick`: `max_concurrent` minus the count of live (non-expired at `now`) claims from `store.claims()`; saturating so it never underflows.
2. Zero slots → dispatch none, report `Idle` without fetching/claiming (AC1). A freed slot on a later tick (fewer live claims) lets dispatch proceed again (AC2).
3. Dispatch loop fills slots from the candidate list in order, decrementing per successful claim, and breaks the moment slots reach zero — remaining eligible candidates are left for a future tick (AC3).
4. Tests per AC with the existing `FakeTracker`/`Store` fakes: N pre-existing live claims → nothing dispatched; releasing one → next tick dispatches; more candidates than slots → only up-to-slots claimed, rest untouched.

## Out of scope
- Priority ordering of the eligible set → STORY-004.
- Per-status caps → STORY-006.

## Principles/conventions
- [[ADR-007]] cap is advisory ceiling, not a reservation; governs future dispatch only.
- TDD; extend `tick.rs` `mod tests` with the existing fakes.

## Verification
The cap counts the running set, not offered candidates: with slots exhausted, zero claims are committed even when the tracker still offers eligible work.
