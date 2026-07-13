---
title: Release orphaned claims on restart
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-016
- blocks: ITERATION-021
- blocks: ITERATION-022
---

## Objective
Startup reconcile durably releases claims whose lease expired with no live worker; retains valid leases; projects each release so operator sees it.

## Context
- Implements: STORY-016 (ACs there).
- Architecture: [[ADR-002]] — on restart, reconcile redb claims, release expired leases with no live worker.
- Touch: `src/tick.rs` `reconcile` / `ReconcileReport` (already releases expired + tracks `re_offered`); wire release projection from STORY-019 iter.

## Satisfies
STORY-016 AC1–AC3.

## Tasks
1. `reconcile`: claim with expired lease (`due_at <= now`) + no reattachable live worker → durably `release` + record in `ReconcileReport.released`. (No worker reattach in this milestone → expired ⇒ orphan.)
2. Valid lease (`due_at > now`) → retained (pending reattach/expiry).
3. Project each release via log projection (STORY-019) — N releases ⇒ N recorded lines operator can see.
4. Keep the `re_offered` no-double-dispatch invariant assertion.
5. Tests: expired orphan released + projected; valid lease retained; N releases each projected/recorded.

## Out of scope
- Cross-check vs lazyspec doc state → STORY-017.
- Cross-check vs on-disk worktrees → STORY-065.
- Retry schedules.

## Principles/conventions
- [[ADR-002]] resumability across daemon crash; no orphan blocks backlog.
- TDD; extend `src/tick.rs` reconcile tests.

## Verification
After reconcile releases an orphan, the log projection contains a release line for that id.

