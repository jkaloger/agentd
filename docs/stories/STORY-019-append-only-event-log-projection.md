---
title: Append-only event log projection
type: story
status: in-progress
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
- blocks: STORY-054
- targets: MILESTONE-002
---

As an operator, I want every state transition appended to a plain-text .agentd/log, so that I can tail/grep full history without a running daemon.

- Given any durable state change, When it commits, Then a key=value line with the iteration id is appended to .agentd/log.
- Given a crash mid-write, When the daemon restarts, Then the log stays append-only and parseable (partial trailing line tolerated).
- Given the log sink write fails, When it happens, Then the daemon continues with a warning; the store stays authoritative.
