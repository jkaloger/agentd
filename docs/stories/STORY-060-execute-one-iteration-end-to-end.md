---
title: Execute one iteration end-to-end
type: story
status: in-progress
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
- targets: MILESTONE-001
---

As an operator, I want one eligible iteration to flow through the whole stack — polled from lazyspec, claimed in the durable store, run by a real agent in its own worktree, and advanced on the outcome — so that the complete path is proven on a single ticket before any layer is hardened. (Integrating walking skeleton across STORY-001/022/025/029/039.)

- Given a git project with one dispatch-eligible iteration and a real agent configured, When the daemon ticks, Then it claims the item, creates its worktree, runs the agent to completion, and advances the item to its terminal state on clean exit.
- Given the run finishes, When I query status and log, Then the iteration shows the full trace: claimed, running, terminal, with token/runtime totals and the resulting lazyspec transition.
- Given the agent exits non-clean, When the tick completes, Then the item is transitioned per the failure policy and its claim released, with the worktree preserved for inspection.
- Given the daemon is killed mid-run, When it restarts, Then reconcile resolves the orphaned claim without double-dispatching the iteration.
