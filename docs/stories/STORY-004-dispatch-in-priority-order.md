---
title: Dispatch in priority order
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-007
- targets: MILESTONE-003
---

As a backlog owner, I want the most important item dispatched first when slots are scarce, so that high-priority work is not starved.

- Given several eligible items and one slot, When sorted, Then the lowest priority number goes first.
- Given equal priority, When sorted, Then oldest created_at wins, then identifier lexicographically.
- Given null/unknown priority, When sorted, Then it sorts after all defined priorities.
