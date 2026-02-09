<!--
  Sync Impact Report
  ==================
  Version change: 1.2.0 → 2.0.0 (MAJOR)
  Bump rationale: Complete restructuring — all 6 original principles
    replaced by 5 new architectural principles, 8 new sections added,
    scope expanded from narrow implementation rules to comprehensive
    project governance.

  Modified principles:
    - "Test-First for Public APIs" → merged into Testing section
    - "Type Safety First" → subsumed by Principle IV (Composition)
      and Rust Standards
    - "Structured Observability" → merged into Rust Standards
    - "On-Demand Rendering" → merged into Performance Constraints
    - "Coverage Requirements" → merged into Testing section
    - "ID Offset Pattern" → moved to Codebase Conventions

  Added sections:
    - Project Identity
    - Crate Architecture (full ~20 crate listing)
    - Core Architectural Principles (5 new principles)
    - Rust Standards
    - Widget System Conventions
    - Testing (expanded)
    - Performance Constraints
    - Rendering Backend
    - Codebase Conventions (ID Offset, Arity, Ambassador)
    - Anti-Patterns

  Removed sections:
    - Core Principles (6 narrow principles → replaced by above)
    - Technical Constraints (content redistributed)

  Templates requiring updates:
    - .specify/templates/plan-template.md ✅ no update needed
    - .specify/templates/spec-template.md ✅ no update needed
    - .specify/templates/tasks-template.md ✅ no update needed
    - .specify/templates/checklist-template.md ✅ no update needed
    - .specify/templates/agent-file-template.md ✅ no update needed

  Files requiring follow-up:
    - CLAUDE.md ⚠ update version reference 1.2.0 → 2.0.0
      and Constitution Compliance section content
-->

# FLUI Constitution

## Project Identity

FLUI is a modular, Flutter-inspired declarative UI framework for Rust.
It provides a widget-based declarative UI system with a layered crate
architecture, built on wgpu for high-performance GPU-accelerated
rendering. The proven three-tree architecture
(Widget → Element → RenderObject) drives the rendering pipeline.

## Crate Architecture

Monorepo with ~20 specialized crates organized in dependency layers.
Dependencies flow strictly downward — no circular dependencies.

| Layer | Crate | Responsibility |
|-------|-------|---------------|
| Foundation | `flui-types` | Base types reused by all crates |
| Foundation | `flui-foundation` | Geometry, colors, text styles |
| Reactivity | `flui-reactivity` | Signals, effects, state management |
| Tree | `flui-tree` | Widget tree: build, diff, reconciliation |
| Rendering | `flui-rendering` | Render objects, layout protocol, paint |
| Painting | `flui-painting` | Low-level draw primitives (canvas, paths) |
| Compositing | `flui-layer` | Compositing layer tree |
| Accessibility | `flui-semantics` | Accessibility tree |
| Interaction | `flui-interaction` | Hit testing, gestures, focus, pointers |
| Animation | `flui-animation` | Controllers, curves, tweens, implicit |
| Scheduling | `flui-scheduler` | Frame scheduling, microtasks, lifecycle |
| Engine | `flui-engine` | Pipeline: build → layout → paint → composite |
| Platform | `flui-platform` | Window management, event loop, platform channels |
| Build | `flui-build` | Build macros, codegen, platform builders |
| Assets | `flui-assets` | Asset loading, caching, image decoding |
| App | `flui-app` | App runner, root widget, app lifecycle |
| CLI | `flui-cli` | CLI tooling (create, build, run, dev) |
| DevTools | `flui-devtools` | Inspector, widget tree viewer, perf overlay |
| Logging | `flui-log` | Structured logging for framework internals |

## Core Architectural Principles

### I. Flutter as Reference, Not Copy

Follow Flutter's proven patterns (widget → element → render object,
owner-based dirty marking, relayout/repaint boundaries) but adapt
idiomatically to Rust's ownership model. No GC, no runtime reflection.

- MUST study Flutter's architecture for layout algorithms and
  widget lifecycle patterns.
- MUST NOT translate Dart code line-by-line — adapt patterns to
  Rust idioms (ownership, borrowing, lifetimes, enums).

### II. Strict Crate Dependency DAG

