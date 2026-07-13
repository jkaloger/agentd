---
title: Surface token usage and rate limits
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
- blocks: STORY-021
- targets: MILESTONE-004
---

As an operator, I want the adapter to extract token counts and the latest rate-limit snapshot from the stream, so that cost and throttling are visible and recorded.

- Given usage events, When parsed, Then input/output/total token counts attach to the canonical events usage map.
- Given absolute cumulative totals, When accumulating, Then deltas are tracked against last-reported to avoid double-counting.
- Given a rate-limit payload, When seen, Then it is emitted as the latest snapshot.
- Given no usage fields, When parsed, Then usage is absent/zero, not fabricated.
