---
title: Inspect effective config
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
---

As an operator, I want `agentd config` to print the fully resolved effective configuration, so that I can confirm what the daemon believes after defaults, resolution, and hot-reload.

- Given a running daemon, When I run `agentd config`, Then it prints the effective post-default post-resolution config, not raw file text.
- Given secrets resolved from \$VAR, When printed, Then they are redacted by default.
- Given --json, When I run it, Then output is machine-readable and matches the human view.
