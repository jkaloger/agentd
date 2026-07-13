---
title: Agent failure reason is swallowed
type: bug
status: reported
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- related-to: ADR-002
- related-to: ADR-004
---

## Expected vs actual

**Expected:** when a dispatched item resolves to error, the operator can see *why*. `agentd log <id>` / `agentd show <id>` surface the failure reason (e.g. `cannot assemble prompt: ITERATION-023 has no parent work item`, `agent exited with status 3`, `cannot launch \`claude\``).

**Actual:** `agentd status` shows `terminal:error` with no cause. The reason is captured only in the in-memory `RunRecord` and never projected. The append-only `.agentd/log` records `claim` then `release` — no failure event, no reason.

## Repro

1. Dispatch an iteration with no `implements` link (e.g. ITERATION-023, `related: []`).
2. Daemon claims it, prepares a worktree, then `assemble_prompt` fails with `MissingParent` (`tick.rs:247`) before any agent runs.
3. `status` reports `terminal:error` in ~69ms; `log` shows only `claim`/`release`. The operator has no way to learn the cause without attaching a debugger.

## Scope

All post-claim faults, not just the observed one:
- pre-agent faults: prompt assembly (`tick.rs:247`) and worktree prep failures.
- agent turn outcomes: `TurnOutcome::Failed { reason }` and `TurnOutcome::LaunchError { message }`.

None of these project their reason to `.agentd/log`. Fix: emit a failure event carrying the reason to the projection (ADR-002 surface), sourced from the canonical agent events / outcome (ADR-004), and surface it via `log`/`show`.