Dependencies flow downward only. No circular dependencies permitted.

- `flui-types` and `flui-foundation` are leaf crates with zero
  intra-project dependencies.
- Higher-level crates (`flui-app`, `flui-engine`) compose
  lower-level ones.
- Adding a new dependency edge MUST be reviewed against the
  layer table above.

### III. Zero Unsafe in Widget/Application Layer

`unsafe` is permitted ONLY in:
- `flui-platform` (FFI to OS/window APIs)
- `flui-painting` (FFI to graphics backends)
- `flui-engine` internals (where absolutely necessary)

Every `unsafe` block MUST have a `// SAFETY:` comment explaining
the invariant being upheld. Widget code, application code, and
layout algorithms MUST NOT contain `unsafe`.

### IV. Composition Over Inheritance

Rust has no inheritance. Use trait composition, generics, and the
widget pattern (StatelessWidget, StatefulWidget, RenderObjectWidget)
via traits + associated types.

- Prefer generics and enum dispatch over `dyn` trait objects.
- Use `dyn` dispatch only when type erasure is genuinely required
  (e.g., heterogeneous widget trees, platform abstractions).

### V. Declarative API, Imperative Internals

The public widget API is fully declarative: `build()` returns a
widget tree description. Internal scheduling, layout, and painting
are imperative and optimized for minimal allocations.

- Widget authors MUST only interact via declarative `build()`.
- Framework internals MAY use imperative mutation, arena allocation,
  and index-based access for performance.

## Rust Standards

- **Edition**: 2021 | **Minimum Rust version**: 1.91
- **Strict clippy**: `clippy::all` and `clippy::pedantic` at warn
  level workspace-wide (via `[workspace.lints.clippy]`)
- **No `unwrap()`/`expect()` in library code** — propagate errors
  with `thiserror` (typed errors) or `anyhow` (application errors).
  Tests MAY use `unwrap()`.
- **No `println!`/`eprintln!`/`dbg!` in committed code** — all
  logging MUST use the `tracing` crate (`info!`, `debug!`, `warn!`,
  `error!`). Use `#[tracing::instrument]` for span-based tracing.
- **Naming**: `snake_case` for modules/functions, `CamelCase` for
  types/traits, `SCREAMING_CASE` for constants. Widget names match
  Flutter conventions where applicable (Container, Row, Column,
  Stack, Padding).
- **Documentation**: Every public item MUST have `///` doc comments.
  Crate roots MUST have `//!` overview documentation.
- **Minimal dependencies**: Prefer `std`/`core` where possible.
  Each external dependency MUST be justified. No "kitchen sink"
  crates.

## Widget System Conventions

- Widgets are immutable value types. No interior mutability in the
  widget layer.
- State lives in `State<W>` objects managed by the element tree.
- `BuildContext` provides tree traversal and inherited widget lookup.
- Keys (`ValueKey`, `GlobalKey`) control identity and state
  preservation across rebuilds.
- `Widget::build()` MUST be pure — no side effects, no I/O, no
  mutation of external state.

## Testing

- **Unit tests** in each crate (`mod tests`).
- **Integration tests** for cross-crate pipelines (`flui-engine`
  tests).
- **Property-based tests** (`proptest`) for layout algorithms and
  geometric operations.
- **Visual regression tests** for rendering (snapshot-based).
- **No mocking frameworks** — use trait-based test doubles.
- `cargo test --workspace` MUST always pass.

Minimum coverage thresholds by crate category:

| Category | Minimum Coverage | Examples |
|----------|-----------------|----------|
| Core | 80% | `flui-tree`, `flui-foundation` |
| Platform | 70% | `flui-platform` |
| Widget | 85% | `flui-widgets` |

## Performance Constraints

- **Widget rebuild**: < 1ms for 1000 widgets.
- **Layout pass**: single-pass O(n) where possible (Flutter model).
- **Frame target**: 60fps minimum on desktop (16ms frame budget).
- **Hot path allocations**: zero allocations in layout and paint
  after initial build.
