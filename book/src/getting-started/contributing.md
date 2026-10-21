# Contributing to FLUI

This page is for working on FLUI itself — its crates, its render pipeline, its CLI — not for
building an application with it (see [Installation and first run](installation.md) for that path
instead).

The authoritative contributor references live in the repository, not duplicated here, so they
stay in one place as they evolve:

- [`AGENTS.md`](https://github.com/vanyastaff/flui/blob/main/AGENTS.md) — the single agent/contributor
  guide: the design stance, worktree workflow, commands, enforced rules, and the
  Definition of Done.
- [`CONTRIBUTING.md`](https://github.com/vanyastaff/flui/blob/main/CONTRIBUTING.md) — planning a
  large change, git hygiene, reporting bugs, security.
- [`docs/testing.md`](https://github.com/vanyastaff/flui/blob/main/docs/testing.md) — the test
  pyramid, quality gates, and which `cargo xtask` command to run for what you touched.

## Quick start

```bash
git clone https://github.com/vanyastaff/flui
cd flui
cargo build --workspace
```

Work happens in a dedicated `git worktree` per task (`cargo xtask worktree new <area>/<slug>`),
never the shared checkout; AGENTS.md's "Working here" has the commit conventions. The
local pre-PR checks run one at a time on the crates you changed (AGENTS.md, "Checking a change"):

```bash
cargo fmt --all
cargo clippy -p <crate> --all-targets -- -D warnings
cargo nextest run -p <crate>
```

## This book

This site is itself a FLUI-workspace artifact, under `book/`. See
[`.github/workflows/docs.yml`](https://github.com/vanyastaff/flui/blob/main/.github/workflows/docs.yml)
for how it builds and deploys. A page here is a guide for readers; the design records and
reference material stay in the repository's own `docs/`, and the book links to them rather than
copying them.
