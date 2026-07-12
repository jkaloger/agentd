---
title: Resolve $VAR and ~ in config values
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
---

As an operator, I want config values referencing \$VAR or ~ resolved only where the field intends, so that I keep secrets and machine paths out of the checked-in file.

- Given workspace.root = ~/agentd-ws, When resolved, Then ~ expands and normalizes to an absolute path.
- Given a field set to \$VAR, When the env var is set, Then it substitutes; when empty, it is treated as missing.
- Given a relative workspace.root, When resolved, Then it anchors to the config.toml directory, not the process cwd.
- Given a URI or arbitrary shell string, When resolved, Then \$VAR/~ expansion is NOT applied.
