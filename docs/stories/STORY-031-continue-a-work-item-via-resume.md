---
title: Continue a work item via --resume
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
---

As an operator, I want an item that stays active continued on the same Claude session via `claude -p --resume <id>`, so that follow-up turns keep prior context.

- Given a completed first turn, Then the adapter retained the Claude session_id from the stream.
- Given a retained id and a still-active item, When a continuation is requested, Then a fresh `claude -p --resume <id>` spawns in the same worktree.
- Given a continuation turn, When the prompt is built, Then it sends continuation guidance only, not the original prompt.
- Given no session id captured, When continuation is requested, Then the adapter reports it cannot resume.
