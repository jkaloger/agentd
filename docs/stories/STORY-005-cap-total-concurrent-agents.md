---
title: Cap total concurrent agents
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-007
- blocks: STORY-006
- targets: MILESTONE-003
---

As an operator, I want a hard ceiling on concurrent agents, so that the host is not overwhelmed.

- Given max_concurrent_agents=N with N running, When more eligible items are evaluated, Then none dispatch (available slots 0).
- Given a worker exits freeing a slot, When the next tick runs, Then dispatch resumes.
- Given the dispatch loop hits zero slots mid-loop, When it continues, Then it breaks and leaves the rest for a future tick.
