---
title: Isolate an iteration in a git worktree
type: iteration
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-039
---

## Objective
Prepare an isolated git worktree on its own branch per dispatched iteration (`git worktree add <root>/<iter-id> -b agentd/<iter-id>`), exposing its path/branch and failing cleanly with no half-created tree.

## Context
- Implements: STORY-039 (see its ACs for create/expose/failure behaviour).
- Architecture: [[ADR-005]] — git worktree per ticket sharing the object store; cwd is the worktree; path must stay under the workspace root; plain-dir mode is a deferred fallback.
- Workspace walking skeleton.

## Tasks
1. Add the worktree via `git worktree add <root>/<iter-id> -b agentd/<iter-id>` (honour lazyspec `branch_name` metadata when present).
2. Expose the worktree path and branch to `show`/`status`.
3. On creation failure, fail with an operator-visible reason and leave no half-created/claimable tree.

## Acceptance Criteria
- STORY-039 AC1–AC3 hold: worktree created on its branch sharing the object store; path/branch surfaced; failure is clean with no partial tree.

## Out of scope
- Plain-dir fallback (STORY-045); terminal cleanup/`worktree remove`; the agent run itself.

