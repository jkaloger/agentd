---
title: Append-only event log projection
type: iteration
status: accepted
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-019
- blocks: ITERATION-018
- blocks: ITERATION-019
- blocks: ITERATION-020
---

## Objective
Every durable state change appends one `key=value` line (with iter id) to `.agentd/log`. Append-only, crash-tolerant, non-fatal on write failure.

## Context
- Implements: STORY-019 (ACs there).
- Architecture: [[ADR-002]] — daemon projects human-readable plain files; append-only `.agentd/log` tail/greppable. Store stays authoritative.
- Touch: new `src/projection.rs` (log sink); wire from `src/store.rs` durable mutations or `src/tick.rs`/`src/daemon.rs` post-commit seam. Register module in `src/main.rs`.

## Satisfies
STORY-019 AC1–AC3.

## Tasks
1. `src/projection.rs`: append-only sink — open `.agentd/log` in append mode, write one line per event `ts=<ms> iter=<id> event=<kind> [k=v...]`, flush. Never truncate/rewrite.
2. Emit after each committed durable change: claim, reclaim, release, heartbeat, reconcile release. Fire post-commit (store authoritative first).
3. Write failure → warn to stderr + continue; do not propagate/panic.
4. Reader tolerance: format is line-oriented; a partial trailing line (crash mid-write) is skippable by a parser.
5. Tests: a state change appends a parseable `key=value` line with the id; write failure (bad dir) does not panic; append-only across reopen (prior lines retained).

## Out of scope
- `state.json` / `refs/claims` snapshot → STORY-020.
- `log` CLI reader command (STORY-054, later).

## Principles/conventions
- [[ADR-002]] projection is offline surface, not authority.
- TDD; small focused module.

## Verification
Kill mid-append (simulate partial line) → file still parseable, earlier lines intact.

