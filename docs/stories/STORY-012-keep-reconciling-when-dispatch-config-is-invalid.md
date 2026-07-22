---
title: Keep reconciling when dispatch config is invalid
type: story
status: review
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
- targets: MILESTONE-003
---

As an operator, I want a bad config to pause new dispatch without freezing safety reconciliation, so that a typo cannot leave stale agents running while blocking recovery.

- Given per-tick preflight validation fails, When a tick runs, Then reconciliation still runs first, then dispatch is skipped with an operator-visible error.
- Given validation later passes, When a subsequent tick runs, Then dispatch resumes.
- Given a candidate fetch failure, When a tick runs, Then reconciliation still ran, dispatch is skipped, and the tick reschedules.
