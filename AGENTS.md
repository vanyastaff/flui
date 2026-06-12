# AGENTS.md

> Structural map of the FLUI repository for AI agents and new contributors. Keep this file in sync with the actual layout — when crates, top-level directories, or key entry points change significantly, update this file.

## Project Overview

FLUI is a modular, Flutter-inspired declarative UI framework for Rust with a three-tree architecture (View → Element → Render) and a `wgpu`-backed GPU rendering engine. Forward architecture and phase status are owned by [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md) (architecture contract) and [`docs/ROADMAP.md`](docs/ROADMAP.md) (construction plan); refer to ROADMAP for the current phase. Foundation layers are stable while higher layers are being landed incrementally.

## Tech Stack

- **Programming language:** Rust 1.96 (edition 2024)
- **Build system:** Cargo workspace (20+ crates organized in foundation / core / rendering / framework / application layers)
- **Async runtime:** `tokio` 1.43 LTS
- **Graphics:** `wgpu` 29.x, `lyon`, `glyphon`, `cosmic-text`, `glam`
- **Platform:** native Win32, AppKit, headless backends + `winit` 0.30 fallback
- **Diagnostics:** `tracing`, `tracing-forest`
- **Errors:** `thiserror` (libraries), `anyhow` (applications)

See [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md) for the full architectural and stack rationale.

## Project Structure

```
flui/
├── crates/                   # Workspace member crates
│   ├── flui-types/           # Foundation: base types and units (ACTIVE)
│   ├── flui-geometry/        # Foundation: geometry primitives (ACTIVE)
│   ├── flui-foundation/      # Foundation: utilities, primitives, log helpers (ACTIVE)
│   ├── flui-tree/            # Foundation: generic tree abstractions (ACTIVE)
│   ├── flui-platform/        # Foundation: cross-platform abstraction (ACTIVE — MVP)
│   ├── flui-macros/          # Foundation: derive macros (ACTIVE)
│   ├── flui-painting/        # Core: painting primitives (ACTIVE)
│   ├── flui-semantics/       # Core: accessibility (ACTIVE)
│   ├── flui-scheduler/       # Core: frame scheduling (ACTIVE)
│   ├── flui-layer/           # Core: layer composition (ACTIVE)
│   ├── flui-interaction/     # Core: gesture recognition (ACTIVE)
│   ├── flui-engine/          # Core: GPU rendering engine (ACTIVE)
│   ├── flui-hot-reload/      # Core: scene-plugin hot reload via dlopen (ACTIVE)
│   ├── flui-rendering/       # Framework: render tree (ACTIVE)
│   ├── flui-view/            # Framework: view + element tree (ACTIVE)
│   ├── flui-app/             # Application: top-level framework (ACTIVE — migration)
│   ├── flui-animation/       # Animation primitives (DISABLED until integration)
│   ├── flui-reactivity/      # Signal/reactive primitives (DISABLED)
│   ├── flui-devtools/        # Developer inspector (DISABLED)
│   ├── flui-cli/             # CLI commands (DISABLED)
│   ├── flui-build/           # Async cross-platform build pipeline (DISABLED)
│   └── flui-assets/          # Asset pipeline (DISABLED)
├── examples/                 # Runnable demos and platform smoke tests
│   ├── android_app/          # Widget-based hot-reloadable plugin (cdylib)
│   ├── android_demo/         # Android GPU demo (cdylib)
│   ├── android_scene/        # Hot-reloadable Android scene plugin (cdylib)
│   ├── desktop_scene/        # Hot-reloadable desktop scene plugin
│   ├── painting_demo/        # Painting + engine demo (Web/WASM)
│   ├── web_demo/             # Web/WASM platform demo (cdylib)
│   └── *.rs                  # Single-file desktop demos
├── tools/
│   └── web-server/           # Built-in web dev server (wasm-pack + HTTP serve)
├── docs/
│   ├── plans/                # Dated planning notes per major workstream
│   └── research/             # Dated research notes
├── specs/                    # Feature specs and per-change task notes
├── .pi/                      # Pi runtime configuration (settings + LSP)
├── .mcp.json                 # MCP server configuration (cratesio, rust-docs)
├── .compound-engineering/    # Compound Engineering config (legacy, being phased out)
├── .agents/skills/           # Generic agent skills (vendor-neutral)
├── .atl/skill-registry.md    # Auto-generated cross-agent skill index
├── .claude/                  # Claude Code residuals (hooks + active git worktrees only)
├── .flutter/                 # Vendored Flutter source (UI architecture reference, read-only)
├── .gpui/                    # Vendored GPUI source (Rust platform-pattern reference, read-only)
├── .cargo/config.toml        # Cargo build profile, linker, and target overrides
├── Cargo.toml                # Workspace manifest
├── Cargo.lock                # Resolved dependency graph
├── rustfmt.toml              # Formatter configuration
├── README.md                 # Public-facing project README
└── AGENTS.md                 # This file — structural map for AI agents
```

