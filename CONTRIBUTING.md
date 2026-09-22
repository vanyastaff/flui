# Contributing to FLUI

Welcome! The rules for a change live in two places:

- [`AGENTS.md`](AGENTS.md) — how to work (worktrees, task/report format, commits, PRs, what not
  to touch), the local gate commands, the architecture-constraint table, and the Definition of
  Done. Written for humans and agents alike.
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
   `.flutter/` do (see [`docs/PORT.md`](docs/PORT.md))? If FLUI diverges, name what's better and
   how a test proves it.
5. **Plan** — the dependency-ordered steps, each one shippable.

Put the record where the change is: an ADR under `docs/adr/` for a protocol-level or cross-crate
contract, a `## Mapping decisions` entry in the crate's `ARCHITECTURE.md` for a local one. A
design document with no code, and a code change with no record, are both incomplete.

## Git Hygiene

- Never run destructive git operations without explicit user permission: `git checkout`,
  `git reset --hard`, `git stash`, `git push --force`, `git branch -D`. Prefer non-destructive
  alternatives (new branches, new commits, tags).
- Hooks must not be skipped. `--no-verify`, `--no-gpg-sign`, and equivalents are reserved for
  explicit user requests.
- `just install-hooks` points git at the checked-in pre-push hook (`just gate`, the non-test half
  of `just ci`), with a text-only fast path for markdown-only pushes.

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
