---
title: Run best-effort lifecycle hooks
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-005
- targets: MILESTONE-006
---As an operator, I want the non-fatal workspace lifecycle hooks — after-run and before-remove — run on a best-effort basis, so that teardown and post-run extras run without a hook failure aborting or blocking the run. (Split from STORY-043; STORY-043 keeps the fatal after-create and before-run hooks.)

- Given after_run configured, When an attempt ends, Then the hook runs and its failure or timeout is logged but does not change the run outcome.
- Given before_remove configured, When a worktree is about to be removed, Then the hook runs best-effort and cleanup proceeds even if it fails or times out.
- Given a hook exceeds hooks.timeout_ms, When it runs, Then it is terminated and the outcome recorded, and the run continues.
- Given no such hook is configured, When the lifecycle point is reached, Then it is a no-op.