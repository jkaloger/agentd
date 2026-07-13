---
title: Boot and pick up one item
type: story
status: in-progress
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
- blocks: STORY-002
- blocks: STORY-003
- blocks: STORY-007
- blocks: STORY-010
- blocks: STORY-012
- blocks: STORY-060
- targets: MILESTONE-001
---

As an operator, I want to start agentd and have it pick up a single eligible iteration and run an agent on it, so that work begins without hand-scripting. (Walking skeleton: stubbed store/adapter/workspace.)

- Given a valid config and one eligible item, When I run `agentd start`, Then the daemon validates config, binds its socket, schedules an immediate tick, and spawns exactly one worker.
- Given the daemon is running, When I query status, Then the item shows as Running with identifier and started_at.
- Given startup validation fails, When I run `agentd start`, Then it aborts non-zero with an operator-visible error and leaves no socket bound.
