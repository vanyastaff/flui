[← Architecture](architecture.md) · [Back to README](../README.md) · [Testing →](testing.md)

# Crates Map

The FLUI workspace contains 20+ crates organized into a strict layered DAG. This page is the canonical inventory: what each crate does, what layer it sits in, and whether it is currently active.

A crate marked **DISABLED** is commented out in `Cargo.toml` `[workspace.members]` while integration is in progress; the source tree still exists but is not built by default.

## Layer 0 — Foundation

Zero internal dependencies. Reused by every other crate.

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-types` | ✅ ACTIVE | Base value types, units (px, dp), ID newtypes built on `NonZeroUsize` |
| `flui-foundation` | ✅ ACTIVE | Geometry (`Size`, `Rect`, `Offset`), color, text style, common error helpers |

## Layer 1 — Tree Primitives

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-tree` | ✅ ACTIVE | Generic tree abstractions, visitor patterns, build / diff / reconcile primitives |

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

## Layer 5 — Engine / Platform / Hot-Reload / Logging

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-engine` | ✅ ACTIVE | GPU pipeline (build → layout → paint → composite). Owns all `wgpu` state. |
| `flui-platform` | ✅ ACTIVE | Native Win32 / AppKit / Headless backends + `winit` fallback. Sole home of OS-specific code. |
| `flui-hot-reload` | ✅ ACTIVE | `dlopen`-based scene plugin host for desktop iteration |
| `flui-log` | ✅ ACTIVE | `tracing` setup helpers, Android logging layer |

## Layer 6 — View / Assets / Build

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-view` | ✅ ACTIVE | View + Element tree, `BuildContext`, view trait |
| `flui-assets` | ⏸️ DISABLED | Asset loading, caching, image decoding |
| `flui-build` | ⏸️ DISABLED | Async cross-platform build pipeline (`PlatformBuilder` typestate) |

## Layer 7 — Application / CLI / DevTools

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

- [Architecture](architecture.md) — three-tree pipeline + layered DAG
- [Getting Started](getting-started.md) — build and run instructions
- [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) — full architectural rules
- [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — constitution v2.2.0
