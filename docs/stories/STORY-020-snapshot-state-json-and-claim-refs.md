---
title: Snapshot state.json and claim refs
type: story
status: accepted
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-002
- blocks: STORY-053
- targets: MILESTONE-002
---

As an operator, I want current claim/lease state projected to .agentd/state.json and refs/claims/<iter-id>, so that I can jq the live picture and inspect per-iteration status offline, git-style.

- Given the store changes, When projected, Then state.json is rewritten atomically (temp+rename) and is always valid JSON.
- Given an active claim, Then refs/claims/<iter-id> points at the holder/fence; When released, the ref is removed.
- Given a projection write failure, When it happens, Then the store stays authoritative and the daemon warns without crashing.
