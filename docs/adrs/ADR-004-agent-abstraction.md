---
title: Agent abstraction
type: adr
status: draft
author: Jack Kaloger
date: 2026-07-12
tags: []
related:
- related-to: ADR-003
- related-to: ADR-002
---

## Context

Symphony hardwires the Codex app-server protocol (thread/turn, streaming JSON over stdio,
`session_id = <thread_id>-<turn_id>`, token-usage events — SPEC §10). agentd must target
Claude Code (ClaudeP) first but stay extensible and DRY across codex, opencode, pi, which
differ wildly in shape: Claude Code is headless `claude -p` with resume; codex is a long-lived
app-server; opencode runs a server with an HTTP/SSE API.

The DRY win is a single canonical event model (SPEC §10.4 already enumerates the events) that
every adapter maps its native protocol onto, so the orchestrator only ever sees canonical
events. The seam can be an in-process Rust trait, an external shim protocol, or pure config
templates — the last breaks on stateful protocols.

## Decision

We will define an in-process **`AgentAdapter` trait** (`start_session` / `run_turn →
stream<AgentEvent>` / `stop`) over a **canonical `AgentEvent` model**. Adapter *selection* is
config-driven (`agent.kind`); adapter *code* is in-tree. A **generic-subprocess adapter** is
the escape hatch for simple config-declared targets, recovering most template flexibility
without the fragility. External shim binaries are a deferred option, not built now.

Adapter order: **claude → codex → opencode**.

The **claude adapter drives one-shot turns**: a fresh `claude -p --resume <session_id>` per
turn, process exits after; continuation is re-invocation with the same session id (Claude
loops to the goal internally, so Symphony's multi-turn app-server loop largely collapses to
one agentic run per iteration, with re-dispatch only if the doc is still active). The TS/Python
Agent SDK is rejected — it would break the single-binary story.

Default posture is **high-trust / non-interactive**: auto-approve commands and edits, and treat
`input-required` as failure rather than blocking (SPEC §10.5 high-trust example). The user need
not interact with the agent. This posture MUST stay **configurable** — a stricter
operator-approval mode is possible even though the default is autonomous.

## Consequences

- New agents plug in behind one trait + one event model; the orchestrator is protocol-agnostic.
- codex reuses Symphony's design almost directly; opencode wraps its server API.
- One-shot + resume gives the simplest, most crash-robust worker lifecycle; the trait still
  abstracts lifecycle so codex can stay persistent.
- **Security:** autonomous auto-approve lets the agent run arbitrary commands unattended. This
  is a deliberate, documented trust posture (SPEC §15). Isolation is delegated to the agent's
  own sandbox, the host, and the workspace-path invariants of [[adr-005-workspace-isolation]];
  a stricter posture remains configurable.
- The adapter's clean-exit vs failure signal feeds the daemon's lifecycle transition
  ([[adr-003-work-source-lazyspec]]); token/runtime usage feeds the store
  ([[adr-002-durable-state-and-leasing]]).

