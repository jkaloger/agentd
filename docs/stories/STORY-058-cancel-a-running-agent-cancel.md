---
title: Cancel a running agent (cancel)
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- targets: MILESTONE-005
---

As an operator, I want `agentd cancel <iter-id>` to stop a running agent via the daemon, so that I can pull the plug on misbehaving work.

- Given a running iteration, When I run `agentd cancel <iter-id>`, Then the daemon terminates the agent, runs after_run semantics, and releases the claim.
- Given cancellation, When complete, Then the worktree is preserved unless the iteration is terminal.
- Given the iteration is not running, When I cancel it, Then a clear no-op message.
- Given cancel is issued, When it routes, Then it goes through the daemon (not a direct kill), preserving the leasing tenet.
