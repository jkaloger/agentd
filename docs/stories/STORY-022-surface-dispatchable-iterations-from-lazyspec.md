---
title: Surface dispatchable iterations from lazyspec
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-003
- blocks: STORY-023
- blocks: STORY-025
- blocks: STORY-061
- blocks: STORY-060
- targets: MILESTONE-001
---

As an operator, I want the daemon to read dispatch-eligible iterations via `lazyspec status --json`, so that work marked ready is discovered automatically. (Read walking skeleton behind a Tracker trait.)

- Given a lazyspec repo, When candidates are fetched, Then it invokes `lazyspec status --json` (never reads graph files) and returns only accepted-state iterations.
- Given the CLI JSON, When normalized, Then each candidate exposes id, identifier, title, body, state, parent link, dependency refs.
- Given lazyspec exits non-zero or emits unparseable output, When a fetch runs, Then a typed error returns and the tick can be skipped.
