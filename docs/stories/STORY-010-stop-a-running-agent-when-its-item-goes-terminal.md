---
title: Stop a running agent when its item goes terminal
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
---

As a backlog owner, I want the daemon to stop an agent whose item was closed under it, so that no effort is wasted on abandoned work.

- Given a running item now terminal, When reconciliation runs, Then the worker is terminated and its workspace cleaned.
- Given a running item still active, When refreshed, Then the snapshot updates and the worker keeps running.
- Given a running item neither active nor terminal, When refreshed, Then the worker is terminated without workspace cleanup.
- Given state refresh fails, When reconciliation runs, Then workers keep running and it retries next tick.
