---
title: Aggregate token and runtime accounting
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
---

As an operator, I want per-iteration and aggregate token/runtime totals accumulated durably from canonical agent events, so that cost and runtime survive restarts and appear in status.

- Given an event with absolute thread token totals, When ingested, Then cumulative tokens update by delta-vs-last-reported (delta-style payloads ignored).
- Given a session ends, When observed, Then its run-duration seconds add to cumulative runtime.
- Given a restart, When totals reload, Then prior totals persist and keep accumulating.
