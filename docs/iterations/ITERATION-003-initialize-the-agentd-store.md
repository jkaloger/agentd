---
title: Initialize the agentd store
type: iteration
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- implements: STORY-046
---

## Objective
`agentd init` scaffolds a `.agentd/` store beside `.lazyspec.toml` — a config and default prompt template with commented defaults — without clobbering existing files.

## Context
- Implements: STORY-046 (see its ACs for create/no-clobber/failure behaviour).
- Architecture: [[ADR-006]] — policy splits into `.agentd/config.toml` + separate prompt template file(s), sibling to `.lazyspec.toml`; `init` maps to `git init`.
- Touch: the `init` subcommand handler and an embedded default config + prompt template.

## Tasks
1. Create `.agentd/config.toml` with commented documented defaults and a default prompt template file per ADR-006.
2. Report the written paths; on re-run, detect existing files and emit a clear already-initialized message without overwriting (non-zero only when `--force` would be needed).
3. Handle a non-writable target directory: exit non-zero with an operator-visible reason.

## Acceptance Criteria
- STORY-046 AC1–AC3 hold: fresh init creates commented files and reports paths; re-run does not clobber and messages clearly; non-writable dir fails non-zero with a reason.

## Out of scope
- Config parsing/validation semantics (STORY-047's iteration); daemon startup.

