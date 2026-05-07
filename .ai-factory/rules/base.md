# FLUI Project Base Rules

> Auto-detected conventions from codebase analysis (Rust 1.94, edition 2024 workspace).
> These rules are project-wide axioms; area-specific rules go under `.ai-factory/rules/<area>.md`.

## Naming Conventions

- **Crates:** kebab-case with `flui-` prefix (`flui-types`, `flui-rendering`, `flui-platform`).
- **Modules:** snake_case (`render_box`, `pipeline_owner`).
- **Files:** snake_case `.rs` files mapped to module names.
- **Types / structs / enums / traits:** `UpperCamelCase` (`RenderObject`, `BoxChild`, `ElementId`, `PaintContext`).
- **Functions / methods / variables:** snake_case (`mark_needs_layout`, `perform_layout`).
- **Constants / statics:** SCREAMING_SNAKE_CASE.
- **Lifetimes:** short single-letter or descriptive lowercase (`'a`, `'view`).
- **Generics:** single uppercase letters with semantic intent (`A` for arity, `T` for value).
- **ID types:** wrap `NonZeroUsize` and follow the offset pattern (see Module Structure).

## Module Structure

- **Layered crate DAG (foundation → core → rendering → framework → application).** Dependencies flow downward only. No upward references and no cycles. Aligns with `.specify/memory/constitution.md` (v2.2.0).
- **Crate layers:**
  - Foundation: `flui-types`, `flui-foundation`, `flui-tree`, `flui-platform`.
  - Core / rendering: `flui-painting`, `flui-semantics`, `flui-scheduler`, `flui-layer`, `flui-interaction`, `flui-engine`, `flui-log`, `flui-hot-reload`.
  - Framework: `flui-rendering`, `flui-view`.
  - Application: `flui-app`.
  - Disabled until integration completes: `flui-animation`, `flui-reactivity`, `flui-devtools`, `flui-cli`, `flui-build`, `flui-assets`.
- **Per-crate layout:** `src/lib.rs` re-exports public API; submodules grouped by responsibility; `prelude` module where useful (`use flui_rendering::prelude::*;`).
- **ID offset pattern (CRITICAL):** Slab uses 0-based indices; public IDs use 1-based `NonZeroUsize`. Insert: `slab_index + 1`; lookup: `id.get() - 1`. Applies to `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`. `Option<ElementId>` is 8 bytes thanks to `NonZeroUsize`.
- **Arity system:** Render children must use type-safe arity wrappers — `Leaf`, `Single`, `Optional`, `Variable`. `BoxChild<Single>` matches `impl RenderBox<Single>`; do not use `Vec<Box<dyn RenderObject>>` directly.

## Error Handling

- **Library crates use `thiserror`:** every crate that defines its own error type provides `src/error.rs` with `#[derive(Debug, thiserror::Error)]` enums (`flui-foundation`, `flui-painting`, `flui-platform`, `flui-cli`, `flui-build`, `flui-assets`, `flui-reactivity`).
- **Application / glue code may use `anyhow`** for context-rich errors (CLI, build pipelines, examples). Library APIs must not leak `anyhow::Error` across crate boundaries.
- **No `unwrap()` / `expect()` outside tests, examples, or `unsafe` invariants documented with `// SAFETY:` comments.** Constitution principle.
- **No `panic!` in widget/app layer.** Recoverable failures return `Result<T, E>`; truly unreachable states use `unreachable!()` with justification.
- **Result type aliases** are encouraged per crate: `pub type Result<T> = std::result::Result<T, Error>;`.

## Logging

- **Always `tracing` — never `println!`, `eprintln!`, or `dbg!`.** Constitution principle.
- **Subscriber init lives in `flui-log`** and applications wire it up at startup.
- **Function-level instrumentation:** use `#[tracing::instrument]` on hot paths and lifecycle methods; prefer structured fields over string formatting.
- **Levels:** `error` for recoverable failures surfaced to the user, `warn` for unexpected-but-handled paths, `info` for lifecycle and frame milestones, `debug` for layout/paint traces, `trace` for per-pixel / per-event detail.

## Testing

- **Unit tests live in the same file** under `#[cfg(test)] mod tests { ... }`.
- **Integration tests** live in `tests/` per crate.
- **Examples** under `examples/<name>.rs` (single-file) or `examples/<name>/` (multi-file) for runnable demos and platform smoke tests.
- **Coverage targets** (constitution-mandated): core ≥ 80 %, platform ≥ 70 %, widget layer ≥ 85 %.
- **No mocks for GPU / platform.** Use `flui-platform` `HeadlessPlatform` for tests that need a windowing surface; integration tests on real platforms are required for `flui-platform`.

## Unsafe Code

- **`unsafe` permitted only in `flui-platform`, `flui-painting`, `flui-engine`.** Constitution principle: zero `unsafe` in widget/app layers.
- **Every `unsafe` block requires a `// SAFETY:` comment** explaining the invariant being upheld.
- **Prefer interior mutability via `parking_lot::Mutex` / `Arc<RwLock<T>>`** over raw pointers; raw `*const T` / `*mut T` only when interfacing with FFI.

## Concurrency

- **Async runtime:** `tokio` 1.43 (LTS).
- **Sync primitives:** prefer `parking_lot` over `std::sync` for `Mutex` / `RwLock`.
- **Channels:** `flume` 0.11 by default; `crossbeam` for SPSC/MPMC where needed.
- **Hot data structures:** `dashmap` for sharded concurrent maps; `slab` for stable indices (combined with the ID offset pattern).
- **Send/Sync expectations:** public types in `flui-platform`, `flui-engine`, and `flui-scheduler` document their thread-safety story explicitly.

## Code Style

- **`rustfmt.toml` is authoritative.** Edition 2024, `max_width = 100`, `fn_params_layout = "Tall"`, `use_try_shorthand = true`, `use_field_init_shorthand = true`, `force_explicit_abi = true`. Run `cargo fmt --all` before commit.
- **Clippy zero-warning policy.** `cargo clippy --workspace -- -D warnings` must pass.
- **Imports:** group `std`, external crates, then `crate::` paths; rely on `reorder_imports = true`.
- **Match arms:** no leading pipes (`match_arm_leading_pipes = "Never"`).

## Build & Tooling

- **Workspace build order:** Foundation → Core → Rendering → Framework → Application. Disabled crates listed in `Cargo.toml` `[workspace.members]` comments.
- **`wgpu` pinned at 25.x** (26.0+ is broken — see `https://github.com/gfx-rs/wgpu/issues/7915`).
- **Minimum Rust version:** 1.94 (`workspace.package.rust-version`). Do not bump without updating CI.
- **Conventional commits:** `feat`, `fix`, `refactor`, `test`, `docs`, `chore` (with optional scope, e.g. `feat(rendering): ...`).
- **Never** run `git checkout`, `git reset`, `git stash`, or other destructive git operations without explicit user permission.

## Documentation

- **Crate-level `//!` docs** describe purpose, layer, and public entry points.
- **`#[doc(hidden)]`** on internal items that must remain `pub` for cross-crate use but are not part of the stable surface.
- **Architecture documents per crate:** `crates/<name>/ARCHITECTURE.md` for non-trivial crates (`flui-foundation` has one). Workspace-wide architecture lives in `.ai-factory/ARCHITECTURE.md`.
- **Reference sources** for design comparisons live under `.flutter/` (UI architecture) and `.gpui/` (Rust platform patterns) — never copied verbatim, always adapted to FLUI idioms.
