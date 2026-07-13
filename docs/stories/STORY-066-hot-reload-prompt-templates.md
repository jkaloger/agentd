---
title: Hot-reload prompt templates
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- targets: MILESTONE-005
---

As an operator, I want edits to prompt template files to take effect without restarting the daemon, so that I can retune the agent prompt live. (Split from STORY-050: the prompt half.)

- Given the daemon is running, When I edit a prompt template file, Then the change is picked up and applied to future dispatches without a restart.
- Given a template edit renders invalid, When it is reloaded, Then the last-known-good template is kept and an operator error is surfaced.
- Given an in-flight run is using the prior template, When a reload occurs, Then that run is untouched and only future dispatches use the new template.
- Given a watch event is missed, When the next dispatch renders, Then the template is re-validated defensively rather than trusting stale state.
