[← Port](PORT.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md) · [Testing →](testing.md)

# Crates Map

> **Scope.** This page describes the **current** workspace as it is built today. `flui-localizations`, `flui-material`, and `flui-cupertino` (Catalog.1) have landed; the remaining target crate decomposition — the formal `flui` facade — is defined in [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md); the migration is sequenced in [`ROADMAP.md`](ROADMAP.md).

The FLUI workspace contains 20+ crates organized into a strict layered DAG. This page is the canonical inventory: what each crate does, what layer it sits in, and whether it is currently active.

A crate marked **DISABLED** is commented out in `Cargo.toml` `[workspace.members]` while integration is in progress; the source tree still exists but is not built by default. A crate may be active but omitted from `default-members`; `cargo build --workspace` still includes every active workspace member.

## Layer 0 — Foundation (value types)

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-geometry` | ✅ ACTIVE | Geometry primitives and unit-safe coordinate spaces (`Point`, `Rect`, `Size`, `Offset`, `Matrix4`, Bézier, superellipse), re-exported by `flui-types` |
| `flui-types` | ✅ ACTIVE | Base value types and units (px, dp); styling (colors, paint values); typography; layout enums; gestures; physics value types; platform value types. **ID newtypes** (`ElementId`, `RenderId`, `LayerId`, etc. — all `NonZeroUsize`-backed) live in `flui-foundation`, not here. |

## Layer 1 — Framework primitives + Tree primitives

`flui-foundation` is responsible for framework primitives above raw value types, but its current runtime manifest intentionally stays leaf-like: `flui-types` is a dev-dependency only. See `Note on flui-foundation placement` in [Architecture](architecture.md).

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-foundation` | ✅ ACTIVE | Framework primitives: `ChangeNotifier` / `Listenable`, `Id` system, `BindingBase`, `Key`, diagnostics, error helpers |
| `flui-macros` | ✅ ACTIVE | Proc-macro crate for framework derives and generated boilerplate |
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
| `flui-animation` | ✅ ACTIVE | Curves, tweens, controllers, implicit animations (re-enabled for the Core.1 transition widgets) |

## Layer 5 — Engine / Platform

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-engine` | ✅ ACTIVE | GPU pipeline (build → layout → paint → composite). Owns all `wgpu` state. |
| `flui-platform` | ✅ ACTIVE | Native Win32 / AppKit / Headless backends + `winit` fallback. Sole home of OS-specific code. |

## Layer 6 — View / Assets / Build

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-view` | ✅ ACTIVE | View + Element tree, `BuildContext`, view trait |
| `flui-objects` | ✅ ACTIVE | Concrete `RenderBox` / `RenderSliver` catalog that `flui-widgets` wraps |
| `flui-widgets` | ✅ ACTIVE | User-facing Flutter-style widget catalog (configuration objects over `flui-objects`); owns the `Localizations`/`Directionality`/`WidgetsLocalizations` ambient-theming and localization substrate |
| `flui-localizations` | ✅ ACTIVE | Global (multi-language) localized resources — `GlobalWidgetsLocalizations`, the analog of Flutter's `flutter_localizations`. Depends on `flui-widgets` (implements its `LocalizationsDelegate`/`WidgetsLocalizations` traits) |
| `flui-material` | ✅ ACTIVE | Material Design theming foundation — `ColorScheme`, `Typography`/`TextTheme`, `ThemeData`, and the `Theme` inherited widget (constants-first M3 baseline; `fromSeed` deferred). Depends on `flui-widgets` (implements its `InheritedTheme` trait) |
| `flui-cupertino` | ✅ ACTIVE | iOS-style (Cupertino) theming foundation — `CupertinoDynamicColor`/`CupertinoColors`, `CupertinoTextThemeData`, `CupertinoThemeData`, the `CupertinoTheme` inherited widget, and `CupertinoButton` (constants-first V1; brightness-only dynamic-color resolution, one component). Depends on `flui-widgets` (implements its `InheritedTheme` trait); independent sibling of `flui-material` (ADR-0028 — neither depends on the other) |
| `flui-binding` | ✅ ACTIVE | Deterministic non-singleton headless frame driver: `HeadlessBinding::pump_frame(dt)` advances a virtual `ManualClock` and polls clock-bound gesture-arena deadlines — sleep-free time-based gesture tests (long-press, double-tap). Animation-controller ticks (Phase 3) and tree-rebuild integration (Phase 1b) are deferred. |
| `flui-assets` | ✅ ACTIVE | Asset loading, caching, image decoding |
| `flui-build` | ✅ ACTIVE | Async cross-platform build pipeline (`PlatformBuilder` typestate) |

## Layer 7 — Hot-Reload

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-hot-reload` | ✅ ACTIVE | Two-layer hot-reload: runtime `HotReloadDriver` (layer 2, dlopen) + optional `SourceWatcher` (layer 1, `source-watch` feature). See [hot-reload.md](hot-reload.md). |

## Layer 8 — Application / CLI / DevTools

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-app` | ✅ ACTIVE (migration) | App runner, root widget, application lifecycle |
| `flui-cli` | ✅ ACTIVE | CLI tooling (`flui run` hot-reload orchestration, Android scene deploy) |
| `flui-devtools` | ✅ ACTIVE (partial) | Profiler; `HotReloader` delegates to `flui-hot-reload` |

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
cargo build -p flui-geometry
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
3. Add the path to `[workspace.members]` in the root `Cargo.toml`; add it to `default-members` unless the crate is intentionally excluded from default local builds.
4. Update the layer table if it represents a new responsibility — in the constitution (`.specify/memory/constitution.md`) once restored, and meanwhile in [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md#part-iv--the-target-crate-decomposition), the de-facto layer-table source while the constitution file is absent from the repo.
5. Update this page (`docs/crates.md`) and the directory tree in `AGENTS.md`.

## See Also

- [Foundations](FOUNDATIONS.md) — architecture contract, target crate graph
- [Roadmap](ROADMAP.md) — construction phases from current to target
- [Architecture](architecture.md) — three-tree pipeline + layered DAG (current state)
- [Getting Started](getting-started.md) — build and run instructions
- [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) — full architectural rules
- [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — constitution (last ratified v2.3.0; ⚠ file currently absent from the repo — pending maintainer restore. Layer table: see [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md#part-iv--the-target-crate-decomposition))
