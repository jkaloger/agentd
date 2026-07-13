---
title: Parse, default, and validate config
type: story
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- blocks: STORY-048
- blocks: STORY-049
- blocks: STORY-051
- blocks: STORY-001
- targets: MILESTONE-001
---As an operator, I want the daemon to load .agentd/config.toml, apply defaults, and reject invalid values up front, so that I learn of a bad config at startup, not mid-run.

- Given a minimal config, When loaded, Then every optional field takes its documented default, including agent.max_turns.
- Given a missing required field or out-of-range typed value, When validated, Then startup fails with a field-scoped error naming the key.
- Given unknown top-level keys, When loaded, Then they are ignored for forward compatibility.
- Given an invalid per-state cap entry, When loaded, Then that entry is dropped and the rest apply.