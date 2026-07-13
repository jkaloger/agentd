---
title: Hold a lease with TTL and heartbeat
type: iteration
status: in-progress
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-014
- blocks: ITERATION-017
- blocks: ITERATION-020
---

## Objective
Claim carry TTL lease + monotonic fence. Live worker renew by heartbeat. Mismatched holder/fence rejected. Expiry observable (row not deleted).

## Context
- Implements: STORY-014 (ACs there).
- Architecture: [[ADR-002]] — leases carry TTL, renewed by heartbeat.
- Touch: `src/store.rs` (`ClaimRecord`, `claim`, new `heartbeat`), callers in `src/dispatch.rs`/`src/tick.rs` (TTL already threaded via `DEFAULT_LEASE_TTL`).

## Satisfies
STORY-014 AC1–AC4.

## Tasks
1. `ClaimRecord`: add `fence: u64`. `claim` fresh row → `fence = 1`, `due_at = now + ttl`.
2. `Store::heartbeat(id, holder, fence, now, ttl)` → one write txn: extend `due_at = now + ttl`; reject (typed err) if row absent, or holder/fence ≠ stored.
3. Expiry = `due_at <= now`. No delete on expiry — row stays, observably expired via `get`.
4. Tests: fresh claim carries expiry+fence; heartbeat extends `due_at`; holder mismatch + fence mismatch rejected; past-expiry row still present via `get`.

## Out of scope
- Fence bump on reclaim → STORY-015 (next iter).
- Reconcile/release-on-restart, projection, retry.

## Principles/conventions
- [[ADR-002]] durable ACID; redb single write txn per state change.
- TDD (`testing` skill); existing `src/store.rs` test style.

## Verification
Heartbeat with stale fence after a (future) reclaim must be rejectable — assert fence stored + checked now.

