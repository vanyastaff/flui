[← Testing](testing.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md)

# Contributing

Welcome! This page is the working agreement for changes to FLUI: how to plan, commit, lint, and ship a change without surprises.

## Read These First

Before opening a PR or even a planning issue, read:

1. [`docs/FOUNDATIONS.md`](FOUNDATIONS.md) — **architecture contract**: target architecture, the three architectural rules, locked contracts (C1–C9), target crate graph (Part IV).
2. [`docs/ROADMAP.md`](ROADMAP.md) — **construction plan**: dependency-ordered phases that move the workspace from current state to the target.
3. [`docs/PORT.md`](PORT.md) — port methodology, refusal triggers, per-crate `ARCHITECTURE.md` template.
4. [`AGENTS.md`](../AGENTS.md) — the non-negotiable rules of this workspace: layered DAG, `unsafe` boundaries, no `unwrap()` / `println!`, no polling render loops, and the build/CI commands. `docs/FOUNDATIONS.md` (item 1 above) carries the full rule and anti-pattern reference.
5. [Architecture overview](architecture.md) and [Crates Map](crates.md) — high-level orientation (current-state).

## Quality Gates

Every change must pass the local CI recipe:

```bash
just ci
```

This expands to formatting, workspace-inventory drift, port-methodology checks,
Clippy, and the workspace test suite. CI also runs `taplo fmt --check`,
`typos`, per-feature clippy (cargo-hack), a wasm32 target check, docs,
benchmark compilation, and the configured nextest/GPU jobs.

See [Testing](testing.md) for per-crate commands, coverage targets, and benchmark setup.

## Planning a Large Change

For new features, breaking changes, or architecture shifts, write the shape
down before the code, in this order:

1. **Problem** — what a user cannot do today, with a concrete example.
2. **Alternatives** — at least one you rejected, and why. A design decision
   with no rejected alternative has not been decided, only defaulted into.
3. **Contract** — what changes for callers: public API, observable behavior,
   error cases, edge cases.
4. **Reference check** — if the change touches render/layout/paint/hit-test/
   semantics/scheduling, what does `.flutter/` do (see [`PORT.md`](PORT.md))? If
   FLUI diverges, name what is better and how a test proves it.
5. **Plan** — the dependency-ordered steps, each one shippable.

Put the record where the change is: an ADR under `docs/adr/` for a
protocol-level or cross-crate contract, a `## Mapping decisions` entry in the
crate's `ARCHITECTURE.md` for a local one. A design document with no code and
a code change with no record are both incomplete.

Existing specs live in `specs/`; check whether one already covers your area
before writing a new document.

## Working With an AI Agent

This repository carries `AGENTS.md` files at the root and per crate — they are
the cross-tool agent guide, and they are written for humans too (the root one
opens with the three rules every change is measured against). A contributor
using an agent should have it read the root `AGENTS.md` and the target crate's
`crates/<crate>/AGENTS.md` before starting.

The agent-facing rules are the same as the human ones: run `just ci`, do not
skip hooks, do not commit without being asked, and do not report work as done
without the verification [Definition of Done](../AGENTS.md#definition-of-done-anti-cheating)
requires.

## Conventional Commits

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org):

```
feat(rendering): add RenderFlex parent-data wiring
fix(platform): use Weak<RwLock<>> instead of raw pointer in RenderView
refactor(rendering): rename duplicate HitTestable to ViewHitTestable
test(tree): add property-based tests for arity coercions
docs(architecture): document three-tree pipeline contract
chore: bump tracing-subscriber to 0.3.20
```

Allowed prefixes: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`. The optional `(scope)` is usually a crate name without the `flui-` prefix. Aim for one logical change per commit.

## Git Hygiene

- Branch off `main`. Default base branch for plans is `main`.
- Use `feature/<slug>` prefixes for new feature branches (`config.yaml` `git.branch_prefix`).
- **Never** run destructive git operations without explicit user permission: `git checkout`, `git reset --hard`, `git stash`, `git push --force`, `git branch -D`. Prefer non-destructive alternatives (new branches, new commits, tags). This is enforced by `AGENTS.md` rules.
- One change, one commit. Avoid bundled commits that mix refactors, fixes, and features.
- Hooks must not be skipped. `--no-verify`, `--no-gpg-sign`, and equivalent flags are reserved for explicit user requests.

## Code Style

`STYLE.md` was retired in 2026-09. What governs a change here is the enforced set: `rustfmt.toml`,
`clippy.toml`, the workspace lints in the root `Cargo.toml`, `just port-check`'s architecture
refusal triggers, the architecture contract that ships with each crate, accepted ADRs
(`docs/adr/`), and [`AGENTS.md`](../AGENTS.md) together with [`docs/FOUNDATIONS.md`](FOUNDATIONS.md).
Do not copy a subset into a crate and let it drift.

## Architectural Constraints

- **Strict layered DAG.** Lower-layer crates must not depend on higher-layer crates. New edges require updating the layer table in the constitution.
- **No `Arc<Mutex<>>` for tree nodes.** Use arena allocation (`slab`) with the 1-based `NonZeroUsize` ID offset pattern. `Arc<Mutex<>>` is for shared infrastructure (platform state, owners), not topology.
- **No `dyn` without justification.** Prefer generics + arity types. `dyn` is reserved for genuinely heterogeneous trees and platform abstractions.
- **No platform code outside `flui-platform`.** Any `windows::*`, `cocoa::*`, `winit::*`, `objc2::*` import in widget / engine / painting code is wrong.
- **No `wgpu` types in widget or layout code.** GPU access flows through `flui-painting`'s abstract canvas API.
- **No polling render loops.** Use `ControlFlow::Wait`. Constitution-mandated.

For the full anti-pattern list see [`docs/FOUNDATIONS.md`](FOUNDATIONS.md).

## Reviewing a Change

A change is ready for review when:

- ✅ `just ci` passes (`fmt`, inventory drift, port-check, Clippy, tests).
- ✅ The dependency DAG is intact (no upward edges, no cycles).
- ✅ Public API additions / changes are documented (`///` on items, `//!` on crate roots).
- ✅ New `unsafe` blocks (if any) carry `// SAFETY:` comments and are inside the permitted crates.
- ✅ Tests cover the new behavior; coverage targets are met for the affected category.
- ✅ The commit message follows the conventional-commits format.

Review the diff against this checklist before you ask for review — the routine
failures (a fmt diff, a stale doc link, a test that would pass without the
change) are cheapest to catch yourself.

## Reporting Bugs

Open a GitHub issue with:

- Reproduction steps (commands run, OS, Rust version).
- Expected vs. actual behavior.
- Relevant `RUST_LOG=debug` output (or a minimal `tracing` capture).
- Affected crate(s) and commit hash.

For a confirmed regression, add a test that fails on the current `main`
first, then fix it. Paste that test into the issue — it is the reproduction,
and it is what stops the bug from coming back.

## Security and Conduct

- Report vulnerabilities privately through the process in [`SECURITY.md`](../SECURITY.md).
- Project conduct rules live in [`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md).

## See Also

- [Getting Started](getting-started.md) — toolchain setup and first build
- [Architecture](architecture.md) — three-tree pipeline + layered DAG
- [Crates Map](crates.md) — per-layer crate inventory and status
- [Testing](testing.md) — quality gates and coverage targets
