[← Getting Started](getting-started.md) · [Back to README](../README.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Port →](PORT.md)

# Architecture

FLUI combines two patterns: a **Layered Modular Workspace** (workspace structure) and a **Three-Tree Pipeline** (runtime data flow). The first tells you *what may depend on what*; the second tells you *how a frame is built, laid out, and painted*.

For the deep, rule-by-rule guide (anti-patterns, code examples, dependency rules), read [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md). This page is the high-level orientation.

## Layered Modular Workspace

20+ crates are organized into a strict directed acyclic graph (DAG). Dependencies flow downward only; circular dependencies are forbidden. Each crate exposes its public API exclusively through `lib.rs` (and an optional `prelude` module). Internal modules default to `pub(crate)`.

```
Layer 8  ── flui-app, flui-cli, flui-devtools
Layer 7  ── flui-hot-reload   (depends on flui-view via `app-plugin` feature)
                │
Layer 6  ── flui-view, flui-objects, flui-widgets, flui-binding,
                │  flui-build, flui-assets
                │
Layer 5  ── flui-engine, flui-platform
                │
Layer 4  ── flui-scheduler, flui-rendering, flui-animation
                │
Layer 3  ── flui-painting, flui-layer, flui-semantics, flui-interaction
                │
Layer 2  ── flui-reactivity*
                │
Layer 1  ── flui-tree, flui-foundation, flui-macros
                │   (flui-foundation = framework primitives:
                │    ChangeNotifier, Id system, BindingBase, Key, diagnostics)
Layer 0  ── flui-geometry, flui-types
                (geometry, styling, typography, layout, gestures, physics,
                 platform value types; base units)
```

Note on `flui-foundation` placement: in the current workspace its Cargo deps are leaf (no internal-crate runtime deps), but its *responsibility* is framework primitives that operate on top of `flui-types`' value types — so it is placed above `flui-types` in the layered table. The target crate graph in [`FOUNDATIONS.md`](FOUNDATIONS.md) Part IV makes that placement an enforced edge.

See [`crates.md`](crates.md) for the full inventory and current status of each crate.

### Why this structure?

- **The compiler enforces the layout.** Cargo prevents an upward edge at build time, not at review time.
- **Public API discipline scales.** A consumer cannot reach into another crate's internals because they are `pub(crate)`. Reviewers reject changes that expose internals "just to make it compile" — that is the signal an abstraction is wrong.
- **Backends slot in via traits.** `Platform`, `PaintBackend`, `RenderBox<A>`, and similar are extension points. Implementations live in dedicated crates, not in widget code.

## Three-Tree Pipeline

Every frame, data flows through three trees in a fixed order:

```
View Tree        ──build──▶   Element Tree   ──layout──▶   Render Tree   ──paint──▶  Layer Tree  ──submit──▶  GPU
(immutable)                   (mutable state)              (RenderBox<A>)             (composition)            (wgpu)
```

| Phase | Owner | Input | Output | Constraint |
|-------|-------|-------|--------|------------|
| Build | `BuildOwner` | dirty `View` nodes | reconciled `Element` tree | `View::build()` is pure — no I/O, no external mutation |
| Layout | `LayoutPhase` | `Constraints` | `Size` per `RenderBox` | Single-pass O(n) where possible (Flutter constraint protocol) |
| Paint | `PaintPhase` | `RenderBox` tree | `DisplayList` → layers | Recording is in `flui-painting`; GPU submission in `flui-engine` |

The pipeline is **on-demand**. The platform event loop uses `ControlFlow::Wait`. Nothing runs unless a tree is dirty (`mark_needs_layout`, `mark_needs_paint`). Polling render loops are forbidden by the constitution.

## Type-Safe Children: the Arity System

Render children are parameterized by `Arity`. Mismatches become compile errors, not runtime panics.

| Arity | Children | Used by |
|-------|----------|---------|
| `Leaf` | 0 | Text, Image |
| `Single` | exactly 1 | Center, Padding |
| `Optional` | 0 or 1 | Container |
| `Variable` | 0..n | Row, Column, Stack |

```rust
pub struct RenderPadding {
    child: BoxChild<Single>,    // exactly one child
}

pub struct RenderFlex {
    children: BoxChild<Variable>, // 0..n
}
```

