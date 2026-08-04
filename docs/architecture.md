[← Getting Started](getting-started.md) · [Back to README](../README.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Port →](PORT.md)

# Architecture

FLUI combines two patterns: a **Layered Modular Workspace** (workspace structure) and a **Three-Tree Pipeline** (runtime data flow). The first tells you *what may depend on what*; the second tells you *how a frame is built, laid out, and painted*.

For the deep, rule-by-rule guide (anti-patterns, code examples, dependency rules), read [`FOUNDATIONS.md`](FOUNDATIONS.md) (`.ai-factory/ARCHITECTURE.md` does not exist in this checkout). This page is the high-level orientation.

## Layered Modular Workspace

20+ crates are organized into a strict directed acyclic graph (DAG). Dependencies flow downward only; circular dependencies are forbidden. Each crate exposes its public API exclusively through `lib.rs` (and an optional `prelude` module). Internal modules default to `pub(crate)`.

```
Layer 10 ── flui                       (facade)
                │
Layer 9  ── flui-app, flui-devtools, flui-cli
                │
Layer 8  ── flui-localizations         (implements the catalogs' delegate contracts)
                │
Layer 7  ── flui-material, flui-cupertino
                │
Layer 6  ── flui-widgets, flui-testing, flui-hot-reload, flui-build
                │
Layer 5  ── flui-view
                │
Layer 4  ── flui-engine, flui-rendering, flui-objects
                │   (objects → rendering, never the reverse)
Layer 3  ── flui-layer, flui-semantics, flui-animation
                │
Layer 2  ── flui-tree, flui-platform, flui-scheduler, flui-painting,
                │  flui-interaction, flui-assets
                │  (interaction → platform, never the reverse)
Layer 1  ── flui-foundation, flui-macros
                │   (flui-foundation = framework primitives:
                │    ChangeNotifier, Id system, BindingBase, Key, diagnostics)
Layer 0  ── flui-geometry, flui-types
                (geometry, styling, typography, layout, gestures, physics,
                 platform value types; base units)
```

**This is not enforced by convention.** [`workspace-layers.toml`](workspace-layers.toml) is the authoritative policy, and `scripts/check-workspace-inventory.sh` (`just inventory-check`, part of `just ci` and the CI `checks` job) validates every **normal** Cargo edge against it: strictly downward unless the ordered pair is an explicit same-layer exemption, no forbidden pairs, acyclic including projected future edges, and every member classified. See [ADR-0041](adr/ADR-0041-workspace-topology-contract.md). Dev-dependencies are out of scope and cross layers freely — a test fixture is not an architectural claim.

Note on `flui-foundation` placement: in the current workspace its Cargo deps are leaf (no internal-crate runtime deps), but its *responsibility* is framework primitives that operate on top of `flui-types`' value types — so it is placed above `flui-types` in the layered table. The target crate graph in [`FOUNDATIONS.md`](FOUNDATIONS.md) Part IV draws that placement as a dashed (not-yet-real) edge.

See [`crates.md`](crates.md) for the full inventory and current status of each crate.

### Why this structure?

- **Tooling enforces the layout, not review.** Cargo rejects a dependency *cycle* at build time, but an upward edge that does not close a cycle (`flui-rendering → flui-devtools`, say) builds fine — which is how three placements in this diagram drifted from the code unnoticed. `just inventory-check` closes that gap by comparing the declared layer policy against Cargo's normal edges.
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

### Threading & ownership model

The canonical threading/ownership record is [ADR-0027](adr/ADR-0027-owner-affine-ui-realms.md): a multi-threaded runtime of single-writer ownership domains — per-session `UiRealm` (`!Send + !Sync` owner), bounded typed mailboxes committed at Idle, and an owned `SceneSnapshot` handoff to a single-owner raster seam. It supersedes [ADR-0002](adr/ADR-0002-engine-wide-threading-architecture.md).

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
- [Foundations](FOUNDATIONS.md) — architecture contract, target crate graph, full anti-pattern list
- [`AGENTS.md`](../AGENTS.md) — the current cross-tool rules (`.ai-factory/ARCHITECTURE.md` and `.specify/memory/constitution.md` were the historical originals; neither exists in this checkout)
- [Roadmap](ROADMAP.md) — construction phases from current to target
- [Crates Map](crates.md) — per-layer crate inventory
- [Contributing](contributing.md) — workflow and conventions
