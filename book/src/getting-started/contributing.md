# Contributing to FLUI

This page is for working on FLUI itself — its crates, its render pipeline, its CLI — not for
building an application with it (see [Installation and first run](installation.md) for that path
instead).

The authoritative contributor references live in the repository, not duplicated here, so they
stay in one place as they evolve:

- [`AGENTS.md`](https://github.com/vanyastaff/flui/blob/main/AGENTS.md) — the single agent/contributor
  guide: the Prime Directive, worktree workflow, commands, architecture constraints, and the
  Definition of Done.
- [`CONTRIBUTING.md`](https://github.com/vanyastaff/flui/blob/main/CONTRIBUTING.md) — planning a
  large change, git hygiene, reporting bugs, security.
- [`docs/testing.md`](https://github.com/vanyastaff/flui/blob/main/docs/testing.md) — the test
  pyramid, quality gates, and which `just` recipe to run for what you touched.

## Quick start

```bash
git clone https://github.com/vanyastaff/flui
cd flui
cargo build --workspace
```

Work happens in a dedicated `git worktree` per task, never the shared checkout — see AGENTS.md's
"How to Work" section for the exact command and the commit/PR conventions that go with it. The
local pre-review gate is:

```bash
just ci
```

## This book

This site is itself a FLUI-workspace artifact, under `book/`. See
[`.github/workflows/docs.yml`](https://github.com/vanyastaff/flui/blob/main/.github/workflows/docs.yml)
for how it builds and deploys, and AGENTS.md's documentation rules for what belongs in a page here
versus in the repository's own `docs/`.
