---
title: Dispatch only eligible items
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
- blocks: STORY-061
- blocks: STORY-004
- blocks: STORY-005
- blocks: STORY-064
- targets: MILESTONE-003
---

As a backlog owner, I want the daemon to dispatch only items passing all eligibility rules, so that agents never run on incomplete, wrong-status, or already-claimed work.

- Given an item missing id/identifier/title/status, When evaluated, Then it is skipped.
- Given a status not in active_states (or in terminal_states), When evaluated, Then it is skipped.
- Given an item already running or claimed, When evaluated, Then it is not dispatched again.
- Given an item passing every rule with a free slot, When evaluated, Then it is dispatched.
