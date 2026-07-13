---
title: Serve the control-plane socket
type: story
status: draft
author: Jack Kaloger
date: 2026-07-13
tags: []
related:
- related-to: ADR-001
- blocks: STORY-052
- blocks: STORY-051
- blocks: STORY-053
- blocks: STORY-054
- blocks: STORY-055
- blocks: STORY-056
- blocks: STORY-057
- blocks: STORY-058
- blocks: STORY-063
- targets: MILESTONE-005
---

As an operator, I want the daemon to expose a unix-domain control socket with a request/response protocol, so that every CLI verb has a single authoritative channel to query and command the daemon. (Foundational transport behind STORY-051..058; the git-model control plane of ADR-001.)

- Given the daemon starts, When it finishes boot, Then it binds a unix socket at a known path and accepts framed request/response messages.
- Given a client sends a well-formed request, When the daemon handles it, Then it returns a typed response and the client exit code reflects success or error.
- Given a client sends a malformed or unknown request, When the daemon handles it, Then it returns a typed protocol error without crashing or mutating state.
- Given no daemon is running, When a client connects, Then the client fails fast with a clear unavailable error rather than hanging.
- Given the daemon stops, When it shuts down, Then it releases and unlinks the socket so a stale socket does not block the next start.
