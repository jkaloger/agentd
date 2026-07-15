---
title: Exponential backoff retry on failure
type: story
status: review
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
- blocks: STORY-009
- blocks: STORY-018
- targets: MILESTONE-003
---

As an operator, I want failed items to retry with increasing delays, so that transient failures recover without hammering a broken dependency.

- Given an abnormal worker exit, When handled, Then a retry is scheduled with delay = min(10000 * 2^(attempt-1), max_retry_backoff_ms).
- Given an existing retry timer for the item, When a new retry is scheduled, Then the prior timer is cancelled first.
- Given a scheduled retry, When I query status, Then it shows in the retry queue with attempt and error.
