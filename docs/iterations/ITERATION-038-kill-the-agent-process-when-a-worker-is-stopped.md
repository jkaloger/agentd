---
title: Kill the agent process when a worker is stopped
type: iteration
status: complete
author: Jack Kaloger
date: 2026-08-06
tags: []
related:
- implements: BUG-003
---

## Objective
Make stopping a worker actually kill the agent process it launched, so terminal reconcile and stall kills leave no orphaned `claude` behind.

## Context
- Fixes: BUG-003 (repro and impact there).
- Architecture: [[ADR-008]] — the daemon owns worker lifetime; a worker that is no longer tracked must no longer be running.
- `src/daemon.rs` `stop_running` aborts the `JoinHandle` and nothing else. Aborting drops `run_worker` mid-turn, which drops `tokio::process::Child`; tokio does not kill a dropped child unless the `Command` was built with `kill_on_drop(true)`.
- `src/adapter.rs` builds the `Command` (~line 102) and has a `stop` (~line 181) that is a no-op and is never called.
- Three call sites rely on this: terminal reconcile (STORY-010), the stall kill (STORY-011), and the shutdown abort sweep.

## Tasks
1. Write a failing test first: spawn a long-lived child through the adapter's spawn path, drop/abort the owning future, and assert the OS process is gone (e.g. `kill(pid, 0)` / waiting on the pid with a bounded timeout). Use a cheap shell child (`sleep`), not `claude`.
2. Set `kill_on_drop(true)` on the adapter's `Command` so an aborted worker takes its child with it.
3. Decide the fate of `AgentAdapter::stop`: either give it a real kill and call it from `stop_running` before the abort, or delete the dead seam. Do not leave a no-op named `stop`.
4. Assert ordering where the daemon depends on it: the child is dead **before** the claim is released and before `git worktree remove --force` runs, so no live process is writing into a tree being deleted.

## Acceptance Criteria
- The new test fails on the current code and passes after the change.
- Aborting a tracked worker terminates its OS process; nothing survives a terminal reconcile, a stall kill, or shutdown.
- No no-op `stop` remains in the adapter surface.
- `cargo test` green, `cargo clippy --all-targets` clean.

## Out of scope
- Retry firing (BUG-002 / its own iteration).
- Graceful SIGTERM-then-SIGKILL escalation — a hard kill is sufficient here; note it if it looks worth an ADR.

