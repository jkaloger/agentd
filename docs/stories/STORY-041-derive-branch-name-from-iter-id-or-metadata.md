---
title: Derive branch name from iter-id or metadata
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-005
- targets: MILESTONE-006
---

As an operator, I want the worktree branch to follow a predictable scheme, preferring lazyspec branch metadata, so that branches map cleanly to tickets and PRs.

- Given no branch metadata, When created, Then the branch is agentd/<iter-id>.
- Given a lazyspec branch_name, When created, Then that name is used instead.
- Given unsafe characters, When the branch is created, Then it is sanitized/rejected consistently with workspace-key rules.
