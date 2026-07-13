---
title: Work source is lazyspec
type: adr
status: accepted
author: Jack Kaloger
date: 2026-07-12
tags: []
related:
- related-to: ADR-001
---

## Context

The prior design reads work from Linear over GraphQL (SPEC §11). agentd's tracker is **lazyspec** —
local, file-backed, CLI-driven. lazyspec mandates that its CLI is the only writer and that DAG
/ gate / status be read from the CLI, never from graph files directly. lazyspec also has a
gated lifecycle DAG, unlike Linear's passive states.

Open questions: how agentd reads/writes lazyspec, which doc type is a dispatchable "ticket",
which lifecycle states are eligible, and who advances status. The constitution says *"the
daemon owns scheduling, leasing, and coordination … assignment and lifecycle"* — which
contradicts SPEC §11.5 (agent performs tracker writes).

## Decision

We will consume lazyspec by **shelling out to `lazyspec … --json` behind a `Tracker` trait**
(the SPEC's tracker-adapter seam, preserved so Linear or others remain future adapters).
`lazyspec status --json` is candidate fetch; `lazyspec show <id> --json` is state refresh.
Reading graph files directly is rejected. Dispatch is triggered by a **fixed-interval poll**.

Ticket selection is **fully config-driven**: `.agentd/config.toml`
([[adr-006-config-and-cli-surface]]) declares which type(s) are dispatchable and maps their
lifecycle states to orchestration roles (`dispatch` / `active` / `terminal`), mirroring
SPEC's configurable `active_states` / `terminal_states`. The shipped default is the
`iteration` type with `accepted` as dispatch-eligible, `in-progress` as active/resume, and
`complete` / `rejected` / `superseded` as terminal. Blocker rule (SPEC §8.2): a fresh
dispatch-eligible ticket waits until its lazyspec dependencies (and parent work item) are
terminal-complete.

The **daemon owns all lifecycle transitions** via `lazyspec advance`, config-mapped: claim ⇒
`accepted → in-progress`; clean success ⇒ `→ complete` (or a configured handoff state);
failure ⇒ back or `→ rejected`. The daemon respects lazyspec gates and surfaces gate
rejections. This is the deliberate automation of lazyspec's human-initiated `/execute` wall:
a human opts into agentd, and agentd is the executor.

## Consequences

- lazyspec's contract (CLI-only writer, gated DAG) is honored; no coupling to file formats.
- Diverges from SPEC §11.5 — the daemon, not the agent, writes ticket state; the agent
  touches only code. No state drift from an agent forgetting to advance.
- Fixed-interval poll means up to `interval_ms` pickup latency (fs-watch was considered and
  declined for simplicity).
- Because dispatch is config-driven, agentd is not hardwired to "iteration"; teams can point it
  at any type/state shape.
- Depends on the `AgentAdapter` outcome signal ([[adr-004-agent-abstraction]]) to decide the
  success vs failure transition.

