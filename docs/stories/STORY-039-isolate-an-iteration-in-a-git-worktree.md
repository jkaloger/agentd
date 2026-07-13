---
title: Isolate an iteration in a git worktree
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-005
- blocks: STORY-040
- blocks: STORY-041
- blocks: STORY-042
- blocks: STORY-043
- blocks: STORY-045
- blocks: STORY-029
- blocks: STORY-065
- blocks: STORY-060
- targets: MILESTONE-001
---

As an operator, I want each dispatched iteration to get its own git worktree on its own branch, so that agents work in isolation and each run feeds a clean PR. (Workspace walking skeleton.)

- Given a git repo and iteration <iter-id> dispatched, When the workspace is prepared, Then `git worktree add <root>/<iter-id> -b agentd/<iter-id>` creates an isolated tree sharing the object store.
- Given the worktree exists, When preparation finishes, Then show/status expose its path and branch.
- Given worktree creation fails, When it errors, Then the attempt fails with an operator-visible reason and no half-created tree is left claimable.
