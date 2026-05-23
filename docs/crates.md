[← Port](PORT.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md) · [Testing →](testing.md)

# Crates Map

> **Scope.** This page describes the **current** workspace as it is built today. The **target** crate decomposition (incl. `flui-geometry`, `flui-widgets`, `flui-material`, `flui-cupertino`, `flui-localizations`, the `flui-log` merge into `flui-foundation`) is defined in [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md); the migration is sequenced in [`ROADMAP.md`](ROADMAP.md).

The FLUI workspace contains 20+ crates organized into a strict layered DAG. This page is the canonical inventory: what each crate does, what layer it sits in, and whether it is currently active.

A crate marked **DISABLED** is commented out in `Cargo.toml` `[workspace.members]` while integration is in progress; the source tree still exists but is not built by default.

## Layer 0 — Foundation (value types)

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-types` | ✅ ACTIVE | Base value types and units (px, dp); geometry (`Point`, `Rect`, `Size`, `Offset`, `Matrix4`, Bézier, superellipse); styling (colors, paint values); typography; layout enums; gestures; physics value types; platform value types. **ID newtypes** (`ElementId`, `RenderId`, `LayerId`, etc. — all `NonZeroUsize`-backed) live in `flui-foundation`, not here. |

## Layer 1 — Framework primitives + Tree primitives

`flui-foundation` operates on top of `flui-types`' value types — its responsibility (framework primitives like notifiers and bindings) is above raw value types, and `crates/flui-foundation/Cargo.toml` declares the `flui-types` dependency edge accordingly. See `Note on flui-foundation placement` in [Architecture](architecture.md).

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-foundation` | ✅ ACTIVE | Framework primitives: `ChangeNotifier` / `Listenable`, `Id` system, `BindingBase`, `Key`, diagnostics, error helpers |
| `flui-tree` | ✅ ACTIVE | Generic tree abstractions: `TreeRead` / `TreeNav` / `TreeWrite` trio, iterators / slots, arity markers (`Leaf` / `Single` / `Optional` / `Variable`), depth markers. The Cycle-3 audit deleted speculative `visitor` / `diff` modules (~10k LOC zombie surface) — concrete trees adopt the trio directly |

## Layer 2 — Reactivity

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-reactivity` | ⏸️ DISABLED | Signals, hooks, computed values, batched updates |

## Layer 3 — Painting / Layer / Semantics / Interaction

These crates compose the rendering substrate without knowing about each other.

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-painting` | ✅ ACTIVE | `Canvas` API, `DisplayList`, paths, paint commands, text recording |
| `flui-layer` | ✅ ACTIVE | Layer composition tree (compositor input) |
| `flui-semantics` | ✅ ACTIVE | Accessibility tree (semantics nodes, focus, labels) |
| `flui-interaction` | ✅ ACTIVE | Hit-testing, gestures, focus, pointer events |

## Layer 4 — Scheduling / Rendering / Animation

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-scheduler` | ✅ ACTIVE | Frame scheduling, microtasks, task prioritization |
| `flui-rendering` | ✅ ACTIVE | `RenderObject`, `RenderBox<Arity>`, layout protocol, paint context |
| `flui-animation` | ⏸️ DISABLED | Curves, tweens, controllers, implicit animations |

## Layer 5 — Engine / Platform / Logging

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-engine` | ✅ ACTIVE | GPU pipeline (build → layout → paint → composite). Owns all `wgpu` state. |
| `flui-platform` | ✅ ACTIVE | Native Win32 / AppKit / Headless backends + `winit` fallback. Sole home of OS-specific code. |
| `flui-log` | ✅ ACTIVE | `tracing` setup helpers, Android logging layer |

## Layer 6 — View / Assets / Build

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-view` | ✅ ACTIVE | View + Element tree, `BuildContext`, view trait |
| `flui-assets` | ⏸️ DISABLED | Asset loading, caching, image decoding |
| `flui-build` | ⏸️ DISABLED | Async cross-platform build pipeline (`PlatformBuilder` typestate) |

## Layer 7 — Hot-Reload

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-hot-reload` | ✅ ACTIVE | `dlopen`-based scene plugin host for desktop iteration. Optional `app-plugin` feature depends on `flui-view`, `flui-rendering`, `flui-types` — placing this crate above `flui-view` in the DAG. |

## Layer 8 — Application / CLI / DevTools

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-app` | ✅ ACTIVE (migration) | App runner, root widget, application lifecycle |
| `flui-cli` | ⏸️ DISABLED | CLI tooling (`flui new`, `flui build`, `flui run`) |
| `flui-devtools` | ⏸️ DISABLED | Inspector, widget tree viewer, perf overlay |

## Examples and Tools

| Member | Status | Purpose |
|--------|--------|---------|
| `examples/desktop_scene` | ✅ ACTIVE | Hot-reload-aware desktop scene plugin |
| `examples/web_demo` | ✅ ACTIVE (manual build) | Web/WASM platform demo (`cdylib`) |
| `examples/painting_demo` | ✅ ACTIVE (manual build) | Web/WASM painting + engine demo (`cdylib`) |
| `examples/android_app` | ⛔ Excluded from workspace | Widget-based hot-reloadable Android plugin (requires NDK) |
| `examples/android_demo` | ⛔ Excluded from workspace | Android GPU demo (requires NDK) |
| `examples/android_scene` | ⛔ Excluded from workspace | Hot-reloadable Android scene plugin (requires NDK) |
| `tools/web-server` | ✅ ACTIVE | Built-in web dev server (wasm-pack + HTTP serve) |

## Build Order

The workspace builds bottom-up automatically. For manual incremental builds:

```bash
cargo build -p flui-types
cargo build -p flui-foundation
cargo build -p flui-tree
cargo build -p flui-platform
# ... continue up the layers
cargo build -p flui-app
```

## Adding a New Crate

1. Decide its layer based on what it depends on. Lower-layer crates must not depend on higher-layer ones.
2. Add the directory under `crates/<flui-name>/` with a standard layout (`Cargo.toml`, `src/lib.rs`, `src/error.rs`).
3. Add the path to `[workspace.members]` and `default-members` in the root `Cargo.toml`.
4. Update the constitution layer table in `.specify/memory/constitution.md` if it represents a new responsibility.
5. Update this page (`docs/crates.md`) and the directory tree in `AGENTS.md`.

## See Also

- [Foundations](FOUNDATIONS.md) — architecture contract, target crate graph
- [Roadmap](ROADMAP.md) — construction phases from current to target
- [Architecture](architecture.md) — three-tree pipeline + layered DAG (current state)
- [Getting Started](getting-started.md) — build and run instructions
- [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) — full architectural rules
- [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — constitution v2.3.0
