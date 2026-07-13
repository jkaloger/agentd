---
title: "Safety and workspace hardening"
type: milestone
status: planned
author: "Jack Kaloger"
date: 2026-07-13
tags: []
related: []
---

The safety and workspace edges: path/cwd invariants, lifecycle hooks (fatal and best-effort), branch-name derivation, worktree cleanup, plain-directory fallback for non-git projects, dispatch-scope filtering, and control-socket access restriction.

Releasable outcome: agentd is safe to point at a real repo under an autonomous posture, with the agent surface minimized and constrained.
