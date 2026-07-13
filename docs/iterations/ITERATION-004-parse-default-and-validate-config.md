---
title: Parse, default, and validate config
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-047
---

## Objective
Load `.agentd/config.toml`, apply documented defaults, and reject invalid values up front with field-scoped errors — so a bad config fails at startup, not mid-run.

## Context
- Implements: STORY-047 (see its ACs for defaults/validation/unknown-key/per-state-cap behaviour).
- Architecture: [[ADR-006]] — TOML config sibling to `.lazyspec.toml`; strict up-front validation; `agent.max_turns` parsed and defaulted here regardless of adapter (per [[ADR-004]]).
- Touch: config model (serde) + loader/validator seam consumed by `agentd start`.

## Tasks
1. Define the serde config model with documented defaults for every optional field (including `agent.max_turns`).
2. Validate required fields and typed ranges; on missing/out-of-range, fail with a field-scoped error naming the key.
3. Ignore unknown top-level keys for forward compatibility.
4. Drop an invalid per-state cap entry while applying the rest.

## Acceptance Criteria
- STORY-047 AC1–AC4 hold: defaults applied; missing/out-of-range fails naming the key; unknown keys ignored; bad per-state cap dropped, rest applied.

## Out of scope
- The dispatch type/state-role mapping semantics (STORY-024's iteration); hot-reload watching.

