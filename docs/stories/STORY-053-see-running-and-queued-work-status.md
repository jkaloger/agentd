---
title: See running and queued work (status)
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
---

As an operator, I want `agentd status` to show active sessions and the retry queue, so that I can tell what the daemon is doing (git status analog).

- Given running sessions, When I run `agentd status`, Then each row shows iter-id, state, session, turn count, and last event/time.
- Given queued retries, When I run `agentd status`, Then each shows iter-id, attempt, due-at, and last error.
- Given aggregates, When I run `agentd status`, Then token and runtime totals show as a live aggregate.
- Given the daemon unreachable, When I run it, Then I get unavailable/timeout rather than a hang.
