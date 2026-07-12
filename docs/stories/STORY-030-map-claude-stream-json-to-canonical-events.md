---
title: Map Claude stream-json to canonical events
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-004
---

As an integrator, I want the Claude adapter to translate stream-json into the canonical AgentEvent model, so that the orchestrator only ever sees protocol-agnostic events.

- Given a stream-json message line, When parsed, Then it maps to notification/other_message with timestamp and pid.
- Given session start, When parsed, Then session_started is emitted with the session id.
- Given turn-failure/cancel lines, When parsed, Then turn_failed/turn_cancelled are emitted.
- Given invalid JSON or an unknown shape, When parsed, Then it emits malformed without crashing the stream.
