---
title: Stop and drain the daemon
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- targets: MILESTONE-005
---

As an operator, I want `agentd stop` to shut the daemon down cleanly, draining or releasing in-flight work, so that I can stop it like a git-like tool without orphaning claims. (Split from STORY-052: the stop half.)

- Given the daemon is running with active workers, When I run `agentd stop`, Then it stops accepting new dispatch, allows a grace period, and releases leases for work still running at the deadline.
- Given the grace period elapses, When workers have not exited, Then they are force-terminated and their claims released for retry.
- Given no daemon is running, When I run `agentd stop`, Then it reports that cleanly with a non-error exit.
- Given stop completes, When it returns, Then the control socket is released and the store reflects no live workers.
