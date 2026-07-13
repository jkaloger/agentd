---
title: Language and topology
type: adr
status: draft
author: Jack Kaloger
date: 2026-07-12
tags: []
related:
- related-to: ADR-002
---## Context

agentd is a unix, git-like daemon that orchestrates coding agents against a lazyspec
backlog. It is modelled on a prior orchestration design (`SPEC.md`) but must feel like a
first-class CLI: a human at a terminal and a script get the same interface, and `status` /
`log` behave like their git counterparts.

Two coupled foundational choices gate everything else: the implementation language and the
CLI↔daemon topology. Candidates considered: Rust, Go, Elixir/OTP. Elixir's supervision tree
maps cleanly onto the orchestrator/worker/retry model in `SPEC.md` and would nearly give the SSH
worker extension (SPEC Appendix A) for free, but it has no true static binary and BEAM boot
latency undermines a snappy git-like CLI. Go ships a daemon fastest but has the weakest types
for proving the leasing/state-machine invariants. Rust gives a single static binary,
compile-time modelling of the orchestration state machine (illegal states unrepresentable),
and clean long-lived-subprocess handling, at the cost of hand-building supervision.

## Decision

We will build agentd in **Rust** as a **single static binary** that is **CLI-first**: the same
binary is both the daemon and the porcelain. The daemon owns the authoritative state; the CLI
is the primary interface to it.

The topology is the **git model**: a fast transactional core owned by the daemon, a plain-text
surface for inspection, and a unix domain socket as the control plane. State mutation and live
queries route through the daemon over the socket; offline inspection reads the projected text
(see [[adr-002-durable-state-and-leasing]]). Concurrency uses tokio; per-ticket workers are
supervised by the orchestrator task.

The **unix socket is the only control-plane surface**: SPEC §13.7's optional HTTP server /
dashboard is **declined**. Programmatic access is served by machine-readable CLI output over
the socket (SPEC §13.7 `/api` parity is met by `agentd show --json` etc.), keeping a single
authenticated, filesystem-permissioned channel rather than a second network listener.

## Consequences

- Single artifact, instant CLI, strong compile-time guarantees on the lease/state machine.
- We hand-build supervision/retry that OTP would have given us; the orchestrator is the single
  authority that serializes all state mutations (SPEC §7).
- Long-lived agent subprocesses are handled directly rather than through a Port abstraction.
- The socket control plane is idiomatic (`/var/run`-style), but live state requires the daemon;
  offline inspectability is delivered by the text projection in [[adr-002-durable-state-and-leasing]].
- No HTTP surface to secure or operate; access control reduces to socket file permissions, and
  scripting goes through the same porcelain a human uses.
- Diverges from `SPEC.md`'s language-agnostic framing by committing to Rust; the abstraction
  layers of SPEC §3.2 are preserved as Rust module boundaries.