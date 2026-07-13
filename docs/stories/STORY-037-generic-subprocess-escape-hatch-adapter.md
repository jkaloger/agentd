---
title: Generic-subprocess escape-hatch adapter
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
- blocks: STORY-038
- targets: MILESTONE-004
---

As an integrator, I want a config-driven generic-subprocess adapter (command, args, cwd) emitting canonical events, so that I can point agentd at a simple new agent without a bespoke adapter.

- Given a generic-subprocess target, When a turn runs, Then the declared command spawns in the worktree and maps to turn_completed/failure.
- Given zero vs non-zero exit, When it ends, Then clean-exit vs failure is reported accordingly.
- Given a target with no structured stream, When it runs, Then output surfaces as other_message/notification without protocol parsing.
