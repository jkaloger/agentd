---
title: Reconcile claims against on-disk worktrees
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-065
---

## Objective
Restart reconcile cross-checks each claim against worktrees on disk: valid lease + worktree present → retain untouched; worktree absent → flag+release per policy+record; worktree with no claim → report orphan for cleanup; listing fail → retain+retry.

## Context
- Implements: STORY-065 (ACs there). Split from STORY-017 (lazyspec half).
- Architecture: [[ADR-002]] — reconcile redb claims against real worktrees.
- Touch: `src/workspace.rs` (list worktrees on disk under workspace root), `src/tick.rs` `reconcile` / `ReconcileReport`.

## Satisfies
STORY-065 AC1–AC4.

## Tasks
1. `src/workspace.rs`: enumerate worktrees present on disk under workspace root (dir scan or `git worktree list --porcelain`), keyed by iter id.
2. `reconcile` cross-check:
   - claim + valid lease + worktree present → retain untouched (AC1).
   - claim + worktree absent → flag + release per policy + record (AC2).
   - worktree on disk with no matching claim → report as orphaned-worktree for cleanup (new `ReconcileReport` field) (AC3).
3. Listing failure → retain claims untouched + retry next cycle (AC4).
4. Tests per AC.

## Out of scope
- Actual worktree deletion/cleanup execution (report only; STORY-044 removes on terminal).
- lazyspec cross-check (STORY-017).

## Principles/conventions
- [[ADR-002]] / [[ADR-005]] workspace isolation; reconcile preserves live trees.
- TDD; extend reconcile tests with on-disk worktree fixtures.

## Verification
Listing failure is conservative: no claim mutated, no orphan reported when the scan errors.

