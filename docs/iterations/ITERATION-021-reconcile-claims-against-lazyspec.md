---
title: Reconcile claims against lazyspec
type: iteration
status: accepted
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-017
---

## Objective
Restart reconcile cross-checks each claim against lazyspec state: absent → drop+record; terminal-complete → finalize (not re-dispatch); active → retain; lazyspec read fail → retain all + retry next cycle.

## Context
- Implements: STORY-017 (ACs there). Worktree half is STORY-065.
- Architecture: [[ADR-002]] — lazyspec is truth for *what work exists*; reconcile cross-checks claims against it.
- Touch: `src/tracker.rs` (`DocView`/`fetch_doc` expose doc `status`), `src/mapping.rs` `RoleMapping::classify` (Dispatch/Active/Terminal via `StateRole`), `src/tick.rs` `reconcile`.

## Satisfies
STORY-017 AC1–AC4.

## Tasks
1. Expose claim's current lazyspec state to reconcile: add `status` to `DocView` (parse `show --json`); classify via `RoleMapping` (`StateRole`).
2. Per claim in `reconcile`:
   - doc absent (not found) → drop claim + record (AC1).
   - `StateRole::Terminal` complete → finalize (release, not re-dispatch) (AC2).
   - `StateRole::Active` → retain (AC3).
3. Distinguish "absent" (doc gone) from "read failed" (CLI/spawn error): read failure → retain ALL claims untouched, return so next cycle retries (AC4).
4. Tests per AC: absent→dropped+recorded; terminal→finalized not re-offered; active→retained; read error→all retained.

## Out of scope
- On-disk worktree cross-check → STORY-065.
- Expired-lease orphan release (STORY-016, prereq).

## Principles/conventions
- [[ADR-002]] lazyspec = truth for existence; store = truth for claims.
- [[ADR-003]] work source is lazyspec.
- TDD; extend reconcile tests with a fake tracker returning per-id status.

## Verification
Read failure must be conservative: zero claims mutated when lazyspec errors.

