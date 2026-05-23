# Architecture: Layered Modular Workspace + Three-Tree Pipeline

## Overview

FLUI uses a **Layered Modular Workspace** pattern at the workspace level: 20+ crates are organized into a strict directed acyclic graph (DAG) of dependency layers, each crate exposing an explicit public API through its `lib.rs` (and an optional `prelude` module) while internal modules remain private. At runtime FLUI follows the **Three-Tree Pipeline** pattern adapted from Flutter: an immutable `View` tree describes intent, a mutable `Element` tree owns state and reconciliation, and a `Render` tree performs layout and paint. The two patterns are complementary — the workspace layout enforces *what code is allowed to depend on what*, and the runtime pipeline enforces *how data flows between trees per frame*.

This architecture is mandated by `.specify/memory/constitution.md` (v2.3.0). Any change that weakens crate boundaries, introduces a circular dependency, or bypasses pipeline phases is a constitutional violation. The forward architecture (target crate graph, locked contracts) is owned by [`docs/FOUNDATIONS.md`](../docs/FOUNDATIONS.md) (the architecture contract); the migration sequencing lives in [`docs/ROADMAP.md`](../docs/ROADMAP.md).

## Decision Rationale

- **Project type:** GPU-accelerated declarative UI framework (library, not a service) targeting desktop, mobile, and web.
- **Tech stack:** Rust 1.94 (edition 2024), Cargo workspace, `wgpu` 25.x, `tokio` 1.43, `parking_lot`, `slab`, `ambassador`, `thiserror` / `anyhow`, `tracing`.
- **Key factor:** A UI framework needs (1) clear extensibility points (Widget / RenderObject / Platform) without leaking implementation details, and (2) a deterministic per-frame data flow that the borrow checker can verify. Layered crates give us the first; the three-tree pipeline gives us the second.
- **Why not Clean Architecture / DDD / Microservices:** FLUI has no business domain entities, no transactional boundaries, and no service-deployment story. Those patterns would create empty layers and no dependency-rule wins.
- **Why not pure Layered or pure Modular Monolith:** layered alone does not capture per-crate public-API discipline; modular alone does not capture the strict downward dependency direction required by the constitution.

## Folder Structure

```
flui/
├── Cargo.toml                 # Workspace manifest — declares all crates and shared deps
├── Cargo.lock
├── rustfmt.toml               # Formatter contract (edition 2024, max_width = 100)
├── .cargo/config.toml         # Per-target linker, profiles, target overrides
│
├── crates/                    # Workspace members, organized by layer
│   │
│   │   ── Layer 0: Value types ──
│   ├── flui-types/            # Base value types, units, IDs (NonZeroUsize);
│   │                            geometry, styling, typography, layout enums,
│   │                            gestures, physics, platform value types
│   │
│   │   ── Layer 1: Framework primitives + Tree primitives ──
│   ├── flui-foundation/       # Framework primitives: ChangeNotifier / Listenable,
│   │                            Id system, BindingBase, Key, diagnostics, error helpers
│   ├── flui-tree/             # Generic tree abstractions: TreeRead / TreeNav / TreeWrite
│   │                            trio, iterators / slots, arity markers (Leaf / Single /
│   │                            Optional / Variable), depth markers. Cycle 3 audit deleted
│   │                            speculative visitor / diff modules (zero in-workspace use)
│   │
│   │   ── Layer 2: Reactivity (currently disabled) ──
│   ├── flui-reactivity/       # Signals / effects (DISABLED until integration)
│   │
│   │   ── Layer 3: Painting / Layer / Semantics / Interaction ──
│   ├── flui-painting/         # Canvas API, DisplayList, paths, paint commands
│   ├── flui-layer/            # Layer composition tree
│   ├── flui-semantics/        # Accessibility tree
│   ├── flui-interaction/      # Hit-testing, gestures, focus
│   │
│   │   ── Layer 4: Scheduling / Rendering / Animation ──
│   ├── flui-scheduler/        # Frame scheduling, microtasks
│   ├── flui-rendering/        # Render objects, layout protocol, RenderBox<Arity>
│   ├── flui-animation/        # Curves, tweens, controllers (DISABLED)
│   │
│   │   ── Layer 5: Engine / Platform / Logging ──
│   ├── flui-engine/           # GPU pipeline (build → layout → paint → composite)
│   ├── flui-platform/         # Win32 / AppKit / winit / Headless backends
│   ├── flui-log/              # tracing setup and helpers
│   │
│   │   ── Layer 6: View / Assets / Build ──
│   ├── flui-view/             # View + Element tree, BuildContext
│   ├── flui-assets/           # Asset loading & caching (DISABLED)
│   ├── flui-build/            # Async PlatformBuilder (DISABLED)
│   │
│   │   ── Layer 7: Hot-Reload ──
│   ├── flui-hot-reload/       # dlopen-based scene plugin host
│   │                            (depends on flui-view via `app-plugin` feature)
│   │
│   │   ── Layer 8: Application & tooling ──
│   ├── flui-app/              # App runner, root widget, lifecycle
│   ├── flui-cli/              # CLI tooling (DISABLED)
│   └── flui-devtools/         # Inspector / perf overlay (DISABLED)
│
├── examples/                  # Runnable demos (single-file *.rs and per-target dirs)
├── tools/web-server/          # wasm-pack-aware dev server
├── docs/{plans,research}/     # Dated planning and research notes
├── .specify/memory/constitution.md  # Project constitution (v2.3.0) — MANDATORY
├── .ai-factory/               # AI Factory configuration and artifacts
├── .flutter/, .gpui/          # Vendored references (read-only, never copied)
└── AGENTS.md, CLAUDE.md, README.md
```

