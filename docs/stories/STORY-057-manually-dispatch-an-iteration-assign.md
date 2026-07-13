---
title: Manually dispatch an iteration (assign)
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- targets: MILESTONE-005
---

As an operator, I want `agentd assign <iter-id>` to prioritize dispatching a specific iteration through the daemon, so that I can jump a ticket ahead without becoming a second claimant.

- Given a running daemon and an eligible iteration, When I run `agentd assign <iter-id>`, Then the request routes to the daemon which prioritizes it; the daemon stays the sole leasing authority.
- Given the iteration is already running/claimed, When I assign it, Then the daemon reports current state and creates no duplicate claim.
- Given no free slot, When I assign it, Then it queues with a clear reason rather than bypassing limits.
- Given an ineligible/unknown iter-id, When I assign it, Then a clear rejection and non-zero exit.
