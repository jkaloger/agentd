---
title: Hold a lease with TTL and heartbeat
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
---

As an operator, I want a running claim to carry a TTL lease the live worker renews by heartbeat, so that a healthy worker keeps its claim while a dead worker becomes reclaimable.

- Given a successful claim, Then the record carries a lease expiry = now + TTL.
- Given a live worker heartbeats before expiry, When processed, Then expiry extends by TTL in a durable transaction.
- Given a heartbeat whose holder/fence mismatches, When processed, Then it is rejected.
- Given no heartbeat before expiry, When the clock passes it, Then the lease is observably expired (row not deleted).
