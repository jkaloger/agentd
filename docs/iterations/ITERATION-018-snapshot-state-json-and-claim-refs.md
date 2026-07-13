---
title: Snapshot state.json and claim refs
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-020
---

## Objective
Project live claim/lease state to `.agentd/state.json` (atomic temp+rename, always valid JSON) and `refs/claims/<iter-id>` (holder/fence), removed on release. Write failure non-fatal.

## Context
- Implements: STORY-020 (ACs there).
- Architecture: [[ADR-002]] — git-style plain-text surface: `state.json` (jq-able) + `refs/claims/<iter-id>` pointers.
- Touch: `src/projection.rs` (from STORY-019 iter); wire alongside log emit in claim/release/reconcile paths.

## Satisfies
STORY-020 AC1–AC3.

## Tasks
1. `state.json` writer: serialize current claims snapshot (`store.claims()`), write temp file + `rename` (atomic); result always valid JSON.
2. `refs/claims/<iter-id>`: active claim → write file with holder + fence; release → remove file.
3. Wire into claim / reclaim / release / reconcile-release.
4. Any projection write failure → warn + continue; store authoritative, no crash.
5. Tests: `state.json` is atomic (temp+rename) + valid JSON reflecting claims; ref created on claim, removed on release; write failure no crash.

## Out of scope
- Log projection (STORY-019, prereq).
- `status` socket reader (STORY-053, later).

## Principles/conventions
- [[ADR-002]] fast binary core behind plain-text surface.
- TDD; reuse `src/projection.rs`.

## Verification
Concurrent-reader safety: a reader never sees a truncated `state.json` (temp+rename guarantees).

