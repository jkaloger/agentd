---
title: Control the daemon lifecycle
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- blocks: STORY-067
- targets: MILESTONE-005
---As an operator, I want `agentd start` to launch the daemon and report it running, so that I can bring agentd up like a git-like tool. (Split: shutdown/drain is STORY-067.)

- Given an initialized store and valid config, When I run `agentd start`, Then the daemon validates config, binds its socket, begins operating, and reports running (foreground or detached per flag).
- Given config validation fails, When I run `agentd start`, Then it aborts non-zero with an operator-visible error and leaves no socket bound.
- Given a daemon is already running, When I run `agentd start`, Then it refuses to start a second instance and reports the existing one.