## Key Entry Points

| File | Purpose |
|------|---------|
| `Cargo.toml` | Workspace manifest — declares members, default-members, shared dependencies, and `rust-version` |
| `Cargo.lock` | Resolved dependency graph; commit-tracked |
| `.cargo/config.toml` | Cargo profiles, linker selection per target, Android NDK / WASM hooks |
| `rustfmt.toml` | Formatter settings (edition 2024, max_width = 100, Tall fn params) |
| `crates/flui-types/src/lib.rs` | Foundation types and unit system entry |
| `crates/flui-geometry/src/lib.rs` | Geometry primitives (Point, Rect, Size, transforms) |
| `crates/flui-foundation/src/lib.rs` | Foundation utilities, error types, log helpers |
| `crates/flui-macros/src/lib.rs` | Derive macros (Stateless, Stateful) |
| `crates/flui-tree/src/lib.rs` | Generic tree primitives shared by view / element / render trees |
| `crates/flui-platform/src/lib.rs` | `Platform` trait and `current_platform()` factory |
| `crates/flui-engine/src/lib.rs` | GPU rendering engine entry |
| `crates/flui-app/src/lib.rs` | Application framework entry |
| `examples/hello_world.rs` | Minimal desktop bootstrap |
| `examples/desktop_scene/` | Hot-reload-aware desktop scene plugin example |
| `tools/web-server/` | `cargo run -p web-server` launches the wasm-pack-aware dev server |

## Documentation

| Document | Path | Description |
|----------|------|-------------|
| **Foundations** | `docs/FOUNDATIONS.md` | **Architecture contract** — target architecture, locked contracts (C1–C9), target crate graph (Part IV) |
| **Roadmap** | `docs/ROADMAP.md` | **Construction plan** — dependency-ordered phases from current state to target |
| Strategy | `STRATEGY.md` | Product strategy and the three port rules ("behavior loyal, structure Rust-native") |
| Port methodology | `docs/PORT.md` | Governance + Dart→Rust translation manual: refusal triggers, lock decisions, per-crate ARCHITECTURE.md template, type map, idiom map, strings discipline, error mapping canon, inline port-marker tier, ecosystem-first adoption table |
| README | `README.md` | Project landing page |
| Getting Started | `docs/getting-started.md` | Prerequisites, build, run examples |
| Architecture | `docs/architecture.md` | Three-tree pipeline + layered DAG overview (current state) |
| Crates Map | `docs/crates.md` | Per-layer crate inventory and status |
| Testing | `docs/testing.md` | Build / test / clippy / fmt commands and coverage targets |
| Render harness | `crates/flui-rendering/docs/TESTING.md` | `RenderTester` / `Probe` API, catalog rules, multi-frame helpers |
| Contributing | `docs/contributing.md` | Workflow, commits, agent conventions |
| Crate architecture notes | `crates/<crate>/ARCHITECTURE.md` | Per-crate architecture (e.g. `flui-foundation/ARCHITECTURE.md`) |
| Plans | `docs/plans/` | Dated planning notes per workstream |
| Research | `docs/research/` | Dated investigations (e.g. GPU tessellation, Skia techniques) |
| Specs | `specs/` | Per-feature specifications and task notes |

## AI Context Files

| File | Purpose |
|------|---------|
| `AGENTS.md` | Structural map of the repository for any AI agent (this file) |
| `.pi/settings.json` | Pi runtime settings (primary agent harness for this repo) |
| `.pi/lsp.json` | LSP server registration for Pi |
| `.mcp.json` | MCP servers exposed to agents (cratesio, rust-docs) |
| `.atl/skill-registry.md` | Auto-generated cross-agent skill index (regenerate via `/skill-registry:refresh`) |
| `.agents/skills/` | Vendor-neutral agent skills usable from any harness |

## Build & Development Commands

