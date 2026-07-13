---
title: Workspace isolation
type: adr
status: draft
author: Jack Kaloger
date: 2026-07-12
tags: []
related:
- related-to: ADR-004
---

## Context

SPEC §9 gives each issue a plain per-issue directory under a workspace root, reused across
runs, populated via hooks. agentd is git-like, and lazyspec iterations map naturally
onto a branch → PR flow. Options: git worktree per ticket, full clone per ticket, or that
plain-dir approach.

## Decision

We will isolate each ticket in a **git worktree**: `git worktree add <root>/<iter-id> -b
agentd/<iter-id>` (or lazyspec's `branch_name` metadata when present — SPEC §4.1.1). agentd
operates on the current repo; worktrees branch off it and share the object store, so creation
is near-instant with no redundant clone, and each agent gets an isolated working tree on its
own branch feeding a clean PR-per-iteration.

SPEC §9.5's safety invariants still hold: agent cwd is the worktree, the path must
stay under the workspace root, and the workspace key is sanitized. The `before_run` hook
remains available for dependency bootstrap. A **plain-dir mode is a config fallback** for
non-git projects.

## Consequences

- On-brand git-like isolation; branch-per-iteration integrates with the PR workflow.
- Requires the target project to be a git repo (fallback covers the rest).
- Worktree lifecycle (`git worktree add` / `remove`) replaces the dir create/cleanup above;
  terminal-issue cleanup (SPEC §8.6) removes the worktree.
- Path-boundary and sanitization checks matter more if the SSH worker extension
  (SPEC Appendix A) is later adopted, since worktrees are host-local.