- **Rendering mode**: on-demand. The event loop MUST use
  `ControlFlow::Wait` (event-driven wakeup). Polling loops are
  prohibited. Rendering occurs only when the UI tree is dirty.
  Animations trigger re-renders via explicit invalidation
  (`mark_needs_layout`, `mark_needs_paint`).
- **Benchmark suite**: `criterion` for regression detection.

## Rendering Backend

- **Primary backend**: wgpu (cross-platform, Rust-native, WebGPU API).
  MUST stay on wgpu 25.x (26.0+ has breaking issues).
- **Backend abstraction**: `flui-painting` defines `trait PaintBackend`.
  `flui-platform` contains concrete implementations. Architecture
  is ready for alternative backends (blade, skia-safe, vello) but
  currently optimized for wgpu.
- **No wgpu internals leakage**: all GPU code goes through the
  abstract canvas/paint API in `flui-painting`. Widgets and layout
  MUST NOT reference wgpu types directly.
- **Text rendering**: `cosmic-text` / `glyphon`.
- **Shader pipeline**: WGSL shaders stored in `flui-painting/shaders/`.
- **Current platforms**: desktop (Windows, macOS, Linux) via winit.
- **Future platforms**: web (WASM + WebGPU), mobile (TBD).

## Codebase Conventions

### ID Offset Pattern

Slab-based storage uses 0-based indices internally. All public ID
types MUST use 1-based values via `NonZeroUsize`:

```rust
let slab_index = self.nodes.insert(node);
let id = ElementId::new(slab_index + 1); // +1 for NonZeroUsize
self.nodes.get(element_id.get() - 1);    // -1 to recover index
```

Applies to: `ViewId`, `ElementId`, `RenderId`, `LayerId`,
`SemanticsId`.

### Arity System

Child count MUST be encoded in types: `Leaf` (0), `Single` (1),
`Optional` (0..1), `Variable` (0..n). Runtime child count
mismatches become compile-time errors.

### Ambassador Delegation

Trait forwarding through composed fields MUST use the `ambassador`
crate (`#[delegatable_trait]`, `#[derive(Delegate)]`), not manual
boilerplate.

### Node Storage

MUST NOT use `Arc<Mutex<>>` for widget/element/render trees. Use
arena allocation (slab, slotmap) for node storage with index-based
references.

## Anti-Patterns

These patterns are explicitly prohibited:

- **Dart-to-Rust translation**: Do not copy Dart code. Adapt
  patterns to Rust idioms (ownership, enums, traits).
- **`Arc<Mutex<>>` for tree structures**: Use arena allocation
  or slotmap for node storage.
- **`dyn Widget` without justification**: Prefer generics and enum
  dispatch. Use `dyn` only when type erasure is genuinely required.
- **Platform-specific code outside `flui-platform`**: All OS/window
  system interaction MUST be confined to `flui-platform`.
- **Polling render loops**: See Performance Constraints.
- **`unwrap()`/`expect()` in library code**: See Rust Standards.
- **`println!`/`dbg!` in committed code**: See Rust Standards.

## Git & Workflow

- **Conventional commits**: `feat(flui-tree): add reconciliation`.
  Prefixes: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`.
- **One logical change per commit**.
- **Feature branches off main**.
- **Workspace-level `Cargo.toml`** with shared dependency versions.
- **Speckit workflow**: large changes (new features, breaking changes,
  architecture shifts) MUST follow spec → plan → tasks → implement.
- **Destructive git operations** (`git reset --hard`, force-push,
  branch deletion, `git stash`) MUST NOT be performed without
  explicit user permission.
- **Lint gate**: `cargo clippy --workspace -- -D warnings` MUST pass.
- **Format gate**: `cargo fmt --all` MUST pass.

## Governance

- This constitution supersedes all other development practices.
  In case of conflict, the constitution wins.
- Amendments require:
  1. Documentation of the change and rationale.
  2. Semantic version bump (MAJOR for removals/redefinitions,
     MINOR for additions, PATCH for clarifications).
  3. Sync of the version reference in `CLAUDE.md`.
- Compliance is checked at plan time via the "Constitution Check"
  gate in `plan-template.md`.
- All code reviews MUST verify adherence to these principles.

**Version**: 2.0.0 | **Ratified**: 2026-02-08 | **Last Amended**: 2026-02-08
