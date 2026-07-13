---
title: Persist and re-derive retry schedules
type: story
status: accepted
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
- targets: MILESTONE-002
---

As a developer, I want a failed iteration retry stored with its due time and re-armed after restart, so that backoff survives crashes. (Flagged: needs a restart-stable wall-clock due_at, not monotonic.)

- Given a failure, When a retry is scheduled, Then a durable entry stores id, attempt, error, and absolute due_at, replacing any prior entry.
- Given a restart, When entries load, Then each pending retry is re-derived from due_at (not a serialized timer) and re-armed.
- Given a due_at already past at load, When loaded, Then it is eligible immediately.
- Given the item is re-dispatched or leaves candidacy, When observed, Then its retry entry is cleared.
