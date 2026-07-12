---
title: Autonomous auto-approve by default
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
---

As an operator, I want the Claude adapter to run fully non-interactively, auto-approving command and file-change requests, so that work completes unattended.

- Given default config, When a session starts, Then Claude launches non-interactively, auto-approving command execution.
- Given an edit/file-change approval, When it would be requested, Then it is auto-approved and approval_auto_approved emitted.
- Given an unsupported dynamic tool call, When received, Then the adapter returns a tool-failure and the session continues.
