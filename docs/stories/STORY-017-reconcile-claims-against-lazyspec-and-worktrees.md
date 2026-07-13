---
title: Reconcile claims against lazyspec and worktrees
type: story
status: accepted
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
  - related-to: ADR-002
  - targets: MILESTONE-002
---

As an operator, I want restart reconcile to cross-check each claim against lazyspec state, so that claims for vanished or finished work do not linger while live work is preserved. (Split: the on-disk worktree half is STORY-065.)

- Given a claim for an iteration absent from lazyspec, When reconcile runs, Then the claim is dropped and recorded.
- Given a claim lazyspec reports terminal-complete, When reconcile runs, Then it is finalized, not re-dispatched.
- Given a claim lazyspec still reports active, When reconcile runs, Then the claim is retained.
- Given the lazyspec read fails, When reconcile runs, Then claims are retained and it retries next cycle.

