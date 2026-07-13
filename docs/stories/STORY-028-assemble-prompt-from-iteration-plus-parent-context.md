---
title: Assemble prompt from iteration plus parent context
type: story
status: in-progress
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-003
- blocks: STORY-029
- targets: MILESTONE-001
---

As an operator, I want the agent to receive the rendered iteration body with its parent story/bug context, so that it has the plan and surrounding intent without manual pasting. (Flagged: scope to immediate parent; transitive ancestry later.)

- Given an item for dispatch, When its prompt is assembled, Then it includes the rendered iteration body via the lazyspec CLI.
- Given a parent story/bug, When assembled, Then the parents context is fetched via the adapter and included.
- Given an attempt value, When rendered, Then attempt is passed so retry guidance can differ.
- Given rendering fails (unknown var/filter), When assembly runs, Then the attempt fails immediately with a typed error.
