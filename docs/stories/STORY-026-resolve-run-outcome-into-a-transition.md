---
title: Resolve run outcome into a transition
type: story
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-003
- blocks: STORY-027
- targets: MILESTONE-001
---

As an operator, I want the daemon to advance a finished item to terminal on success and back/rejected on failure using the agent outcome, so that the lifecycle tracks reality with no drift.

- Given a clean success, When resolved, Then the config-mapped success transition (default in-progress->complete or a handoff state) runs via `lazyspec advance`.
- Given failure/timeout/stall, When resolved, Then the config-mapped failure transition runs.
- Given a configured handoff state, When a run succeeds, Then the item lands there and is treated terminal-for-dispatch.
