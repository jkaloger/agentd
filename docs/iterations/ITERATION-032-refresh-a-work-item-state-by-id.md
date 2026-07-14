---
title: Refresh a work item state by id
type: iteration
status: review
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-023
---

## Objective
Confirm the per-id refresh seam (`lazyspec show <id> --json` → normalized model, distinguishable not-found, terminal role reported) satisfies STORY-023, closing the one uncovered AC with a test.

## Context
- Implements: STORY-023 (ACs there).
- Architecture: [[ADR-003]] — `lazyspec show <id> --json` is the state-refresh call; lifecycle states map to roles (`dispatch`/`active`/`terminal`) config-driven.
- Already present (STORY-017 reconcile): `src/tracker.rs` `Tracker::lookup_doc` shells `show <id> --json`, normalizes via `parse_doc_view` into `DocView { id, doc_type, title, body, status }`, and returns `DocLookup::Present(view)` vs `DocLookup::Absent` (missing doc via `is_not_found`) vs `Err(TrackerError)` (read failure). AC1 and AC3 are met and unit-tested (`lookup_doc_returns_present_with_the_parsed_status`, `lookup_doc_maps_a_not_found_exit_to_absent`, `lookup_doc_surfaces_other_failures_as_read_errors`).
- Role classification: `src/mapping.rs` `RoleMapping::classify(doc_type, status) -> Option<StateRole>` (`src/config.rs` `StateRole::Terminal`), applied caller-side in `src/tick.rs` `reconcile`.
- Gap: no seam-level test asserts a refreshed now-terminal doc reports `StateRole::Terminal` (AC2) — it is only exercised transitively in reconcile.

## Satisfies
STORY-023 AC1–AC3. `DocView` carries no role field: AC2 ("refreshed item reports the terminal role") is met by composition — `DocView.status` from the refresh seam plus `RoleMapping::classify` — not by a role stored on the record.

## Tasks
1. Verify AC1: `lookup_doc` issues `show <id> --json` per id and normalizes to `DocView`. If not already asserted, add a `RecordingCli`-style test confirming the exact args and the parsed `doc_type`/`title`/`status`.
2. Verify AC3: `DocLookup::Absent` is returned distinctly from an `Err` read failure — covered by existing tests; extend only if a case is missing.
3. Close AC2: add a `src/tracker.rs` `mod tests` case that refreshes a now-terminal doc via `lookup_doc` and asserts `RoleMapping::classify(&view.doc_type, &view.status) == Some(StateRole::Terminal)`, so the refreshed record reports the terminal role through the seam.

## Out of scope
- Acting on a refreshed state (release/finalize/retain) — that is reconcile, STORY-017 (`src/tick.rs`).
- Any change to the `Tracker` trait shape or `DocLookup` variants — the seam already exists.

## Principles/conventions
- [[ADR-003]] `show <id> --json` = refresh; role mapping is config-driven, not hardcoded.
- TDD; extend `src/tracker.rs` `mod tests` using the existing `FakeCli`/`FailingShow` fakes.

## Verification
The added AC2 test drives classification through the refresh path (`lookup_doc` → `DocView` → `RoleMapping::classify`), not by constructing a `DocView` directly.
