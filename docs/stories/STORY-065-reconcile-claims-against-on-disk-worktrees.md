---
title: Reconcile claims against on-disk worktrees
type: story
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
- targets: MILESTONE-002
---

As an operator, I want restart reconcile to cross-check each claim against the worktrees present on disk, so that claims whose working tree has vanished do not linger and live trees are preserved. (Split from STORY-017: the worktree half.)

- Given a claim with a valid lease and its worktree present, When reconcile runs, Then the claim is retained untouched.
- Given a claim whose worktree is absent from disk, When reconcile runs, Then the claim is flagged and released per policy and the action is recorded.
- Given a worktree on disk with no corresponding claim, When reconcile runs, Then the orphaned worktree is reported for cleanup.
- Given the worktree listing fails, When reconcile runs, Then claims are retained and reconcile retries next cycle.
