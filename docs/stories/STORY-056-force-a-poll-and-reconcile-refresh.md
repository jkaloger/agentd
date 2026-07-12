---
title: Force a poll and reconcile (refresh)
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
---

As an operator, I want `agentd refresh` to trigger an immediate poll + reconcile, so that I do not wait for the next interval after editing the backlog.

- Given a running daemon, When I run `agentd refresh`, Then a poll+reconcile cycle is queued and the command confirms it (queued/coalesced).
- Given repeated rapid refresh calls, When they arrive, Then they may coalesce rather than stack.
- Given no running daemon, When I run `agentd refresh`, Then a clear not-running error.
