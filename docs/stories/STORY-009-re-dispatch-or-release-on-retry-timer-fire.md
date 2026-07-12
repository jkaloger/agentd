---
title: Re-dispatch or release on retry-timer fire
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
---

As a backlog owner, I want a fired retry to re-check the backlog before acting, so that stale or completed work is not relaunched.

- Given a retry fires and the item is absent from candidates, When handled, Then the claim is released.
- Given the item is present and eligible with a slot, When the timer fires, Then it dispatches carrying its attempt number.
- Given the item is eligible but no slot, When the timer fires, Then it requeues with error "no available orchestrator slots".
- Given the candidate re-fetch fails, When the timer fires, Then it requeues rather than releasing.
