---
title: Durable state and leasing
type: adr
status: accepted
author: Jack Kaloger
date: 2026-07-12
tags: []
related:
- related-to: ADR-001
---

## Context

The prior orchestration design keeps scheduler state entirely in memory and recovers by re-polling the tracker
(SPEC §14.3: no retry timers, running sessions, or worker state survive restart). agentd's
constitution rejects that: *"state is inspectable and work is resumable — a crashed agent or
daemon must not lose or double-assign a ticket."* That promise requires durable, atomic claim
state.

We also want the git property that state is inspectable as text without a running service,
while keeping a bulletproof no-double-assign guarantee. Pure plain-text storage makes atomic
claiming and crash-safety a hand-rolled problem. A durable KV with ACID transactions makes it
free but is not `cat`-able. redb is single-process, so a second process cannot open the store
while the daemon holds it.

## Decision

We will split source-of-truth: **lazyspec is the truth for _what work exists_** (re-read each
tick, like the prior design re-polls Linear), and the **agentd store is the truth for _what is
claimed / running / attempted_** — the state that design discards and we keep.

The store is **redb** (pure-Rust, embedded, ACID), owned solely by the daemon. A claim is a
single ACID compare-and-set transaction on a `claims` table — claim iff absent or lease-expired
— making double-assign structurally impossible and crash-safe. Leases carry a TTL and are
renewed by a heartbeat while the worker is alive. Retry schedules are **re-derived on restart
from a persisted `due_at`**, not from serialized timer handles.

The daemon continuously **projects human-readable state to plain files**: an append-only
`.agentd/log` (tail-able, greppable) and a `.agentd/state.json` (jq-able), plus
`refs/claims/<iter-id>`-style pointers. This is git's design: a fast binary core behind a
plain-text surface. Reads for `status`/`log` go through the daemon socket when live
([[adr-001-language-and-topology]]); the text projection serves offline inspection.

## Consequences

- No-double-assign and resumability hold across daemon and agent crashes.
- ACID transactions replace hand-rolled locking; correctness is the store's job.
- redb's single-process constraint is why live CLI reads are daemon-mediated; the text
  projection is the offline/greppable surface, not the authority.
- On restart, the daemon reconciles redb claims against real worktrees + lazyspec state and
  releases expired leases with no live worker.
- Observability (SPEC §13) is largely satisfied by the projection; token/runtime totals
  aggregate in redb from canonical agent events (see [[adr-004-agent-abstraction]]).

