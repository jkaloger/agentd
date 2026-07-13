---
title: Surface and recover from gate-rejected transitions
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-003
- targets: MILESTONE-003
---

As an operator, I want the daemon to detect when `lazyspec advance` is refused by a lifecycle gate and surface it without corrupting claim state, so that I can see why an item is stuck.

- Given a gate rejection, When advance returns it, Then the daemon distinguishes it from a transport error and logs item, transition, and reason.
- Given a gate-rejected claim, When handled, Then the agent does not run and the item stays consistent (no half-claim).
- Given repeated gate rejections, When polls continue, Then the daemon does not busy-loop (bounded/deduplicated attempts).
