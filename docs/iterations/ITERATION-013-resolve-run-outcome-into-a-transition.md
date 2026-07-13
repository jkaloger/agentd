---
title: Resolve run outcome into a transition
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-026
---

## Objective
Resolve a finished run's outcome into the config-mapped lazyspec transition via `lazyspec advance` — success → terminal (or handoff), failure/timeout/stall → back or rejected — so the lifecycle tracks reality.

## Context
- Implements: STORY-026 (see its ACs for success/failure/handoff transitions).
- Architecture: [[ADR-003]] — daemon owns transitions via `lazyspec advance`, config-mapped: clean success ⇒ →complete or a configured handoff state (treated terminal-for-dispatch); failure ⇒ back or →rejected. Consumes the adapter's clean-exit vs failure signal ([[ADR-004]]).
- Uses the claim-advance seam (STORY-025 iteration) for the advance mechanism.

## Tasks
1. Map the adapter outcome to the config success/failure transition and run it via `lazyspec advance`.
2. On clean success, advance to the mapped terminal (default in-progress→complete) or the configured handoff state.
3. On failure/timeout/stall, run the mapped failure transition.
4. Treat a configured handoff state as terminal-for-dispatch so it is not re-offered.

## Acceptance Criteria
- STORY-026 AC1–AC3 hold: success runs the mapped success transition; failure/timeout/stall runs the failure transition; a handoff state is reached on success and treated terminal-for-dispatch.

## Out of scope
- Gate-rejection recovery (STORY-027, MILESTONE-003); claim release/reconcile details.

