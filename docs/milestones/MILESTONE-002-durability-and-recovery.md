---
title: "Durability and recovery"
type: milestone
status: complete
author: "Jack Kaloger"
date: 2026-07-13
tags: []
related: []
---

The store becomes crash-safe: durable ACID claims, TTL leases with heartbeat, reclaim of expired leases, restart reconcile against lazyspec and on-disk worktrees, and the plain-text projections. No-double-assign and resumability hold across daemon and agent crashes.

Releasable outcome: kill the daemon mid-run and restart — no claim is lost, duplicated, or left orphaned.
