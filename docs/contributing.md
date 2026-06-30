[← Testing](testing.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md)

# Contributing

Welcome! This page is the working agreement for changes to FLUI: how to plan, commit, lint, and ship a change without surprises.

## Read These First

Before opening a PR or even a planning issue, read:

1. [`docs/FOUNDATIONS.md`](FOUNDATIONS.md) — **architecture contract**: target architecture, locked contracts (C1–C9), target crate graph (Part IV).
2. [`docs/ROADMAP.md`](ROADMAP.md) — **construction plan**: dependency-ordered phases that move the workspace from current state to the target.
3. [`STRATEGY.md`](../STRATEGY.md) — product strategy and the three port rules ("behavior loyal, structure Rust-native").
4. [`docs/PORT.md`](PORT.md) — port methodology, refusal triggers, per-crate `ARCHITECTURE.md` template.
5. [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — the project constitution (last ratified v2.3.0). Non-negotiable rules: layered DAG, `unsafe` boundaries, no `unwrap()` / `println!`, on-demand rendering, etc. **⚠ The file is currently absent from the repo** (lost in a history squash; pending maintainer restore) — until then [`FOUNDATIONS.md`](FOUNDATIONS.md) is the live architecture contract.
6. [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) — full architectural rules and anti-patterns.
7. [`.ai-factory/rules/base.md`](../.ai-factory/rules/base.md) — project base rules (naming, modules, errors, logging, testing, unsafe).
8. [`CLAUDE.md`](../CLAUDE.md) — Claude Code-specific guidance for this repo (build commands, troubleshooting).
9. [Architecture overview](architecture.md) and [Crates Map](crates.md) — high-level orientation (current-state).

## Quality Gates

Every change must pass the local CI recipe:

```bash
just ci
```

This expands to formatting, workspace-inventory drift, port-methodology checks,
Clippy, and the workspace test suite. CI also runs `taplo fmt --check`,
`typos`, docs, benchmark compilation, and the configured nextest/GPU jobs.

See [Testing](testing.md) for per-crate commands, coverage targets, and benchmark setup.

## Speckit Workflow (large changes)

For new features, breaking changes, or architecture shifts, follow spec → plan → tasks → implement using the speckit skills:

```
/speckit.specify   create or update the feature spec
/speckit.clarify   ask up to 5 targeted clarification questions
/speckit.plan      generate the design / planning artifacts
/speckit.tasks     produce a dependency-ordered task list
/speckit.analyze   cross-artifact consistency check
/speckit.implement execute the planned tasks
```

Spec artifacts live in `specs/<NNN-feature-slug>/` (e.g. `specs/001-cli-completion/`). Reuse the templates in `.specify/templates/` and never copy stale specs.

## AI Factory Workflow (smaller changes)

For task-level work the AI Factory skills speed up planning and verification:

```
/aif-plan         scoped plan with optional git branch flow
/aif-implement    execute tasks from the active plan
/aif-review       review staged changes / current PR
/aif-verify       confirm completion against the plan
/aif-fix          targeted bug fix flow
/aif-commit       conventional commit message generator
```

`config.yaml` (`.ai-factory/config.yaml`) controls language, paths, and git workflow for these skills.

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

- **Formatter:** `rustfmt.toml` is authoritative (edition 2024, `max_width = 100`, Tall fn params). Run `cargo fmt --all` before commit.
- **Linter:** `cargo clippy --workspace --all-targets -- -D warnings` must pass with `clippy::all` and `clippy::pedantic`.
- **Naming:** snake_case for modules and functions, UpperCamelCase for types and traits, SCREAMING_SNAKE_CASE for constants. Crate names use the `flui-` prefix.
- **Errors:** library crates use `thiserror`-derived enums; application / CLI / build glue may use `anyhow::Error`. `anyhow` MUST NOT cross a library crate boundary.
- **Logging:** `tracing` only. No `println!`, `eprintln!`, or `dbg!` in committed code.
- **`unsafe`:** confined to `flui-platform`, `flui-painting`, `flui-engine`. Every block needs a `// SAFETY:` comment.
- **`unwrap()` / `expect()`:** disallowed outside tests, examples, and `// SAFETY:`-justified invariants.

## Architectural Constraints

- **Strict layered DAG.** Lower-layer crates must not depend on higher-layer crates. New edges require updating the layer table in the constitution.
- **No `Arc<Mutex<>>` for tree nodes.** Use arena allocation (`slab`) with the 1-based `NonZeroUsize` ID offset pattern. `Arc<Mutex<>>` is for shared infrastructure (platform state, owners), not topology.
- **No `dyn` without justification.** Prefer generics + arity types. `dyn` is reserved for genuinely heterogeneous trees and platform abstractions.
- **No platform code outside `flui-platform`.** Any `windows::*`, `cocoa::*`, `winit::*`, `objc2::*` import in widget / engine / painting code is wrong.
- **No `wgpu` types in widget or layout code.** GPU access flows through `flui-painting`'s abstract canvas API.
- **No polling render loops.** Use `ControlFlow::Wait`. Constitution-mandated.

For the full anti-pattern list see [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md).

## Reviewing a Change

A change is ready for review when:

- ✅ `just ci` passes (`fmt`, inventory drift, port-check, Clippy, tests).
- ✅ The dependency DAG is intact (no upward edges, no cycles).
- ✅ Public API additions / changes are documented (`///` on items, `//!` on crate roots).
- ✅ New `unsafe` blocks (if any) carry `// SAFETY:` comments and are inside the permitted crates.
- ✅ Tests cover the new behavior; coverage targets are met for the affected category.
- ✅ The commit message follows the conventional-commits format.

`/aif-review` automates the routine portions of this checklist on the staged diff.

## Reporting Bugs

Open a GitHub issue with:

- Reproduction steps (commands run, OS, Rust version).
- Expected vs. actual behavior.
- Relevant `RUST_LOG=debug` output (or a minimal `tracing` capture).
- Affected crate(s) and commit hash.

For confirmed regressions, the speckit workflow (`/speckit.specify` → `/speckit.plan` → ...) provides a structured path from report to fix.

## Security and Conduct

- Report vulnerabilities privately through the process in [`SECURITY.md`](../SECURITY.md).
- Project conduct rules live in [`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md).

## See Also

- [Getting Started](getting-started.md) — toolchain setup and first build
- [Architecture](architecture.md) — three-tree pipeline + layered DAG
- [Crates Map](crates.md) — per-layer crate inventory and status
- [Testing](testing.md) — quality gates and coverage targets
