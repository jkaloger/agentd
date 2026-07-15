---
title: Dispatch in priority order
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-004
---

## Objective
Order the eligible candidate set before selecting so the highest-priority item is dispatched first when slots are scarce.

## Context
- Implements: STORY-004 (ACs there).
- Architecture: [[ADR-007]] — scheduling policy is orchestrator-owned; sort eligible work by priority, then age, missing priority last (mirrors SPEC §8.2).
- Touch: `src/tracker.rs` `Candidate`, `RawDoc`, and `normalize` — all three gain new fields (`Candidate.priority: Option<u32>`, `Candidate.created_at: String`; matching deserialized fields on `RawDoc`). `RawDoc` today parses only id/path/title/status/type/related, so the priority and created_at fields do not exist yet and must be added. `src/tick.rs` `run_tick` orders the `fetch_dispatchable` result before `candidates.into_iter().next()`.
- Source of the new fields is unconfirmed: `status --json` does emit a `date` key and an `attributes` map, but `attributes` is empty on every current doc, so no priority is actually published yet — Task 1 must verify the real emitted keys before wiring them.

## Satisfies
STORY-004 AC1–AC3.

## Tasks
1. Extend `Candidate` with `priority: Option<u32>` and `created_at: String`; parse both in `RawDoc`/`normalize` (priority from the doc `attributes` map, created_at from the `date` field). Update the existing tracker fake/fixtures that build a `Candidate`.
2. Add a candidate ordering in `src/tick.rs` (comparator/`sort_by`): lowest priority number first (AC1); tie → oldest `created_at`, then `identifier` lexicographically (AC2); `None` priority sorts after all defined (AC3). Apply it to the fetched list before `.next()`.
3. Tests per AC using the ordering directly over a constructed `Vec<Candidate>` (mixed priorities, equal-priority age/identifier ties, null vs defined) — no daemon.

## Out of scope
- Global concurrency cap → STORY-005; per-status cap → STORY-006. This slice orders only; it does not change how many slots are free.
- Any config key: ordering reads candidate fields, not `.agentd/config.toml`.

## Principles/conventions
- [[ADR-007]] priority → age → missing-last; policy governs future dispatch only.
- TDD; extend the touched file's `mod tests` with the existing `Candidate` fixtures.

## Verification
`None` priority never outranks a defined priority, even a large one; equal priority + equal created_at falls through to `identifier` deterministically.
