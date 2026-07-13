---
title: Gate dispatch until dependencies and parent are terminal
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-003
- blocks: STORY-025
- targets: MILESTONE-003
---

As a backlog owner, I want a dispatch-eligible iteration held until its lazyspec dependencies and parent work item are terminal-complete, so that an agent never starts work whose prerequisites are unfinished. (Implements the ADR-003 / SPEC §8.2 blocker rule.)

- Given an eligible iteration with a dependency that is not terminal, When the daemon evaluates candidates, Then the iteration is skipped and the blocking reason is recorded.
- Given an eligible iteration whose parent work item is not terminal-complete, When the daemon evaluates candidates, Then it is skipped as blocked.
- Given all dependencies and the parent become terminal, When the next tick runs, Then the iteration becomes dispatch-eligible.
- Given a dependency reference cannot be resolved in lazyspec, When evaluation runs, Then the iteration is treated as blocked and the unresolved reference is surfaced, not silently dispatched.
