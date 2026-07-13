---
title: Run an item through Claude in one turn
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
- blocks: STORY-031
- blocks: STORY-032
- blocks: STORY-035
- blocks: STORY-038
- blocks: STORY-060
- targets: MILESTONE-001
---

As an operator, I want agentd to launch `claude -p` in a items worktree, run one turn to completion, and report clean exit vs failure, so that an iteration executes without me driving the agent. (Walking skeleton; keep hard-stubbed.)

- Given a prepared worktree, When the adapter starts a session, Then `claude -p` spawns with the worktree as cwd and the rendered prompt as input.
- Given the completion signal, When the turn ends, Then a turn_completed canonical event is emitted and clean exit reported.
- Given non-zero exit or no completion signal, When the stream ends, Then failure is reported.
- Given the agent binary is not found, When start is attempted, Then a launch error surfaces rather than hanging.
