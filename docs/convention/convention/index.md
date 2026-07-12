---
title: "Convention"
type: convention
status: draft
author: "Jack Kaloger"
date: 2026-07-12
tags: []
---

## Purpose

**agentd** is a unix, git-like tool for orchestrating LLM coding agents to pick up and execute work — a daemon that assigns agents to open tickets. This document is the project's constitution: the values and constraints that shape every decision. Specific principles live as `principle` documents beside it.

## Conventions

- **Unix-shaped.** Small composable commands, text streams, exit codes. Do one thing well; pipe, don't bundle. A human at a terminal and a script get the same interface.
- **Git-like.** Familiar verbs and mental models — a local store, refs, status, log. If git has a word for it, use that word.
- **Tickets are the unit of work.** An agent picks up an open ticket, executes it, and reports back. The daemon's job is assignment and lifecycle, not doing the work itself.
- **The daemon assigns; agents execute.** Clear separation. The daemon owns scheduling, leasing, and coordination. Agents own the code changes. Neither reaches into the other's job.
- **Spec-driven.** No work without a plan. Decisions are ADRs; work is stories and bugs; execution is iterations. See `CLAUDE.md` for the loop.
- **Observable and recoverable.** Every assignment leaves a trace. State is inspectable and work is resumable — a crashed agent or daemon must not lose or double-assign a ticket.

