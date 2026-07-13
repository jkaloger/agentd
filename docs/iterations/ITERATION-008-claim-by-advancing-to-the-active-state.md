---
title: Claim by advancing to the active state
type: iteration
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-025
---

## Objective
At claim time the daemon advances the item from dispatch to active via `lazyspec advance` (default accepted→in-progress) before the agent starts; on advance failure the agent does not run and the claim is released.

## Context
- Implements: STORY-025 (see its ACs for advance-before-run / not-re-offered / release-on-failure behaviour).
- Architecture: [[ADR-003]] — the daemon owns all lifecycle transitions via `lazyspec advance`, config-mapped; claim ⇒ accepted→in-progress. Uses the durable claim (STORY-013 iteration) and role mapping (STORY-024 iteration).
- Write walking skeleton: daemon-owned transition.

## Tasks
1. On selecting an item for dispatch, run the config-mapped claim transition via `lazyspec advance` before starting the agent.
2. On advance success, ensure the next poll sees the item as active and does not re-offer it as fresh.
3. On advance failure, skip the agent and release the claim for retry.

## Acceptance Criteria
- STORY-025 AC1–AC3 hold: claim transition runs before the agent; a claimed item is active and not re-offered; advance failure means no run and a released claim.

## Out of scope
- Success/failure resolution after the run (STORY-026 iteration); gate-rejection recovery.

