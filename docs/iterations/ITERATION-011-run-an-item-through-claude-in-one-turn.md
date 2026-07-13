---
title: Run an item through Claude in one turn
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-029
---

## Objective
Launch `claude -p` in the item's worktree, run one turn to completion, and report clean exit (with a `turn_completed` event) vs failure — including a launch error when the binary is missing.

## Context
- Implements: STORY-029 (see its ACs for spawn/completion/failure/launch-error behaviour).
- Architecture: [[ADR-004]] — `AgentAdapter` trait (`start_session`/`run_turn`/`stop`); the claude adapter drives one-shot `claude -p` turns, process exits after; default high-trust non-interactive posture (`input-required` treated as failure), kept configurable.
- Walking skeleton; keep hard-stubbed beyond the one-turn path. Uses the worktree (STORY-039) and prompt (STORY-028) iterations.

## Tasks
1. Implement the `AgentAdapter` claude adapter: `start_session` + `run_turn` spawning `claude -p` with the worktree as cwd and the rendered prompt as input.
2. On the completion signal, emit a `turn_completed` canonical event and report clean exit.
3. On non-zero exit or missing completion signal, report failure.
4. On agent-binary-not-found, surface a launch error rather than hanging.

## Acceptance Criteria
- STORY-029 AC1–AC4 hold: `claude -p` spawns with worktree cwd + prompt input; completion emits `turn_completed` and clean exit; non-zero/no-completion reports failure; missing binary surfaces a launch error.

## Out of scope
- Full stream-json → AgentEvent mapping (STORY-030 iteration); resume/multi-turn; codex/opencode adapters.

