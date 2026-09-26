# Contributing to FLUI

Welcome! The rules for a change live in two places:

- [`AGENTS.md`](AGENTS.md) — the codebase map, how to work (worktrees, commits, PRs, what not
  to touch), the local gate commands, the architecture-constraint table, and the Definition of
  Done. Written for humans and agents alike; `CLAUDE.md` imports it.
- [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md) — the architecture contract: target architecture,
  locked contracts, target crate graph.

This page covers what those two don't: planning a large change, git hygiene beyond the worktree
rule, and how to report a bug.

## Planning a Large Change

For new features, breaking changes, or architecture shifts, write the shape down before the
code, in this order:

1. **Problem** — what a user cannot do today, with a concrete example.
2. **Alternatives** — at least one you rejected, and why. A decision with no rejected alternative
   has not been decided, only defaulted into.
3. **Contract** — what changes for callers: public API, observable behavior, error cases, edge
   cases.
4. **Reference check** — for render/layout/paint/hit-test/semantics/scheduling changes, what does
   `.flutter/` do? If FLUI diverges, name what's better and how a test proves it (see
   [`AGENTS.md`](AGENTS.md)'s Design stance).
5. **Plan** — the dependency-ordered steps, each one shippable.

Put the record where the change is: an ADR under `docs/adr/` for a protocol-level or cross-crate
contract, a `## Mapping decisions` entry in the crate's `ARCHITECTURE.md` for a local one. A
design document with no code, and a code change with no record, are both incomplete.

## Git Hygiene

- Destructive git operations need an explicit go-ahead, because they throw away work that may
  exist nowhere else: `git checkout`, `git reset --hard`, `git stash`, `git push --force`,
  `git branch -D`. Prefer non-destructive alternatives (new branches, new commits, tags).
- Keep your own hooks and signing on: `--no-verify`, `--no-gpg-sign` and equivalents are for
  when the maintainer explicitly asks.
- The repository ships no git hook: CI's fast lane is the gate. For the answer before a push,
  `cargo xtask check-changed` runs that lane locally, and `cargo xtask gate` the non-test half of
  `cargo xtask ci`.

## Changelog

- A change a consumer would notice adds a fragment, `changelog.d/<branch-slug>.md`, instead of
  editing `CHANGELOG.md`: a Keep a Changelog `###` header and bullets under it. The format,
  naming and link rules are in [`changelog.d/README.md`](changelog.d/README.md), and
  `cargo xtask changelog --check` (part of `cargo xtask checks`) enforces them.
- Docs-only, tooling-only and CI-only changes need no fragment, and no gate requires one.
- At release: `cargo xtask changelog --dry-run` to read the merged `## [Unreleased]` region,
  then `cargo xtask changelog` to write it and remove the fragments, then commit. Renaming
  `[Unreleased]` to the version stays a manual edit.
- A pull request that already edits `CHANGELOG.md` directly can land as it is; the merge never
  touches existing lines.

## Reporting Bugs

Open a GitHub issue with:

- Reproduction steps (commands run, OS, Rust version).
- Expected vs. actual behavior.
- Relevant `RUST_LOG=debug` output (or a minimal `tracing` capture).
- Affected crate(s) and commit hash.

For a confirmed regression, add a test that fails on the current `main` first, then fix it. Paste
that test into the issue — it is the reproduction, and it is what stops the bug from coming back.

## Security and Conduct

- Report vulnerabilities privately through the process in [`SECURITY.md`](SECURITY.md).
- Project conduct rules live in [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## See Also

- [Getting Started](docs/getting-started.md) — toolchain setup and first build
- [Architecture](docs/architecture.md) — three-tree pipeline + layered DAG
- [Crates Map](docs/crates.md) — per-layer crate inventory and status
- [Testing](docs/testing.md) — quality gates and coverage targets
