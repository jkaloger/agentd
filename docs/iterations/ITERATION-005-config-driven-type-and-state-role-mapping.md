---
title: Config-driven type and state-role mapping
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-024
---

## Objective
Config declares which lazyspec type(s) are dispatchable and maps their states to `dispatch`/`active`/`terminal` roles; candidate fetch and classification use it, with the iteration default as fallback.

## Context
- Implements: STORY-024 (see its ACs for mapping/fallback/invalid-state behaviour).
- Architecture: [[ADR-003]] — ticket selection is fully config-driven; shipped default is type `iteration`, `accepted`=dispatch, `in-progress`=active/resume, `complete`/`rejected`/`superseded`=terminal.
- Depends on the config loader (STORY-047 iteration) for parsing; this slice adds role-mapping semantics + DAG validation.

## Tasks
1. Model the type/state-role mapping in config; resolve it into the classification the orchestrator uses for candidate fetch.
2. When no mapping is present, fall back to the ADR-003 iteration/accepted/in-progress/complete default.
3. Validate mapped role states against the lazyspec DAG (via the tracker); fail startup naming the offending mapping when a role names a state the DAG lacks.

## Acceptance Criteria
- STORY-024 AC1–AC3 hold: mapping drives fetch/classification; absent mapping uses the default; a role naming a missing state fails startup naming the mapping.

## Out of scope
- The actual `lazyspec status --json` fetch (STORY-022 iteration); transition execution (STORY-025/026).

