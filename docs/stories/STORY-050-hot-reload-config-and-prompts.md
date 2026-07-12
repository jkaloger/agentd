---
title: Hot-reload config and prompts
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
---

As an operator, I want edits to config.toml and prompt files to take effect without a restart, so that I can retune concurrency, interval, and prompts live. (Flagged: split config/prompt/last-known-good.)

- Given a running daemon and a valid config edit, When detected, Then future dispatch/retry/launch/hook decisions use new values while in-flight sessions run unchanged.
- Given a prompt edit, When detected, Then future runs render the new template; in-flight runs untouched.
- Given an invalid config/template save, When reload runs, Then the daemon keeps last-known-good and emits an operator-visible error without crashing.
- Given a missed watch event, When the next dispatch preflight runs, Then it re-validates defensively and converges to on-disk state.
