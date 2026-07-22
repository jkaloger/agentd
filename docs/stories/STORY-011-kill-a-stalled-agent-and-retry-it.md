---
title: Kill a stalled agent and retry it
type: story
status: review
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
- targets: MILESTONE-003
---

As an operator, I want agents that stop making progress killed and retried, so that a hung agent does not hold a slot forever.

- Given elapsed time since the last agent event (or started_at) exceeds stall_timeout_ms, When reconciliation runs, Then the worker is terminated and a retry queued.
- Given stall_timeout_ms <= 0, When reconciliation runs, Then stall detection is skipped.
- Given both stall detection and status refresh run, When a tick reconciles, Then stall detection runs first.