Per-crate layout follows a uniform shape:

```
crates/<name>/
├── Cargo.toml
├── ARCHITECTURE.md            # Optional, present for non-trivial crates
├── src/
│   ├── lib.rs                 # Public API: re-exports + prelude module
│   ├── error.rs               # thiserror::Error type, `pub type Result<T>` alias
│   ├── prelude.rs             # Optional curated public re-exports
│   └── <feature_modules>/     # Internal modules (pub(crate) by default)
├── tests/                     # Integration tests
└── examples/                  # Crate-scoped runnable demos (where useful)
```

## Dependency Rules

The dependency direction is **strictly downward** through the layer table. Adding a new edge requires updating the constitution layer table.

- ✅ **Allowed:** higher-layer crate `→` lower-layer crate (e.g. `flui-app → flui-engine → flui-painting → flui-foundation`).
- ✅ **Allowed:** sibling crates within the same layer compose only via shared lower-layer types (e.g. `flui-layer` and `flui-semantics` both depend on `flui-foundation`, not on each other unless explicitly modeled).
- ✅ **Allowed:** every crate may depend on `flui-types`, `flui-foundation`, and `flui-log` (these are infrastructural).
- ❌ **Forbidden:** lower-layer `→` higher-layer crate (e.g. `flui-painting → flui-engine`). The constitution explicitly forbids this so that `flui-painting` remains backend-agnostic.
- ❌ **Forbidden:** circular dependencies of any kind.
- ❌ **Forbidden:** widget / app code referencing `wgpu` types directly. All GPU access goes through `flui-painting`'s abstract canvas API.
- ❌ **Forbidden:** platform-specific code (Win32, AppKit, winit calls) outside `flui-platform`.
- ❌ **Forbidden:** `unsafe` outside `flui-platform`, `flui-painting`, and `flui-engine`. Every permitted `unsafe` block must carry a `// SAFETY:` comment.

Reach into private modules of another crate is impossible by construction (Rust visibility), so the boundary is enforced by the compiler — but the *shape* of the public API must still be reviewed: if a crate would need to expose internals just to compile a downstream consumer, the abstraction is wrong.

## Layer / Module Communication

### Workspace level — between crates

- **Public API surface:** each crate exposes types and traits through `lib.rs` (and optionally a `prelude` module). Internal modules are `pub(crate)` by default.
- **Trait-driven extension points:** consumers depend on traits (e.g. `Platform`, `PaintBackend`, `RenderObject`, `RenderBox<A>`), not on concrete implementations. New backends or new platforms slot in by implementing the trait.
- **Typestate over flags:** state machines that must be enforced at compile time use the typestate pattern (e.g. `BuilderContextBuilder<P, Pr>` in `flui-build`).
- **Sealed traits where appropriate:** `flui-build`'s `PlatformBuilder` is sealed (`private::Sealed`) so the supported builder set is closed. Apply this pattern when extension is intentionally framework-internal.
- **Ambassador delegation:** when a wrapper struct needs to forward a trait through a child field (e.g. `RenderPadding` forwarding `RenderObject` through `BoxChild<Single>`), use `#[delegatable_trait]` + `#[derive(Delegate)]` rather than manual forwarding.
- **Errors:** library crates return their own `Error` enum (via `thiserror`); application / CLI / build glue may use `anyhow::Error` for context-rich propagation. `anyhow` MUST NOT cross library crate boundaries.

### Runtime level — the Three-Tree Pipeline

Every frame, data flows through three tree representations in a fixed order:

