---
title: Machine-readable output for scripts
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
---

As automation, I want every read/query verb to offer machine-readable output whose values match the human view, so that I can script agentd without scraping text.

- Given status/show/config/log with --json, When run, Then output is stable structured JSON and the human view carries the same values.
- Given a command fails with --json, When run, Then errors are structured (code + message) and the exit code reflects success/failure.
- Given a schema-affecting change, When output is versioned, Then a version marker is present.
