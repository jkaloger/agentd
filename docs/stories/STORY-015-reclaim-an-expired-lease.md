---
title: Reclaim an expired lease
type: story
status: in-progress
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
- targets: MILESTONE-002
---

As a developer, I want an iteration whose lease expired to become claimable again, so that a crashed worker resumes instead of stalling forever.

- Given an expired lease, When a new claim is requested, Then the CAS succeeds, replaces the holder, and issues a fresh lease.
- Given a still-valid lease, When a new claim is requested, Then it is rejected.
- Given a reclaim, Then a monotonic fence increases so late heartbeats from the prior holder are rejected.