Trait forwarding through wrappers uses the [`ambassador`](https://docs.rs/ambassador) crate (`#[delegatable_trait]` + `#[derive(Delegate)]`) — never manual boilerplate.

## ID Offset Pattern

Slab-based storage uses 0-based indices internally; all public IDs are 1-based via `NonZeroUsize`. This makes `Option<ElementId>` 8 bytes (niche optimization) and turns "missing parent" bugs into compile errors.

```rust
let slab_index = self.nodes.insert(node);
let id = ElementId::new(slab_index + 1).unwrap();
self.nodes.get(id.get() - 1);
```

Applies to: `ViewId`, `ElementId`, `RenderId`, `LayerId`, `SemanticsId`.

## Platform Abstraction

`flui-platform` exposes a unified `Platform` trait with native and headless backends:

```rust
pub trait Platform: Send + Sync + 'static {
    // Core
    fn background_executor(&self) -> Arc<dyn PlatformExecutor>;
    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor>;

    // Lifecycle
    fn run(self: Box<Self>, on_ready: Box<dyn FnOnce()>);
    fn quit(&self);

    // Windows + displays
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>>;
    fn active_window(&self) -> Option<WindowId>;
    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>>;

    // Input
    fn clipboard(&self) -> Arc<dyn Clipboard>;

    // Callbacks + metadata
    fn on_quit(&self, callback: Box<dyn FnMut() + Send>);
    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>);
    fn capabilities(&self) -> &dyn PlatformCapabilities;
    fn name(&self) -> &'static str;
    // ... plus optional methods for cursor, file pickers, app activation, etc.
}

let platform = current_platform().expect("failed to initialize platform");
```

Backends: `WindowsPlatform` (Win32), `MacOSPlatform` (AppKit), `HeadlessPlatform` (CI / tests), and a `winit` fallback. All platform-specific imports (`windows::*`, `cocoa::*`, `winit::*`) are confined to this crate.

Text shaping is **not** a `Platform` method — that Flutter binding (`PlatformTextSystem`) was deleted under the [binding-deletion carve-out in `PORT.md`](PORT.md#flutter-behaviour-primacy-with-binding-deletion-carve-out); `cosmic-text` + `glyphon` (+ future `flui-assets`) cover the responsibility end-to-end.

## Confinement of `unsafe`

`unsafe` is permitted **only** in `flui-platform`, `flui-painting`, and `flui-engine`. Every `unsafe` block carries a `// SAFETY:` comment justifying the invariant. Widget and application code must remain `unsafe`-free.

## Logging and Errors

- **Logging:** `tracing` only — never `println!`, `eprintln!`, or `dbg!`. Use `#[tracing::instrument]` on hot paths and lifecycle methods.
- **Errors:** library crates use `thiserror` and expose typed enums. Application / CLI / build glue may use `anyhow::Error`. `anyhow` MUST NOT cross a library crate boundary.

## Reference Sources

FLUI is designed against two external codebases for read-only architectural reference:

- Flutter framework source (UI architecture, widget patterns, layout algorithms).
- GPUI Rust UI library (platform abstraction, callback registries, type erasure patterns).

Maintainer checkouts may include local `.flutter/` and `.gpui/` mirrors for parity work, but those external source trees are not required for normal builds. Both references are studied, never copied. Patterns are translated to FLUI idioms (Arity, Ambassador delegation, no nullability, strict layered DAG).

## Hot Reload (Dev-Time)

Hot-reload is split into two layers so build tooling and runtime hosts stay decoupled:

1. **Build orchestration** — `SourceWatcher` in `flui-hot-reload` (`source-watch` feature) watches `src/` and triggers `cargo build`. Used by `flui-cli` and `flui-devtools`.
2. **Artifact reload** — `HotReloadDriver` polls the plugin `.so`/`.dll` mtime and reloads via `dlopen` without restarting the host.

See [Hot Reload](hot-reload.md) for workflows, `ReloadStrategy`, and integration examples.

## See Also

- [Hot Reload](hot-reload.md) — two-layer dev model, plugin workflows
- [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) — full architectural rules and anti-patterns
- [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — constitution v2.3.0
- [Foundations](FOUNDATIONS.md) — architecture contract, target crate graph
- [Roadmap](ROADMAP.md) — construction phases from current to target
- [Crates Map](crates.md) — per-layer crate inventory
- [Contributing](contributing.md) — workflow and conventions
