---
title: Inspect one iteration (show)
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- blocks: STORY-059
- targets: MILESTONE-005
---As an operator, I want `agentd show <iter-id>` to print full per-iteration detail, so that I can debug a single ticket. (Machine-readable output is the cross-cutting contract in STORY-059.)

- Given a known iteration, When I run `agentd show <iter-id>`, Then output includes status, workspace path and branch, attempts, running session detail, retry info, recent events, and last error.
- Given an unknown iteration, When I run it, Then an issue-not-found error and non-zero exit.