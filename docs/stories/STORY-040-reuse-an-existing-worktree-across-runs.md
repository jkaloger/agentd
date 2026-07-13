---
title: Reuse an existing worktree across runs
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-005
- targets: MILESTONE-006
---

As an operator, I want a retry/continuation of the same iteration to reuse its worktree, so that prior work persists across attempts.

- Given a worktree for <iter-id> exists, When re-dispatched, Then it is reused (not recreated) with created_now=false.
- Given reuse, When prepared, Then after_create does not run again.
- Given a reused worktree, When preparation fails, Then it is not destructively reset unless explicitly configured.
