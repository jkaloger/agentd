---
title: Surface and recover from gate-rejected transitions
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-027
---

## Objective
Distinguish a lazyspec gate rejection from a transport error at the advance seam: a gate-rejected claim releases cleanly (agent never runs), is logged with id + transition + reason, and the candidate is suppressed so continued polls don't busy-loop.

## Context
- Implements: STORY-027 (ACs there).
- Architecture: [[ADR-003]] — the daemon owns every lazyspec transition via `advance` and must "respect lazyspec gates and surface gate rejections."
- Touch:
  - `src/tracker.rs` `Tracker::advance` maps `CliFailure::Exit` to `TrackerError::Command` today, conflating a lifecycle-gate refusal with a transport fault; needs a gate classifier (mirror `is_not_found`) and a distinct error variant.
  - `src/dispatch.rs` `claim_and_activate` already releases the claim and skips `start_agent` on any advance error (AC2, no half-claim); the gate variant flows through `DispatchError::Advance` unchanged.
  - `src/tick.rs` `run_tick` picks `candidates.into_iter().next()` with no skip set; `TickReport::Skipped { reason, claim }` + `SkipClaim::ClaimedThenReleased` already model the durable claim-then-release.
  - `src/daemon.rs` `spawn_orchestrator` worker owns the post-commit projection seam; it has no cross-poll suppression state.
  - `src/projection.rs` `EventKind` is the log vocabulary (`Claim`/`Release`/…); no gate event yet.

## Satisfies
STORY-027 AC1–AC3.

## Tasks
1. Classify at the seam (AC1): in `src/tracker.rs` `advance`, route a non-zero `CliFailure::Exit` whose stderr signals a gate refusal to a new `TrackerError::Gate { target, reason }` via an `is_gate_rejection(stderr)` helper (mirroring `is_not_found`); leave spawn, other non-zero exits, and parse faults as the existing transport `TrackerError` variants.
2. Keep the claim consistent (AC2): confirm `claim_and_activate` releases the store claim and never invokes `start_agent` when `advance` returns the gate variant (the existing advance-failure path); expose a way for the caller to recognise it (e.g. match `DispatchError::Advance(TrackerError::Gate { .. })`).
3. Surface + log (AC1): add `EventKind::GateRejected` in `src/projection.rs`; carry the gate's `transition` (the claim target) and `reason` on `TickReport::Skipped` so `spawn_orchestrator` records one line with id, transition, and reason.
4. Bound repeated attempts (AC3): give the `spawn_orchestrator` worker an in-memory suppressed-id set spanning poll cycles; add a suppression predicate/param to `run_tick` so it selects the first non-suppressed candidate; on a gate-rejected skip the daemon inserts the id, so later ticks neither re-claim nor re-log it. A transport error must NOT suppress — it stays retryable.
5. Tests per AC using the existing fakes: the tracker classification split (AC1, like `lookup_doc_maps_a_not_found_exit_to_absent`); a gate-variant advance yields `DispatchError::Advance`, no agent start, no residual claim (AC2, extend `advance_failure_skips_the_agent_and_releases_the_claim`); across two ticks a gate-rejected candidate is claimed/logged at most once (AC3, reuse `src/tick.rs` `FakeTracker` `fail_advance` with a gate stderr, or the `src/daemon.rs` orchestrator fakes).

## Out of scope
- Durable retry/backoff scheduling of gate-rejected items (`store.rs` `RETRIES`, `schedule_retry`/`due_retries`) → STORY-008/009 ([[ITERATION-030]]/[[ITERATION-031]]); suppression here is in-memory, per daemon lifetime (a restart re-checks lazyspec).
- Post-run terminal-advance gate rejections: already surfaced via `RunRecord.transition_error`, and never busy-loop (a mid-run item sits in an active, non-dispatch state, so the role filter never re-offers it).
- The poll loop itself → STORY-002 ([[ITERATION-023]]); this slice only supplies the suppression the loop consults.

## Principles/conventions
- [[ADR-003]]: the daemon respects and surfaces lazyspec gates; it never bypasses them.
- [[ADR-002]]: durable state is the store — gate suppression is deliberately in-memory, so restart re-evaluates against lazyspec (see Out of scope).
- TDD; extend the existing fake-based `mod tests` in each touched file.

## Verification
A gate rejection never leaves a half-claim (store holds no claim for the id after the skip), and a candidate re-offered every poll is claimed and logged at most once until its lazyspec state changes — while a transport error stays retryable, not suppressed.
