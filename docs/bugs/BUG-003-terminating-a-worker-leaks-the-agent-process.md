---
title: Terminating a worker leaks the agent process
type: bug
status: reported
author: Jack Kaloger
date: 2026-08-06
tags: []
related:
- related-to: MILESTONE-003
- related-to: ADR-008
---

## Expected vs actual

**Expected:** stopping a worker stops the agent. When reconcile sees an item go terminal (STORY-010) or a worker stall past `stall_timeout_ms` (STORY-011), the `claude` process the daemon launched is killed before its claim is released and its worktree removed.

**Actual:** `stop_running` only calls `JoinHandle::abort`. That drops the `run_worker` future mid-turn, which drops the `tokio::process::Child` — and a dropped `Child` does **not** kill the process. The adapter spawns without `kill_on_drop(true)` and `ClaudeAdapter::stop` is a no-op that is never called on the abort path. The agent keeps running, unsupervised and unowned.

## Repro

1. Dispatch an item and let its agent run.
2. Move the item to a terminal state out-of-band (STORY-010 path), or let the agent go quiet past `stall_timeout_ms` (STORY-011 path).
3. The daemon reports the worker terminated, releases the claim and runs `git worktree remove --force` on the tree.
4. `ps` still shows the `claude` process, with the just-deleted worktree as its cwd.

## Impact

- "Kill a stalled agent" kills nothing: a hung or runaway agent keeps burning tokens while its slot is handed to a second agent — a double assignment against the same item, which the project's recoverability invariant forbids.
- The daemon deletes a directory a live process is writing to, and that process may still run `lazyspec update` against the item the daemon has already released.

## Notes

ITERATION-033 task 3 named a stop seam; the implementation dropped it in favour of a bare `abort`. Fix is either `.kill_on_drop(true)` on the adapter's `Command`, a real `AgentAdapter::stop` that kills, or both.

Found by the MILESTONE-003 end-of-chunk review.

