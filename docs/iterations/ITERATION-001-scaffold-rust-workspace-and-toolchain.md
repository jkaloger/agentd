---
title: Scaffold Rust workspace and toolchain
type: iteration
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
  - implements: STORY-001
---

## Objective

Scaffold the Rust workspace, toolchain, and CI so the crate compiles and lints clean — the home `agentd start` will live in.

## Context

- Implements: STORY-001 (boot needs a compiling crate).
- Architecture: [[ADR-001]] — single static binary, CLI + daemon in one binary, concurrency via tokio; porcelain surface enumerated in [[ADR-006]].
- No code exists yet. This iteration establishes the crate skeleton only; boot logic is a separate slice.
- Touch: `Cargo.toml`, `src/main.rs`, `rustfmt.toml`, `.github/workflows/ci.yml`.
- use the installing dependencies skill

## Tasks

1. `cargo init --bin` the `agentd` crate; pin edition and rust-version.
2. Add core deps per ADR-001: `tokio` (rt-multi-thread, process, net, macros), `clap` (derive), `serde`/`toml`.
3. Add `rustfmt.toml` and a `[lints]` table denying clippy warnings (`clippy::all`, `-D warnings` in CI).
4. Stub the clap CLI with the [[ADR-006]] porcelain subcommands (`init/start/stop/status/log/show/config/refresh/assign/cancel`) as no-op handlers returning "unimplemented".
5. Add CI workflow running `cargo fmt --check`, `cargo clippy -D warnings`, `cargo build`, `cargo test`.

## Acceptance Criteria

- `cargo build` succeeds; `cargo fmt --check` and `cargo clippy` pass with warnings denied.
- `agentd --help` lists the ADR-006 subcommands.
- CI workflow runs green on the four gates.

## Out of scope

- Any daemon/boot behaviour (deferred to the boot iteration) and all store/adapter/workspace logic.
