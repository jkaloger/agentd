---
title: Claim by advancing to the active state
type: story
status: in-progress
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-003
- blocks: STORY-026
- blocks: STORY-060
- targets: MILESTONE-001
---

As an operator, I want the daemon to advance a claimed item from dispatch to active via `lazyspec advance` at claim time, so that the backlog reflects work in progress and it is not picked up twice. (Write walking skeleton; daemon owns transitions.)

- Given an item selected for dispatch, When claimed, Then the config-mapped claim transition (default accepted->in-progress) runs via `lazyspec advance` before the agent starts.
- Given the claim advance succeeds, When the next poll fetches, Then the item is active and not re-offered as fresh.
- Given the claim advance fails, When observed, Then the agent does not run and the claim is released for retry.
