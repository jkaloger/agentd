---
title: Cap concurrent agents per status
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
---

As an operator, I want per-status concurrency caps, so that I can limit agents in an expensive phase independently of the global cap.

- Given a configured per-status cap M with M running in that status, When more of that status are evaluated, Then they do not dispatch even if global slots remain.
- Given a status with no configured cap, When evaluated, Then it falls back to the global limit.
- Given a status key differing only by case, When looked up, Then it matches after lowercase normalization.
