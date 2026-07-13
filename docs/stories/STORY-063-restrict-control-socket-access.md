---
title: Restrict control-socket access
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
- targets: MILESTONE-005
---

As an operator, I want the control socket restricted to the owning user, so that the sole state-mutation channel cannot be driven by other local users. (Security posture on the ADR-001 control plane.)

- Given the daemon binds its socket, When it creates the socket file, Then the socket permissions restrict access to the owning user.
- Given a process without permission connects, When it attempts a request, Then the connection is refused at the OS boundary before any command is processed.
- Given a stricter access policy is configured, When the daemon binds, Then the configured policy is applied and an invalid policy fails startup with an operator-visible error.
