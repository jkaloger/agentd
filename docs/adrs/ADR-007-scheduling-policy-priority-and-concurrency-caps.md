---
title: Scheduling policy priority and concurrency caps
type: adr
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
---

## Context

ADR-001 fixes the orchestrator as the single authority that supervises per-ticket workers, but it does not decide the *policy* that authority applies when work outstrips capacity. SPEC §8.2, §8.3 sorts eligible work by priority then age and caps concurrency both globally and per active-state. agentd inherited those behaviours as stories (priority ordering, global cap, per-status cap) with no ADR sanctioning them — a decision made in code by default rather than on the record.

## Decision

Scheduling policy is an explicit, config-driven decision owned here:

- **Priority ordering.** When eligible items exceed free slots, dispatch highest-priority first; ties break by age (oldest first); missing priority sorts last. Mirrors SPEC §8.2.
- **Global concurrency cap.** A hard ceiling on concurrent agents (`max_concurrent`) protects the host; when full, further eligible work waits.
- **Per-status concurrency cap.** Optional per-active-state ceilings limit agents in an expensive phase independently of the global cap, falling back to the global cap when unset. Mirrors SPEC §8.3.

All three are read from `.agentd/config.toml` ([[adr-006-config-and-cli-surface]]) and re-applied on hot-reload; they govern future dispatch only, never in-flight runs.

## Consequences

- Priority/caps become a recorded, testable contract rather than an implicit default.
- STORY-004 (priority), STORY-005 (global cap), STORY-006 (per-status cap) trace here.
- Caps are advisory ceilings, not reservations: freeing a slot re-evaluates the full eligible set against current priority, so a late high-priority item can jump ahead of a waiting lower one.
- Leaves fair-share / weighted scheduling out of scope; revisit if starvation appears under sustained load.
