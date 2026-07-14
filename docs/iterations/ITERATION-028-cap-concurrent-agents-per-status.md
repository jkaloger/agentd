---
title: Cap concurrent agents per status
type: iteration
status: review
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-006
---

## Objective
The dispatch loop honours a per-status cap: a status at its configured cap is skipped even when global slots remain; a status with no cap falls back to the global limit; status keys match after lowercase normalization.

## Context
- Implements: STORY-006 (ACs there). Builds on STORY-005 (global cap + slot counting) — prerequisite.
- Substrate precondition: `run_tick` (`src/tick.rs`) awaits the agent turn inline — there is no dispatch loop or real concurrency yet. This iteration is the per-status *policy*; it depends on the poll loop (STORY-002/ITERATION-023) and the inline-await→spawn/track conversion that gives multiple in-flight agents to count against.
- Architecture: [[ADR-007]] scheduling policy — per-status cap M with M running in that status blocks that status; uncapped status defers to global; status key lookup normalized to lowercase.
- Touch: `src/config.rs` (`per_status_caps: BTreeMap<String,u32>`, `resolve_caps` — normalize keys to lowercase there), `src/tick.rs` dispatch loop (per-status running count vs cap, alongside STORY-005 global slot check), `src/store.rs` `Store::claims` (live claims to tally), `src/tracker.rs` (`Candidate::state`, set from `doc.status` in `normalize`; resolve each claimed id's state for grouping).

## Satisfies
STORY-006 AC1–AC3.

## Tasks
1. Lowercase-normalize per-status cap keys in `resolve_caps` (`src/config.rs`) so lookup by a candidate's status matches regardless of case (AC3).
2. In the `src/tick.rs` dispatch loop, tally running claims by status (each `Store::claims` id → its lazyspec status via the tracker) and, before dispatching a candidate, look up its `state` (lowercased) in `per_status_caps`: at/over cap → skip that candidate, leaving it for a future tick (AC1).
3. A candidate whose `state` has no configured cap dispatches under the STORY-005 global limit only (AC2).
4. Tests per AC using the existing fake tracker/store patterns in `src/tick.rs` `mod tests`: cap reached blocks with global slots free (AC1); uncapped status uses global limit (AC2); mixed-case key matches (AC3).

## Out of scope
- Global cap and slot counting → STORY-005.
- Priority ordering across statuses → [[ADR-007]], separate story.

## Principles/conventions
- [[ADR-007]] per-status cap semantics; per-status cap never raises the effective limit above the global cap.
- TDD; extend the dispatch-loop tests with fakes.

## Verification
With global slots free, a status at its cap dispatches zero of that status while an uncapped status still dispatches.
