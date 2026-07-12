---
title: Poll the backlog on a cadence
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
---

As a backlog owner, I want the daemon to re-poll on a fixed interval, so that work added later is picked up without a restart.

- Given the first tick completed, When `polling.interval_ms` elapses, Then another tick runs.
- Given a tick is in progress, When the interval fires, Then ticks do not overlap (next scheduled only on completion).
- Given the interval changes via reload, When the current tick completes, Then the next uses the new interval.
