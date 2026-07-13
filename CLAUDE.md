# Working in this repository

This project is **spec-driven**. No work happens without a document behind it. We use [lazyspec](https://github.com/jkaloger/lazyspec) to manage those documents; the type definitions and lifecycles live in `.lazyspec.toml`.

## Philosophy

Decisions come before code, and every piece of work traces back to a decision.

- **ADRs** capture architecture decisions — the context, the choice, and its consequences. They are the source of truth for _how_ the system is built and _why_. ADRs inform the stories and bugs that follow.
- **Stories** capture units of user-facing value; **bugs** report observed defects. Both are top-level work items — a story is written, a bug is reported. They describe _what_ needs to change, shaped by the ADRs.
- **Iterations** break that work into bounded, self-contained dev plans a coding agent can execute in one session. Each iteration `implements` exactly one story or bug.
- **Conventions** and **principles** record how we do things here — durable guidance that outlives any single feature.

```
ADR ──informs──▶ Story / Bug ──broken into──▶ Iteration ──executed──▶ code
```

## Rules

- **No work without a plan.** Before writing code, there must be an iteration describing it, linked to a story or bug.
- **Iterations must link to their work item.** Every iteration `implements` a story or a bug — `validate` enforces this.
- **ADRs must link somewhere.** An architecture decision that connects to nothing is a smell; `validate` enforces a relation.
- **Follow the lifecycle.** Status transitions are gated by each type's DAG in `.lazyspec.toml`. Advance documents through their states; don't skip.
- **Keep documents to their intent.** Each type has a one-line intent and per-section guidance in `.lazyspec/templates/{type}.md`. Write to it; don't restate what a linked document already says.

## The loop

1. An architecture decision is recorded as an **ADR**.
2. Work is written as a **story** or reported as a **bug**, informed by the ADRs.
3. The work is broken into **iterations** — the dev plans agents execute.
4. Iterations are executed, reviewed, and advanced to `complete`.

Use `lazyspec` (or the `/lazy` router skill) to create, link, and advance documents.

## git

no need for branches. commit to main. wait for me to push.
