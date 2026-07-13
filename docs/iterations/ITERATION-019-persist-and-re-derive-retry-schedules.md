---
title: Persist and re-derive retry schedules
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-018
---

## Objective
Failed-iteration retry persisted with id, attempt, error, absolute wall-clock `due_at`, replacing prior entry; re-derived + re-armed from `due_at` on restart (not a serialized timer); past `due_at` at load ⇒ eligible now; cleared when re-dispatched or leaves candidacy.

## Context
- Implements: STORY-018 (ACs there). Flagged: `due_at` must be restart-stable wall-clock, not monotonic.
- Architecture: [[ADR-002]] — retry schedules re-derived on restart from persisted `due_at`.
- Touch: `src/store.rs` (new `retries` table + ops), reload/derive helper; project via log (STORY-019 iter).

## Satisfies
STORY-018 AC1–AC4.

## Tasks
1. `src/store.rs`: `retries` table, entry `{ attempt: u32, error: String, due_at: u64 (wall-clock ms) }`. `schedule_retry(id, attempt, error, due_at)` upsert replacing any prior entry for id.
2. `retries()` list; `clear_retry(id)`.
3. Restart re-derivation: `due_retries(now)` (or reload helper) computes pending from stored `due_at`; entry with `due_at <= now` at load ⇒ eligible immediately. No serialized timer handle persisted.
4. Clear entry when item re-dispatched or leaves candidacy.
5. Project schedule/clear to `.agentd/log` (STORY-019).
6. Tests: schedule persists + replaces prior; reload re-derives from `due_at`; past-due eligible at load; cleared on re-dispatch.

## Out of scope
- Backoff interval computation / retry timer firing + re-dispatch loop → scheduling milestone ([[STORY-009]]).
- Projection module itself (STORY-019, prereq).

## Principles/conventions
- [[ADR-002]] backoff survives crashes via durable `due_at`, not timer handles.
- TDD; extend `src/store.rs` tests, incl. reopen/reload.

## Verification
Reopen store after scheduling → same `due_at` re-derived; a `due_at` in the past loads as immediately eligible.

