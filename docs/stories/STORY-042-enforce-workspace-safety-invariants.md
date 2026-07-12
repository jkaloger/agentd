---
title: Enforce workspace safety invariants
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-005
---

As an operator, I want the daemon to refuse launch unless the workspace path is sanitized, inside the root, and the agents cwd, so that a malformed iter-id can never target an arbitrary directory.

- Given an identifier, When the workspace key is derived, Then every character outside [A-Za-z0-9._-] is replaced with _.
- Given a computed path, When validated, Then it must be absolute with workspace.root as prefix; escaping paths are rejected.
- Given launch preflight, When it runs, Then it asserts cwd == workspace_path and aborts otherwise.
