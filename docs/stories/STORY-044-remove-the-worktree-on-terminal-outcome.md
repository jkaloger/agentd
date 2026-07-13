---
title: Remove the worktree on terminal outcome
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-005
- targets: MILESTONE-006
---

As an operator, I want worktrees for terminal iterations cleaned up, so that stale trees and branches do not accumulate.

- Given an iteration reaches terminal, When cleanup runs, Then before_remove fires (if configured) and the worktree is removed via `git worktree remove`.
- Given stale terminal worktrees at startup, When startup cleanup runs, Then they are removed.
- Given worktree removal fails, When it errors, Then it is logged and operation continues.
