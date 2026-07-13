---
title: Claim an iteration exactly once (ACID)
type: story
status: in-progress
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
- blocks: STORY-014
- blocks: STORY-016
- blocks: STORY-018
- blocks: STORY-025
- targets: MILESTONE-001
---

As a developer, I want each iteration claimed at most once in a durable ACID store, so that two agents never work the same ticket and a claim is never lost on restart.

- Given an unclaimed id, When a claim is requested, Then a claims row is written in one ACID transaction and success returns the record.
- Given two concurrent claims for the same id, When both run, Then exactly one succeeds (CAS, no torn write).
- Given a committed claim, When the daemon is killed and restarted, Then reading the store returns the same claim.
