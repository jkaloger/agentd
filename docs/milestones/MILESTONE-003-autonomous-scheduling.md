---
title: "Autonomous scheduling"
type: milestone
status: active
author: "Jack Kaloger"
date: 2026-07-13
tags: []
related: []
---

The daemon runs unattended: fixed-interval poll, eligibility plus the dependency/parent blocker gate, priority ordering, global and per-status concurrency caps, exponential-backoff retry, continuation, and reconcile that stops agents whose item went terminal or stalled.

Releasable outcome: leave the daemon running against a live backlog and it dispatches, retries, and cleans up correctly with no operator action.