This project uses **`justfile`** for build automation (cross-platform, Rust-friendly). Install [`just`](https://just.systems) and run `just` to see all recipes.

Most-used recipes:

| Recipe | Equivalent | Purpose |
|--------|------------|---------|
| `just check` | `cargo check --workspace --all-targets` | Fast type check |
| `just build` | `cargo build --workspace` | Build the workspace |
| `just test` | `cargo test --workspace` | Run all tests |
| `just clippy` | `cargo clippy --workspace -- -D warnings` | Lint gate (CI) |
| `just fmt` | `cargo fmt --all` | Format the workspace |
| `just fmt-check` | `cargo fmt --all -- --check` | Format gate (CI) |
| `just ci` | `fmt-check + clippy + test` | Full CI gate locally |
| `just example-hello` | `cargo run --example hello_world` | Platform smoke test |
| `just web-server` | `cargo run -p web-server` | WASM dev server |

Run `just` (no argument) for the full grouped recipe list. Raw `cargo` commands work too — `just` is a convenience layer, not a replacement.

## Agent Rules

- **Decompose chained shell commands.** Run each step as a separate command so failures, prompts, and tool gating remain inspectable.
  - Incorrect: `git checkout main && git pull`
  - Correct: first `git checkout main`, then `git pull origin main`
- **Never run destructive git operations** (`git checkout`, `git reset`, `git stash`, `git push --force`, `git branch -D`, etc.) without explicit user permission. Prefer non-destructive alternatives (new branches, new commits, tagging) and ask before discarding work.
- **Honor the architecture contract.** Cross-check any non-trivial change against [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md) and [`docs/ROADMAP.md`](docs/ROADMAP.md) — especially the dependency DAG, `unsafe` boundaries, and the no-`unwrap`/`println!` rules.
- **Track latest stable wgpu major.** The workspace currently uses 29.x (see `[workspace.dependencies]` in `Cargo.toml` for the caret pin and update policy). No active pin — the 25.x pin was lifted after the gfx-rs/wgpu#7915 codespan-reporting issue was resolved.
- **Reference, don't copy.** `.flutter/` and `.gpui/` exist as architectural references only; adapt their patterns to FLUI idioms (Arity system, Ambassador delegation, no nullability).
- **Use the ID offset pattern.** Slab indices are 0-based; public IDs (`ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`) are 1-based `NonZeroUsize`. Insert: `slab_index + 1`; lookup: `id.get() - 1`.
- **Logging via `tracing` only.** No `println!`, `eprintln!`, or `dbg!` in shipped code.

## Agent Memory / Quality Bar

- **Check Flutter before changing Flutter-parity behavior.** For render tree, sliver, layout, paint, hit-test, semantics, scheduling, and parent-data behavior, inspect the vendored `.flutter/` implementation first and preserve the behavioral contract unless FLUI has an explicit documented divergence.
- **Prefer behavior-first ports.** Translate Flutter semantics into Rust-native structure, but keep edge-case behavior loyal: scroll/cache windows, overlap handling, visibility gates, parent-data persistence, paint offsets, hit-test bounds, relayout boundaries, and error/retry semantics.
- **Test the edge, not just the happy path.** Any rendering fix should include regression tests for non-zero scroll/overlap/cache offsets, invisible or fully scrolled-out children, differing paint vs hit-test extents, relayout without repositioning, and cross-protocol Box/Sliver paths when relevant.
- **Verify before commit or PR update.** For `flui-rendering` work, run targeted tests for the changed behavior, then `cargo fmt --package flui-rendering -- --check`, `cargo test -p flui-rendering`, and `cargo clippy -p flui-rendering --all-targets -- -D warnings` unless the user explicitly asks for a lighter pass.
- **Be skeptical of broad fixes.** When expanding a commit path or shared pipeline behavior, check every protocol that now flows through it and seed/commit persistent state symmetrically so one protocol is not reset by another.

## Render-Object Testing Checklist

When adding or materially changing a **`RenderBox`** or **`RenderSliver`** in `flui-rendering`, land harness coverage in the same PR. Do not rely on GPU demos or manual inspection — use the headless pipeline via `flui_rendering::testing` (see [`crates/flui-rendering/docs/TESTING.md`](crates/flui-rendering/docs/TESTING.md)).

### 1. Register the type (CI will fail if skipped)

Every concrete type exported from `crates/flui-rendering/src/objects/mod.rs` must appear in the harness catalog:

| Step | File | Action |
|------|------|--------|
| Export | `objects/mod.rs` | `pub use …::RenderYourType` |
| Catalog list | `tests/render_object_harness.rs` | Add `"RenderYourType"` to `RENDER_OBJECT_TYPES` (keep sorted) |
| Coverage table | same file, module doc | Add a row: harness test name(s), Layout / Hit-test / Paint / Diagnostics |
| Harness test | same file | Add `#[test] fn harness_your_type_…()` that **uses the type name** inside the test body |

Two guards run in CI:

- `catalog_covers_every_render_object_name` — each `RENDER_OBJECT_TYPES` entry must appear in at least one `#[test] fn harness_*` block.
- `render_object_types_match_exports` — `RENDER_OBJECT_TYPES` must match `pub use` exports in `objects/mod.rs` (generic `RenderClip` is excluded; concrete clip variants are not).

### 2. Build the smallest meaningful tree

```rust
use flui_rendering::objects::*;
use flui_rendering::testing::{RenderTester, Probe, box_node, sliver_node};

// Box object — root or nested under a sized host
RenderTester::mount(box_node(RenderYourType::new(…)).label("node"))
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_layout(); // or .run_frame() when paint/layers matter

// Sliver object — always under a Box viewport root (never a sliver root)
RenderTester::mount(
    box_node(RenderViewport::new(AxisDirection::TopToBottom))
        .child(sliver_node(RenderYourSliver::new(…)).label("sliver")),
)
.with_size(Size::new(px(300.0), px(100.0)))
.run_layout();
```

- **Box root** for box-only objects; **viewport host** for every sliver.
- Use `.label("…")` on nodes you assert via `run.id("…")`.
- Stack / flex children need parent metadata: `TreeNode::with_stack_parent_data` or `with_flex_parent_data` (see [`parent_data.rs`](crates/flui-rendering/src/testing/parent_data.rs)).

### 3. Assert by capability (not every object needs every column)

| Capability | When required | Typical assertions |
|------------|---------------|-------------------|
| **Layout** | Always | `run.box_geometry(id)`, `run.offset(id)`, `run.sliver_geometry(id)`; `assert_has_committed_size` / `assert_has_committed_geometry` |
| **Diagnostics** | Always | `impl Diagnosticable` with **snake_case** property names; `assert_descendant_properties(&run.diagnostics(), "RenderYourType", &[…])`; use `run.property_f64` / `descendant_property_f64` for numeric fields (unit suffixes like `25px` are parsed) |
| **Hit-test** | Pointer-blocking or positioned semantics | `run.hit(x, y)`, `run.hit_first(x, y)` — include a negative case (miss / pass-through) |
| **Paint** | `perform_paint`, opacity, transform, decoration, picture layers | `.run_frame()` or `advance_paint`; `run.structure()`, `run.picture_bounds()`, `run.opacity_alpha()`, `run.has_picture_layer()` |
| **Multi-frame** | Animated or dirty-state behavior | `run.update::<T>`, `run.update_paint::<T>`, `advance_layout`, `simulate` + `AnimationController` (see `tests/harness_animation.rs`) |

Pick **at least one** `harness_<snake_name>_…` test per type. Split into multiple tests when layout vs paint vs hit-test need different tree shapes.

### 4. Diagnostics contract

- Object config: `debug_fill_properties` on the render type (`Diagnosticable`).
- Runtime state: pipeline adds committed fields (`paint_offset`, `size`, sliver `geometry`, etc.) — do not duplicate scroll offset as bare `offset` on viewports (`scroll_offset` is the config field).
- Property names are **snake_case** everywhere (render objects, layer tree, harness assertions).
- Avoid duplicate type names in one diagnostics tree; `find_descendant_unique` is used by `Probe::descendant_property*`.

### 5. Edge cases (regression tests beyond happy path)

Add extra tests when the Flutter port has non-obvious behavior:

- Non-zero scroll / cache / overlap offsets on viewport + sliver chains
- Invisible or fully scrolled-out children (zero geometry, no paint, no hit)
- Paint extent vs hit-test extent divergence (e.g. `RenderFractionalTranslation` without hit transform)
- Relayout without repositioning; paint-only mutation without layout (`update_paint` + `pump`)
- Cross-protocol paths (box child under sliver adapter, multiple slivers in one viewport)

Check `.flutter/` for the reference behavior before asserting.

### 6. Local verification (before PR)

```bash
cargo test -p flui-rendering --test render_object_harness
cargo test -p flui-rendering --test harness_animation   # if animation/multi-frame touched
cargo test -p flui-rendering
cargo fmt --package flui-rendering -- --check
cargo clippy -p flui-rendering --all-targets -- -D warnings
bash scripts/port-check.sh -v   # no duplicate cross-crate test helper names; testing modules gated
```

### 7. Naming conventions

| Item | Pattern | Example |
|------|---------|---------|
| Harness test fn | `harness_<snake_case>_…` | `harness_sliver_fixed_extent_list_geometry` |
| Type string in diagnostics | Rust struct name | `"RenderSliverFixedExtentList"` |
| Label registry | `LayerLabelRegistry` (layer), `RenderLabelRegistry` (render) — do not reintroduce a shared `IdRegistry` name across crates |
