---
title: Claim an iteration exactly once (ACID)
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-013
---

## Objective
Claim each iteration at most once via a single ACID compare-and-set on a redb `claims` table, so two agents never share a ticket and a committed claim survives restart.

## Context
- Implements: STORY-013 (see its ACs for single-write/CAS/crash-survival behaviour).
- Architecture: [[ADR-002]] — agentd store is redb (embedded, ACID), daemon-owned; a claim is one CAS transaction (claim iff absent or lease-expired); leases carry a TTL.
- Touch: the redb store module and the `claims` table schema.

## Tasks
1. Open/own the redb store in the daemon; define the `claims` table (id → claim record with lease/`due_at`).
2. Implement claim as one ACID transaction: succeed iff the id is absent or its lease expired; return the record on success.
3. Ensure concurrent claims for the same id resolve to exactly one winner (CAS, no torn write).
4. Verify a committed claim is re-read identically after a daemon kill/restart.

## Acceptance Criteria
- STORY-013 AC1–AC3 hold: claim writes one ACID row and returns the record; concurrent claims yield exactly one success; committed claim persists across restart.

## Out of scope
- Lease heartbeat/renewal and reconcile (later milestone); the lazyspec claim transition (STORY-025 iteration).

