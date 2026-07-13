---
title: Render prompt templates strictly
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-006
- blocks: STORY-066
- targets: MILESTONE-005
---

As an operator, I want prompt templates rendered strictly against iteration context, so that a variable typo fails loudly instead of shipping a broken prompt.

- Given a known variable, When rendered, Then the value is interpolated.
- Given an unknown variable or filter, When rendering runs, Then it fails with template_render_error and the affected attempt fails (others unaffected).
- Given a missing/malformed template file, When loaded, Then it is a config error (no silent default-prompt fallback).
- Given a syntactically invalid template, When parsed, Then template_parse_error surfaces.
