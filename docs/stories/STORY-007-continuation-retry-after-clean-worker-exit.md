---
title: Continuation retry after clean worker exit
type: story
status: review
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
- blocks: STORY-008
- targets: MILESTONE-003
---

As a backlog owner, I want an item a worker finished cleanly to be re-checked shortly after, so that still-active work continues in a fresh session.

- Given a normal worker exit, When handled, Then the running entry is removed, totals updated, and a continuation retry (attempt 1, ~1000ms) scheduled.
- Given the timer fires and the item is still active with a slot, When re-dispatched, Then a new session starts.
- Given the item is no longer active, When the timer fires, Then the claim is released without re-dispatch.
