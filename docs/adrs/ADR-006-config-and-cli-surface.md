---
title: Config and CLI surface
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

SPEC §5, §6.2 puts all runtime config plus the per-issue prompt in one repo-owned `WORKFLOW.md`
(YAML front matter + Liquid body) with hot-reload. agentd must configure poll
interval, concurrency, agent kind + settings, workspace/isolation, hooks, the config-driven
dispatch type/state mapping ([[adr-003-work-source-lazyspec]]), lifecycle transition mapping,
and a per-ticket prompt.

In agentd the iteration doc *is* the plan (self-contained by lazyspec's design), so the
per-ticket prompt is thin — inject the rendered iteration + parent story/ADR context and say
"execute it" — with rich content pulled via `lazyspec show <id> --json`. Config therefore
matters more than the prompt, and TOML fits the Rust ecosystem and sits beside `.lazyspec.toml`.

## Decision

We will **split policy into `.agentd/config.toml` + separate prompt template file(s)**, both
**hot-reloaded** (SPEC §6.2: re-read and re-apply live; applies to future dispatch/retry/launch,
in-flight runs untouched; invalid reload keeps last-known-good and surfaces an operator error).
Config is a sibling to `.lazyspec.toml`; prompts are strict-rendered templates (unknown
variable/filter fails rendering, SPEC §5.4).

The CLI is **full git-like porcelain**, reusing git's words where they exist:

    agentd init                 create .agentd/ store            (git init)
    agentd start | stop         daemon lifecycle
    agentd status               running sessions, retry queue    (git status / §13.3)
    agentd log [<iter-id>]      append-only event stream         (git log / reflog)
    agentd show <iter-id>       per-ticket detail                (§13.7 /api/v1/<id>)
    agentd config               effective config after reload
    agentd refresh              force a poll + reconcile tick     (§13.7 /refresh)
    agentd assign <iter-id>     manually dispatch a ticket
    agentd cancel <iter-id>     stop a running agent

Manual `assign` / `cancel` **route through the daemon**, which remains the sole leasing
authority — `assign` is a prioritized request, not a second claimant — so the "daemon assigns"
tenet holds.

## Consequences

- Clean config/prompt separation; TOML matches the ecosystem and the lazyspec sibling file.
- Hot-reload lets operators retune concurrency/interval/prompt without restart.
- Two watched inputs (config + prompt) instead of one combined file.
- Porcelain gives humans and scripts the same interface; manual override is operator-friendly
  without violating leasing, which stays owned by the store in
  [[adr-002-durable-state-and-leasing]].