1. **Build phase (`BuildOwner`)** — `View` tree (immutable configuration) is consumed by the `Element` tree (mutable state). `View::build()` MUST be pure — no I/O, no side effects, no external state mutation.
2. **Layout phase (`LayoutPhase`)** — the `Element` tree drives the `Render` tree: each `RenderBox<A>` receives `Constraints` and produces a `Size`. Layout is single-pass O(n) where possible (Flutter constraint protocol).
3. **Paint phase (`PaintPhase`)** — the `Render` tree emits paint commands into a `Canvas` (`flui-painting`) which records a `DisplayList`. The engine (`flui-engine`) tessellates the list, batches GPU work, and submits via `wgpu`.

Pipeline contract:

- The pipeline is **on-demand**: nothing runs unless a tree is dirty (`mark_needs_layout`, `mark_needs_paint`). The platform event loop uses `ControlFlow::Wait`. Polling render loops are forbidden.
- Each render lifecycle method has a strict contract: `attach(owner)` → `mark_needs_layout` → `layout(constraints)` → `perform_layout()` → `paint(context)` → `detach()`. Skipping or reordering steps is an architectural error.
- IDs that index into the trees use the **ID offset pattern**: slab indices are 0-based, public IDs are 1-based via `NonZeroUsize`. Insert with `slab_index + 1`, look up with `id.get() - 1`. Applies to `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`.
- Child arity is encoded in the type system (`Leaf`, `Single`, `Optional`, `Variable`). Mismatches become compile errors, not runtime panics.

## Key Principles

1. **Strict downward dependency DAG.** Crate edges flow only down the layer table. Circular dependencies are prohibited and any new edge must be reviewed against the constitution.
2. **Public API is curated, internals are private.** Each crate's surface is `lib.rs` (and optional `prelude`). Internal modules default to `pub(crate)`. Reviewers reject changes that expose internals just to make a consumer compile.
3. **Composition over inheritance.** Use traits + generics + enum dispatch. `dyn` only when type erasure is genuinely required (heterogeneous trees, platform abstractions).
4. **Declarative public API, imperative internals.** `View::build()` is pure; framework internals may use arena allocation, index-based access, and imperative mutation for performance.
5. **Three-tree pipeline is the runtime contract.** Build → Layout → Paint, in that order, on demand only. No phase reordering, no polling.
6. **Type-safe arity for children.** `RenderBox<A: Arity>` and `BoxChild<A>` parameters must match. Variable children use `BoxChild<Variable>`; single children use `BoxChild<Single>`.
7. **`unsafe` is confined.** Only `flui-platform`, `flui-painting`, `flui-engine` may use `unsafe`. Every block needs a `// SAFETY:` comment justifying the invariant.
8. **No cross-crate node sharing via `Arc<Mutex<>>`.** Tree storage uses arenas (`slab` with the ID offset pattern). `Arc<Mutex<>>` is reserved for legitimate shared mutable infrastructure (platform state, owners), never for nodes.
9. **Errors are typed at boundaries.** Libraries propagate `thiserror`-derived enums. Applications may collapse to `anyhow::Error`. Never leak `anyhow` across a library boundary.
10. **Logging is `tracing` only.** No `println!`, `eprintln!`, or `dbg!` in committed code. Use `#[tracing::instrument]` on hot paths and lifecycle methods.

## Code Examples

### Example 1 — Crate public API discipline

```rust
// crates/flui-rendering/src/lib.rs
//! Render tree: layout protocol, RenderBox<Arity>, paint context.

mod arity;
mod box_child;
mod render_object;
mod pipeline;
pub mod prelude;

pub use arity::{Arity, Leaf, Single, Optional, Variable};
pub use box_child::BoxChild;
pub use pipeline::PipelineOwner;
pub use render_object::{RenderBox, RenderObject};

// crates/flui-rendering/src/prelude.rs
//! Curated re-exports for downstream widget crates.
pub use crate::{Arity, BoxChild, Leaf, Optional, RenderBox, RenderObject, Single, Variable};
```

```rust
// crates/flui-widgets-example/src/lib.rs (downstream consumer)
use flui_rendering::prelude::*;
// ✅ Allowed — only the curated surface is touched.

// ❌ Forbidden — reaching past the public API:
// use flui_rendering::pipeline::internal::DirtyQueue;
```

### Example 2 — Dependency rule via trait, not concrete type

```rust
// crates/flui-painting/src/backend.rs
pub trait PaintBackend {
    fn submit(&mut self, list: &DisplayList) -> Result<()>;
}

// ✅ flui-painting depends on no GPU API and no engine type.
// flui-engine implements PaintBackend with wgpu — the dependency edge
// goes engine → painting, not the reverse.
```

