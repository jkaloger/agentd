---
title: Config-driven type and state-role mapping
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-003
---

As an operator, I want to declare in config which lazyspec type(s) are dispatchable and how states map to dispatch/active/terminal roles, so that agentd works with my document shape, not just the iteration default.

- Given a config mapping, When the daemon starts, Then candidate fetch and classification use it instead of defaults.
- Given no mapping, When the daemon starts, Then it falls back to iteration/accepted/in-progress/complete.
- Given a role naming a state the DAG lacks, When validated, Then startup fails naming the offending mapping.
