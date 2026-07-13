---
title: Restrict dispatch scope by filter
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
- targets: MILESTONE-006
---

As an operator, I want to restrict which iterations are eligible for dispatch via a configured scope filter (for example label or tag), so that I can minimize the surface an autonomous agent is turned loose on. (Harness hardening, SPEC §15.5.)

- Given a scope filter is configured, When the daemon evaluates candidates, Then only iterations matching the filter are dispatch-eligible; non-matching items are skipped with a recorded reason.
- Given no scope filter is configured, When evaluation runs, Then all otherwise-eligible iterations remain dispatchable (default open).
- Given an invalid filter expression, When config loads, Then it is rejected at startup rather than silently matching everything.
- Given a manual assign targets an item outside the scope filter, When it is requested, Then the operator override is honored and the bypass is recorded.
