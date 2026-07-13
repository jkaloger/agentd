---
title: Read the event stream (log)
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- blocks: STORY-059
- targets: MILESTONE-005
---

As an operator, I want `agentd log [<iter-id>]` to print the append-only event stream, so that I can audit what happened (git log/reflog analog).

- Given events exist, When I run `agentd log`, Then events print with stable key=value fields including iter-id and outcome.
- Given `agentd log <iter-id>`, When I run it, Then only that iterations events show.
- Given --follow, When I run it, Then new events stream until interrupted.
- Given an unknown iter-id, When I run it, Then a clear not-found message and non-zero exit.
