---
title: Gate dispatch until dependencies and parent terminal
type: iteration
status: review
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-061
---

## Objective
Hold a dispatch-eligible candidate until every `blocked-by` dependency and its parent work item are terminal-complete in lazyspec; skip it with a recorded reason otherwise, treating an unresolvable ref as blocked.

## Context
- Implements: STORY-061 (ACs there).
- Architecture: [[ADR-003]] blocker rule (SPEC §8.2) — a fresh dispatch-eligible ticket waits until its lazyspec dependencies and parent are terminal-complete; lazyspec is the truth for what work exists.
- Touch: `src/tick.rs` `run_tick` candidate selection (currently `candidates.into_iter().next()` — dispatches blindly); `TickReport` (add a way to surface a blocked candidate + reason). `src/tracker.rs` `Candidate.parent`, `Candidate.dependencies` (`DependencyRef { target, kind: DependencyKind }`), `Tracker::lookup_doc` → `DocLookup::{Present(DocView), Absent}`. `src/mapping.rs` `RoleMapping::classify(doc_type, state)` → `Option<StateRole>`; gate is met only when `Some(StateRole::Terminal)`.

## Satisfies
STORY-061 AC1–AC4.

## Tasks
1. Add a gate check in `src/tick.rs` — e.g. `dispatch_gate(tracker, mapping, candidate) -> GateStatus` (`Eligible` | `Blocked { reason }`). Prerequisites are the parent (`candidate.parent`) plus each dependency with `kind == DependencyKind::BlockedBy`. For each: `tracker.lookup_doc`; `DocLookup::Absent` → `Blocked` naming the unresolved ref (AC4); `Present` whose `classify` is not `Some(StateRole::Terminal)` → `Blocked` (AC1/AC2); all terminal → `Eligible` (AC3). A `lookup_doc` `Err` propagates as the existing `TickReport::Error` (conservative — never silently dispatch).
2. In `run_tick`, evaluate candidates in order: skip `Blocked` ones recording their reason, dispatch the first `Eligible`. When none is eligible, surface the blocked reasons (extend `TickReport`, e.g. a `Blocked` variant carrying id + reason) so the daemon's projection records them; keep `Idle` for an empty candidate set. Derive the mapping via `RoleMapping::from_config(config)` or thread a `&RoleMapping` as `reconcile` does.
3. Tests per AC in `tick.rs` `mod tests` using `FakeTracker`'s `lookups` map to set each dep/parent status: not-terminal dep → blocked+reason (AC1); not-terminal parent → blocked (AC2); all terminal → dispatched (AC3); `DocLookup::Absent` dep → blocked with the ref surfaced (AC4).

## Out of scope
- The reverse `DependencyKind::Blocks` edge — this iteration gates only on `BlockedBy` prerequisites and the parent.
- Per-status caps / priority ordering ([[ADR-007]]); reconcile-path gating.

## Principles/conventions
- [[ADR-003]] daemon owns dispatch; lazyspec is truth for existence and status.
- TDD; extend `run_tick`/reconcile tests with the existing `FakeTracker` (`lookups`) pattern.

## Verification
A candidate with a mix of terminal and non-terminal prerequisites is never dispatched; the recorded reason names the specific blocking ref (unresolved refs included), and a `lookup_doc` failure yields `TickReport::Error`, not a dispatch.
