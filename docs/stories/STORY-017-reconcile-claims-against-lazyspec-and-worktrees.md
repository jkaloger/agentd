---
title: Reconcile claims against lazyspec and worktrees
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
---

As an operator, I want restart reconcile to cross-check each claim against lazyspec and on-disk worktrees, so that claims for vanished/finished work do not linger while live work is preserved. (Flagged large: may split into lazyspec vs worktree reconcile.)

- Given a claim for an iteration absent from lazyspec, When reconcile runs, Then the claim is dropped and recorded.
- Given a claim lazyspec reports complete, When reconcile runs, Then it is finalized, not re-dispatched.
- Given a valid lease but no worktree on disk, When reconcile runs, Then the claim is flagged/released per policy.
- Given the lazyspec read fails, When reconcile runs, Then claims are retained and it retries next cycle.
