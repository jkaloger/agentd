---
title: Bound turns with timeout and map errors
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
- targets: MILESTONE-004
---

As an operator, I want long-running or process-level failures terminated and classified, so that the daemon gets a precise failure reason for retry and logs.

- Given a turn exceeding turn_timeout_ms, When the deadline passes, Then the process is killed and failure reported as turn_timeout.
- Given an unexpected mid-stream subprocess exit, When detected, Then failure is reported as port_exit.
- Given a protocol/response error, When encountered, Then it maps to response_error/turn_failed.
- Given any terminal reason, When reported, Then the signal distinguishes clean exit from failure with the normalized category.
