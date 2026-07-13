---
title: Hot-reload config and prompts
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- targets: MILESTONE-005
---As an operator, I want edits to config.toml to take effect without a restart, so that I can retune concurrency, interval, and dispatch policy live. (Split: prompt-template reload is STORY-066.)

- Given a running daemon and a valid config edit, When detected, Then future dispatch/retry/launch/hook decisions use new values while in-flight sessions run unchanged.
- Given an invalid config save, When reload runs, Then the daemon keeps last-known-good and emits an operator-visible error without crashing.
- Given a missed watch event, When the next dispatch preflight runs, Then it re-validates defensively and converges to on-disk state.