---
title: Dispatch only eligible items
type: iteration
status: review
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-003
---

## Objective
Dispatch a candidate only when it clears every eligibility rule: skip items missing required fields, keep only dispatch-role statuses, skip already-claimed items, and dispatch only into a slot the caller reports free.

## Context
- Implements: STORY-003 (ACs there).
- Architecture: [[ADR-003]] work source is lazyspec, states classified by role; [[ADR-007]] scheduling policy; [[ADR-002]] store is truth for claims.
- Precondition: eligibility filtering is the policy layer over the poll loop (STORY-002 / ITERATION-023). The daemon today awaits `run_tick` inline — one worker, no poll loop, no concurrency — so do NOT presume a concurrent-worker model.
- `Candidate.identifier` is DERIVED in `normalize` (via `identifier_from_path`); the raw required fields on `RawDoc` are `id`/`path`/`title`/`status`. AC1's "missing id/identifier/title/status" therefore maps to those raw fields (identifier follows from `path`).
- Touch: `src/tracker.rs` (`parse_and_filter`/`normalize`/`RawDoc` — a listing item missing a raw required field aborts the whole parse today because `RawDoc`'s fields are non-optional), `src/mapping.rs` `RoleMapping::classify` (`StateRole::Dispatch` already excludes active/terminal — AC2), `src/tick.rs` `run_tick` candidate selection (takes `.next()`, no claim gate), `src/store.rs` `claims`/`get` (live-claim source).

## Satisfies
STORY-003 AC1–AC4.

## Tasks
1. AC1: make `parse_and_filter` skip an item missing a required field (`id`/`path`/`title`/`status`) instead of failing the whole listing — deserialize items into a tolerant shape and drop any incomplete one in/around `normalize`. A non-JSON listing stays `TrackerError::Parse`.
2. AC2: retain the existing `classify(..) == Some(StateRole::Dispatch)` filter; add a test asserting an active status and a terminal status are both dropped.
3. AC3: in `run_tick` skip any candidate whose store claim is live (`due_at > now` via `store.get`/`claims`) so an already-running/claimed item is not dispatched again. AC4: treat "a slot is free" as an input the caller supplies — do NOT compute a global ceiling here.
4. Tests per AC on the existing fakes: incomplete-item skip in `tracker.rs` `mod tests`; already-claimed skip in `tick.rs` `mod tests` (`FakeTracker`/`FakeAdapter`, pre-seeded `Store`).

## Out of scope
- Global concurrency ceiling / slot arithmetic → STORY-005 (ITERATION-027).
- Per-status caps (`per_status_caps`) → later scheduling iteration.
- Blocker/dependency gating (`Candidate::dependencies`) — carried, not acted on.

## Principles/conventions
- [[ADR-002]] store = truth for claims; [[ADR-003]] role mapping; [[ADR-007]] global cap.
- TDD; extend the existing `mod tests` fakes in `tracker.rs` and `tick.rs`.

## Verification
- A listing mixing one incomplete item with well-formed dispatch items returns only the well-formed ones — one bad item must not abort the fetch (AC1).
- A candidate already holding a live store claim is not dispatched again (AC3).
