---
title: Reclaim an expired lease
type: iteration
status: accepted
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-015
---

## Objective
Expired-lease claim re-claimable: CAS replaces holder, issues fresh lease, bumps monotonic fence so late heartbeats from prior holder rejected. Valid lease still rejects.

## Context
- Implements: STORY-015 (ACs there).
- Architecture: [[ADR-002]] — claim iff absent or lease-expired (ACID CAS).
- Touch: `src/store.rs` `claim` (fence bump on reclaim path); builds on fence from STORY-014.

## Satisfies
STORY-015 AC1–AC3.

## Tasks
1. `claim`: when existing row present + `due_at <= now` (expired) → overwrite, `fence = existing.fence + 1`, new holder, fresh `due_at`.
2. Valid lease (`due_at > now`) → keep returning `ClaimError::AlreadyClaimed(existing)` (unchanged).
3. Confirm heartbeat from prior holder's fence rejected after reclaim (fence check from STORY-014 iter).
4. Tests: reclaim bumps fence + replaces holder + fresh lease; valid lease rejected; stale-fence heartbeat rejected post-reclaim.

## Out of scope
- Restart reconcile / orphan release (STORY-016).
- Projection, retry.

## Principles/conventions
- [[ADR-002]] no-double-assign via single ACID CAS txn.
- TDD; extend `src/store.rs` tests.

## Verification
Monotonic fence strictly increases across successive reclaims of same id.

