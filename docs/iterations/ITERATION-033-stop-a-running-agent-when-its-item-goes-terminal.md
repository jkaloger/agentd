---
title: Stop a running agent when its item goes terminal
type: iteration
status: review
author: Jack Kaloger
date: 2026-07-14
tags: []
related:
- implements: STORY-010
---

## Objective
Introduce a new running-worker reconcile pass (`reconcile_running`) that refreshes every in-flight item's state per id and acts — now terminal → terminate the worker and clean its workspace; still active → refresh the snapshot and keep running; neither active nor terminal → terminate without cleanup; refresh failure → keep all workers and retry next tick.

## Context
- Implements: STORY-010 (ACs there). Adds reconciliation of live, in-flight workers — distinct from the persisted-claim reconcile.
- Precondition/dependency: today `spawn_orchestrator` (`src/daemon.rs`) awaits a single `run_tick` inline — there is no periodic reconcile loop and no separately-spawned, abortable worker. This slice depends on the poll loop (STORY-002 / [[ITERATION-023]]) and on the inline-await → spawn/track/abort worker conversion; do not assume an existing per-tick loop.
- Architecture: [[ADR-001]] the orchestrator holds authority to start and stop agents; [[ADR-003]] lazyspec is the truth for item state; [[ADR-002]] the store is the truth for claims (release frees the slot); [[ADR-005]] each run owns an isolated git worktree.
- Running-worker representation: `src/daemon.rs` `DaemonState.running: Vec<RunningItem>` (`RunningItem { id, identifier, started_at_ms }`), pushed by `on_activated` in `spawn_orchestrator` and cleared when the tick resolves. It carries neither the worktree path nor a stop handle for the in-flight turn — both are added here. ITERATION-034 (stall) will target `reconcile_running` by name.
- Per-id refresh (STORY-023, [[ITERATION-032]]): `src/tracker.rs` `Tracker::lookup_doc(id) -> Result<DocLookup, TrackerError>` returns `DocLookup::Present(DocView{doc_type,status,..})` / `DocLookup::Absent` / `Err` (read failure). Classify with `src/mapping.rs` `RoleMapping::classify(doc_type, status) -> Option<StateRole>` (`src/config.rs` `StateRole::{Active,Terminal,..}`).
- Existing claim reconcile: `src/tick.rs` `reconcile` reads every lease's state before any store write so a read failure mutates nothing (STORY-017 AC4) — mirror that read-before-write shape here.
- Termination + cleanup seams: `src/adapter.rs` `AgentAdapter::stop(session)`; `src/workspace.rs` (worktree lifecycle — no public removal exists yet); `src/store.rs` `Store::release`; `src/projection.rs` `Snapshot::{set_ref, write_state, remove_ref}` and `Projection::record`.

## Satisfies
STORY-010 AC1–AC4.

## Tasks
1. Represent what stopping + cleaning needs: extend `RunningItem` (`src/daemon.rs`) with the worktree path and a stop handle for the in-flight turn (the worker's `JoinHandle`/abort seam plus `AgentAdapter::stop`); keep `running_view` and `on_activated` compiling.
2. Add a new `reconcile_running` function in `src/tick.rs`, alongside the existing claim-based `reconcile`: refresh **all** running ids via `lookup_doc` first; classify each via `RoleMapping::classify`; produce a per-id decision — `Terminal` → terminate + clean (AC1); `Active` → refresh snapshot, keep (AC2); dispatch / unmapped / `Absent` → terminate, no cleanup (AC3). If any `lookup_doc` returns `Err`, take no action and signal retry (AC4). Name it explicitly — ITERATION-034 (stall) targets `reconcile_running`.
3. Wire the decision in the daemon: terminal → `AgentAdapter::stop`/abort the worker, remove the worktree, `Store::release` + `Snapshot::remove_ref`, drop the `running` entry, project the outcome; active → `Snapshot::set_ref`/`write_state`, leave running; other → stop the worker and `Store::release` + `Snapshot::remove_ref` (the slot must free) but leave the worktree on disk, then drop the `running` entry.
4. Add a minimal worktree removal in `src/workspace.rs` (`git worktree remove`) for the cleaned case, returning a distinct error the caller can log.
5. TDD per AC using the `src/daemon.rs` fakes (`FakeTracker` with per-id `lookup_doc`, a recording/blocking adapter) plus on-disk worktree presence checks: terminal → worker stopped, tree gone, claim released; active → still running, snapshot updated; non-active/non-terminal → stopped, tree preserved; refresh-fail → every worker still running.

## Out of scope
- Stall/timeout-driven termination and retry — STORY-011 (this slice acts only on refreshed lazyspec state).
- The `before_remove` hook and the startup stale-worktree sweep — STORY-044 (this slice removes a live worker's tree on terminal, nothing more).
- Restart claim reconcile over persisted leases — unchanged (`src/tick.rs` `reconcile`, STORY-017/065).

## Principles/conventions
- [[ADR-001]] the orchestrator, not the agent, decides to stop; [[ADR-003]] act on refreshed state, not cached; [[ADR-002]] a stopped worker's claim must be released so the slot frees.
- Read-before-write: refresh all running ids before mutating anything, so a single refresh failure leaves every worker running (mirror `reconcile`).
- TDD; extend `src/daemon.rs` / `src/tick.rs` `mod tests` with the existing fakes.

## Verification
The refresh-failure path mutates nothing: when any `lookup_doc` errors, no worker is stopped, no worktree removed, and no claim released — assert every running item survives and retries next tick.
