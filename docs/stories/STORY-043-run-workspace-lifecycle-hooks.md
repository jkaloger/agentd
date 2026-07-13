---
title: Run workspace lifecycle hooks
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-005
- blocks: STORY-044
- targets: MILESTONE-006
---As an operator, I want the workspace lifecycle hooks whose failure must abort the run — after-create and before-run — run at their defined points, so that a failed dependency bootstrap stops the attempt cleanly instead of running an agent in a broken tree. (Split: best-effort after-run/before-remove hooks are STORY-068.)

- Given after_create configured, When a worktree is newly created, Then it runs with cwd set to the worktree; failure or timeout aborts creation.
- Given before_run configured, When an attempt launches, Then it runs before the agent; failure or timeout fails that attempt.
- Given a hook exceeds hooks.timeout_ms, When it runs, Then it is terminated and the failure recorded.