```rust
// crates/flui-engine/src/wgpu/backend.rs
use flui_painting::{DisplayList, PaintBackend};

pub struct WgpuBackend { /* device, queue, pipelines */ }

impl PaintBackend for WgpuBackend {
    fn submit(&mut self, list: &DisplayList) -> Result<()> { /* ... */ Ok(()) }
}
```

### Example 3 — ID offset pattern with `NonZeroUsize`

```rust
use std::num::NonZeroUsize;
use slab::Slab;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ElementId(NonZeroUsize); // Option<ElementId> is 8 bytes

pub struct ElementTree {
    nodes: Slab<Element>,
}

impl ElementTree {
    pub fn insert(&mut self, element: Element) -> ElementId {
        let slab_index = self.nodes.insert(element);
        ElementId(NonZeroUsize::new(slab_index + 1).expect("slab_index + 1 is non-zero"))
    }

    pub fn get(&self, id: ElementId) -> Option<&Element> {
        self.nodes.get(id.0.get() - 1)
    }
}
```

### Example 4 — Type-safe arity for children

```rust
use flui_rendering::prelude::*;

pub struct RenderPadding {
    child: BoxChild<Single>, // Exactly one child enforced at compile time.
    insets: EdgeInsets,
}

pub struct RenderFlex {
    children: BoxChild<Variable>, // 0..n children.
    direction: Axis,
}

// ❌ Forbidden — losing arity to dyn / Vec leaks runtime invariants:
// pub struct RenderBad { children: Vec<Box<dyn RenderObject>> }
```

### Example 5 — Ambassador delegation through a wrapper

```rust
use ambassador::{delegatable_trait, Delegate};
use flui_rendering::{BoxChild, Single};

#[delegatable_trait]
pub trait RenderObject {
    fn mark_needs_layout(&mut self);
    fn paint(&self, context: &mut PaintContext);
}

#[derive(Delegate)]
#[delegate(RenderObject, target = "child")]
pub struct RenderPadding {
    child: BoxChild<Single>, // RenderObject methods forwarded automatically.
    insets: EdgeInsets,
}
```

### Example 6 — Logging via `tracing`, errors via `thiserror`

```rust
use thiserror::Error;
use tracing::instrument;

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("infinite constraints not supported here")]
    InfiniteConstraints,
    #[error("child {id:?} missing parent data")]
    MissingParentData { id: RenderId },
}

pub type Result<T> = std::result::Result<T, LayoutError>;

#[instrument(level = "debug", skip(self, ctx))]
pub fn layout(&mut self, ctx: &LayoutContext) -> Result<Size> {
    tracing::debug!(?self.constraints, "performing layout");
    // ... no unwrap(), no println!
    Ok(self.size)
}
```

## Anti-Patterns

- ❌ **Upward edges in the crate DAG.** A lower-layer crate depending on a higher-layer crate (`flui-painting → flui-engine`, `flui-foundation → flui-rendering`).
- ❌ **`Arc<Mutex<>>` for tree nodes.** Use arena storage (`slab`) with the ID offset pattern. `Arc<Mutex<>>` is for cross-thread infrastructure (platform state, owners), not tree topology.
- ❌ **`dyn Widget` / `Vec<Box<dyn RenderObject>>` without justification.** Prefer generics + arity types. Use `dyn` only for genuinely heterogeneous boundaries (platform abstractions, plugin loading).
- ❌ **Platform-specific imports outside `flui-platform`.** Any `windows::*`, `cocoa::*`, `winit::*`, `objc2::*` import in widget / engine / painting code is wrong.
- ❌ **`wgpu::*` references in widget or layout code.** All GPU access flows through `flui-painting`'s abstract API.
- ❌ **Polling render loops / `ControlFlow::Poll`.** Constitution mandates on-demand rendering with `ControlFlow::Wait`.
- ❌ **`unwrap()` / `expect()` outside tests, examples, and documented `// SAFETY:` invariants.** Library code returns `Result`.
- ❌ **`println!`, `eprintln!`, `dbg!` in committed code.** Always `tracing::{trace,debug,info,warn,error}!`.
- ❌ **Phase reordering or skipping.** Build / Layout / Paint must run in order. No paint without layout, no layout without build.
- ❌ **`anyhow::Error` crossing a library crate boundary.** Library APIs return their own `thiserror`-derived enum; only application / CLI / build glue may use `anyhow`.
- ❌ **Reaching into another crate's private modules** by re-exporting through trickery. If a downstream needs internals, change the public API explicitly.
- ❌ **Dart-to-Rust transliteration.** `.flutter/` is a *reference*. Patterns are adapted to Rust idioms (ownership, enums, traits) — never copied.
