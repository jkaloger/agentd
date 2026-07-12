---
title: Control the daemon lifecycle
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
---

As an operator, I want `agentd start` and `agentd stop` to run and cleanly shut down the daemon, so that I manage it like a git-like tool.

- Given an initialized store, When I run `agentd start`, Then the daemon validates config, begins operating, and reports running (foreground or detached per flag).
- Given a running daemon, When I run `agentd stop`, Then it drains, records state, signals workers, and exits once torn down; force-kills after a grace period.
- Given no daemon running, When I run `agentd stop`, Then I get a clear not-running message and appropriate exit code.
- Given shutdown completes, Then the unix socket file is released.
