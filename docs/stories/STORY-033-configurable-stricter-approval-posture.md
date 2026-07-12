---
title: Configurable stricter approval posture
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
---

As an operator, I want to switch the adapter to a stricter non-autonomous mode via config, so that I can run where unattended execution is unacceptable.

- Given config selecting a stricter posture, When a session starts, Then Claude launches without blanket auto-approve.
- Given an approval request under stricter posture, When received, Then it is resolved per documented policy and never left blocking.
- Given an invalid posture value, When loaded, Then a config error is reported rather than silently defaulting to high-trust.
