---
title: Run workspace lifecycle hooks
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-005
---

As an operator, I want configurable shell hooks at defined lifecycle points, so that I can bootstrap dependencies and clean up without baking project specifics into agentd. (Flagged: split fatal vs best-effort hooks.)

- Given after_create configured, When a worktree is newly created, Then it runs with cwd=worktree; failure/timeout aborts creation.
- Given before_run configured, When an attempt launches, Then it runs before the agent; failure/timeout fails that attempt.
- Given after_run configured, When an attempt ends, Then it runs and failure/timeout is logged but ignored.
- Given before_remove configured, When a worktree is removed, Then it runs and failure/timeout is logged but cleanup proceeds.
- Given a hook exceeds hooks.timeout_ms, When it runs, Then it is terminated and logged.
