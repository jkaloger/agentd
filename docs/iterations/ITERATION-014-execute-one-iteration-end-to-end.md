---
title: Execute one iteration end-to-end
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-060
---

## Objective
Prove the full path on one ticket: poll from lazyspec → durable claim → real agent in its own worktree → advance on outcome, with status/log showing the complete trace and restart reconciling an orphaned claim.

## Context
- Implements: STORY-060 (see its ACs for the end-to-end / trace / failure / restart behaviour).
- Architecture: [[ADR-001]] — integrates the walking-skeleton layers into the daemon orchestrator task. Composes the fetch (STORY-022), claim (STORY-013), claim-advance (STORY-025), worktree (STORY-039), agent run (STORY-029), and outcome resolution (STORY-026) iterations into one tick.
- Integration slice: wiring, not new subsystems.

## Tasks
1. Wire the orchestrator tick to run the real fetch → claim → worktree → agent run → outcome-advance path on one eligible iteration.
2. Surface the full trace in `status`/`log`: claimed, running, terminal, with token/runtime totals and the resulting lazyspec transition.
3. On non-clean exit, transition per the failure policy and release the claim while preserving the worktree for inspection.
4. On daemon kill mid-run, reconcile on restart to resolve the orphaned claim without double-dispatching.

## Acceptance Criteria
- STORY-060 AC1–AC4 hold: clean run flows end-to-end to terminal; status/log show the full trace with totals + transition; non-clean exit transitions per failure policy with claim released and worktree preserved; restart reconciles the orphan without double-dispatch.

## Out of scope
- Hardening of any individual layer; concurrency beyond the single proving ticket; retry backoff policy.

