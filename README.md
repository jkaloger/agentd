<h1 align="center">
  🤖
  <br>agentd
</h1>
<p align="center">
    A daemon that assigns LLM coding agents to open tickets.
</p>

agentd watches a [lazyspec](https://github.com/jkaloger/lazyspec) backlog and dispatches coding agents to execute the work.

- **Automatic dispatch** — polls lazyspec for dispatchable iterations and claims them as agent capacity allows.
- **Isolated workspaces** — each ticket runs in its own git worktree (or plain directory for non-git projects).
- **Lifecycle driven** — claims, completes, and rejects tickets via `lazyspec advance`, following the lifecycle configured in `.lazyspec.toml`.
- **Recoverable** — failed runs retry with exponential backoff; stalled agents are killed and re-queued; crashed runs don't lose or double-assign tickets.
- **Observable** — an append-only event log, live session status, and per-ticket history over a local unix socket.
- **Hot-reloaded config** — edits to `.agentd/config.toml` apply to future decisions without restarting.

## How?

Requirements: Rust 1.93+, the `lazyspec` CLI, and an agent CLI (`claude`).

```sh
cargo install --path .

cd your-lazyspec-project
agentd init        # creates .agentd/ with config.toml and prompt.liquid
```

Tune `.agentd/config.toml` — concurrency (`max_concurrent`), poll interval, stall timeout, workspace mode, and which document types/states are dispatchable. Every default is documented inline. The agent prompt is a Liquid template at `.agentd/prompt.liquid`.

## Usage

```sh
agentd start              # run the daemon
agentd status             # running sessions and the retry queue
agentd log [ITER-001]     # append-only event stream, optionally for one ticket
agentd show ITER-001      # per-ticket detail
agentd assign ITER-001    # manually dispatch a ticket
agentd cancel ITER-001    # stop a running agent
agentd refresh            # force a poll and reconcile tick
agentd config             # print the effective config
agentd stop               # stop the daemon
```
