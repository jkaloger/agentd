---
title: Plain-directory fallback for non-git projects
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-005
---

As a non-git maintainer, I want a plain per-iteration directory mode when my project is not a git repo, so that I can still run agentd with isolation.

- Given workspace.mode=plain (or git detection fails with fallback enabled), When prepared, Then a plain dir <root>/<iter-id> is created/reused instead of a worktree.
- Given plain mode, When prepared, Then safety invariants and hooks apply identically.
- Given git mode configured but no git repo, When the daemon starts, Then it fails validation with a clear message.
