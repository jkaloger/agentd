---
title: Fetch dispatchable iterations from lazyspec
type: iteration
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-022
---

## Objective
Fetch dispatch-eligible iterations by shelling out to `lazyspec status --json` behind a `Tracker` trait, normalize them to candidates, and return a typed error on CLI failure.

## Context
- Implements: STORY-022 (see its ACs for invoke/normalize/error behaviour).
- Architecture: [[ADR-003]] — consume lazyspec only via `lazyspec … --json` behind a `Tracker` trait (never read graph files); fixed-interval poll triggers fetch. Uses the role mapping from STORY-024's iteration to select dispatch-eligible state.
- Read walking skeleton: real CLI read behind the trait seam.

## Tasks
1. Define the `Tracker` trait and a lazyspec implementation that shells out to `lazyspec status --json`.
2. Return only dispatch-eligible (default: accepted) candidates; never touch graph files.
3. Normalize each candidate to expose id, identifier, title, body, state, parent link, dependency refs.
4. On non-zero exit or unparseable output, return a typed error so the tick can be skipped.

## Acceptance Criteria
- STORY-022 AC1–AC3 hold: fetch invokes the CLI and returns only eligible iterations; normalized fields present; CLI failure yields a typed error and a skippable tick.

## Out of scope
- Blocker/dependency gating; claiming or advancing (STORY-013/025 iterations); prompt assembly.

