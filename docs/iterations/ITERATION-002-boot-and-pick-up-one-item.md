---
title: Boot and pick up one item
type: iteration
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-001
---

## Objective
`agentd start` validates config, binds its control socket, schedules one immediate tick, and spawns exactly one worker for a single eligible item (rest of stack stubbed).

## Context
- Implements: STORY-001 (see its ACs for the boot/status/abort contract).
- Architecture: [[ADR-001]] — daemon owns authoritative state; unix domain socket is the only control plane; tokio orchestrator task supervises per-ticket workers.
- Walking skeleton: store, tracker adapter, and workspace are hard-stubbed here; this slice proves the boot path and worker spawn only.
- Builds on the scaffold iteration.

## Tasks
1. Wire `agentd start` to load+validate config (call into the config loader seam; stub if not yet landed) and abort non-zero on failure with no socket bound.
2. Bind the unix domain socket control plane; ensure clean teardown on abort.
3. Start the tokio orchestrator task; schedule an immediate tick that selects one stubbed eligible item and spawns exactly one worker.
4. Expose `status` over the socket so the item reports `Running` with identifier and `started_at`.

## Acceptance Criteria
- STORY-001 AC1–AC3 hold: successful start binds socket + spawns one worker; `status` shows Running with identifier/started_at; validation failure aborts non-zero and leaves no socket bound.

## Out of scope
- Real tracker fetch, durable claim, real worktree, real agent run (their own iterations). Concurrency beyond one worker.

