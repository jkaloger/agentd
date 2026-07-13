---
title: Initialize an agentd store
type: story
status: complete
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- blocks: STORY-047
- targets: MILESTONE-001
---

As an operator, I want `agentd init` to scaffold a .agentd/ store beside my .lazyspec.toml, so that I have a versionable home for policy before running anything.

- Given no .agentd/, When I run `agentd init`, Then .agentd/config.toml and a default prompt template are created with commented defaults and the written paths reported.
- Given .agentd/ exists, When I run `agentd init`, Then existing files are not clobbered and I get a clear already-initialized message (non-zero only if --force needed).
- Given init runs in a non-writable directory, When it fails, Then it exits non-zero with an operator-visible reason.
