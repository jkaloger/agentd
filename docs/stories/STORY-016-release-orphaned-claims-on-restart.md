---
title: Release orphaned claims on restart
type: story
status: in-progress
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
- blocks: STORY-017
- blocks: STORY-065
- targets: MILESTONE-002
---

As an operator, I want the daemon at startup to release claims whose lease expired with no live worker, so that after a crash the backlog is not blocked by ghost claims.

- Given a stored claim with expired lease and no reattachable worker, When startup reconcile runs, Then it is durably released and recorded.
- Given a still-valid lease, When the daemon starts, Then the claim is retained pending reattach or expiry.
- Given reconcile releases N claims, Then each release is projected so the operator sees it.
