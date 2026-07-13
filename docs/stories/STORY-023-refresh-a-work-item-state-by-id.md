---
title: Refresh a work item state by id
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-003
- targets: MILESTONE-003
---

As an operator, I want the daemon to re-read a specific item state via `lazyspec show <id> --json`, so that in-flight work is stopped/released when its state changes.

- Given active ids, When reconciliation refreshes, Then it calls `lazyspec show <id> --json` per id and normalizes to the same model.
- Given an item now terminal, When refreshed, Then the record reports the terminal role.
- Given an id that no longer exists, When refreshed, Then a distinguishable not-found result returns (not a generic error).
