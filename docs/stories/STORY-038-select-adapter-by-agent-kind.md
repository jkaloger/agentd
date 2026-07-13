---
title: Select adapter by agent.kind
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
- targets: MILESTONE-004
---

As an operator, I want to choose the adapter via `agent.kind` in config, so that the same daemon can drive Claude (or later codex/opencode) without code changes.

- Given agent.kind: claude, When a session starts, Then the Claude adapter is instantiated behind the trait.
- Given agent.kind: generic, When starting, Then the generic adapter is used with identical canonical events.
- Given an unknown agent.kind, When config loads, Then startup fails naming the unsupported kind.
