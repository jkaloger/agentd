---
title: Assemble prompt from iteration plus parent context
type: iteration
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-028
---

## Objective
Assemble the agent prompt from the rendered iteration body plus its immediate parent story/bug context and the attempt value, failing immediately with a typed error on a render fault.

## Context
- Implements: STORY-028 (see its ACs for body/parent/attempt/render-failure behaviour).
- Architecture: [[ADR-006]] — thin per-ticket prompt: inject rendered iteration + parent context and say "execute it"; strict-rendered templates (unknown var/filter fails). Rich content pulled via `lazyspec show <id> --json` through the [[ADR-003]] `Tracker`.
- Flagged in story: scope to the immediate parent only; transitive ancestry is later.

## Tasks
1. Render the iteration body via the lazyspec CLI and include it in the prompt.
2. Fetch the immediate parent story/bug context via the tracker adapter and include it.
3. Pass the attempt value into rendering so retry guidance can differ.
4. On a render fault (unknown variable/filter), fail the attempt immediately with a typed error.

## Acceptance Criteria
- STORY-028 AC1–AC4 hold: prompt includes rendered iteration + parent context; attempt is passed; render fault fails immediately with a typed error.

## Out of scope
- Transitive ancestor context; launching the agent (STORY-029 iteration).

