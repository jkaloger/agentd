---
title: Map Claude stream-json to canonical events
type: iteration
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-030
---

## Objective
Translate Claude stream-json lines into the canonical `AgentEvent` model so the orchestrator only sees protocol-agnostic events, tolerating malformed input without crashing the stream.

## Context
- Implements: STORY-030 (see its ACs for message/session/failure/malformed mapping).
- Architecture: [[ADR-004]] — a single canonical `AgentEvent` model every adapter maps its native protocol onto; the orchestrator is protocol-agnostic.
- Complements the claude adapter (STORY-029 iteration), which produces the raw stream.

## Tasks
1. Parse each stream-json message line → `notification`/`other_message` with timestamp and pid.
2. Map a session-start line → `session_started` with the session id.
3. Map turn-failure/cancel lines → `turn_failed`/`turn_cancelled`.
4. On invalid JSON or unknown shape, emit `malformed` without crashing the stream.

## Acceptance Criteria
- STORY-030 AC1–AC4 hold: message lines map with timestamp/pid; session start emits `session_started`; failure/cancel map correctly; invalid/unknown emits `malformed` and the stream survives.

## Out of scope
- Token/runtime aggregation into the store (MILESTONE-004); codex/opencode event mapping.

