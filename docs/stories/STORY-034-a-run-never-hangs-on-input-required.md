---
title: A run never hangs on input-required
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
---

As an operator, I want any user-input-required signal to end the run instead of blocking, so that a stuck agent frees its slot.

- Given default posture, When Claude emits input-required, Then turn_input_required is emitted, mapped to failure, and the run reported failed.
- Given a run awaiting input with none available, When it occurs, Then the run terminates within bounded time.
- Given stricter posture with an operator channel, When input-required occurs, Then it is handled per that policy rather than hard-failed.
