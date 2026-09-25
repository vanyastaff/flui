[← Architecture](architecture.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md) · [Testing →](testing.md)

# Crates Map

> **Scope.** This page describes the **current** workspace as it is built today. `flui-localizations`, `flui-material`, and `flui-cupertino` (Catalog.1) have landed; the remaining target crate decomposition — the formal `flui` facade — is defined in [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md); the migration is sequenced in [`ROADMAP.md`](ROADMAP.md).

The FLUI workspace contains 28 crates plus the `flui` facade, organized into a strict layered DAG. This page is the canonical inventory: what each crate does, what layer it sits in, and whether it is currently active.

> **Layer assignments here mirror the manifests, which are the authority.** Each crate declares `[package.metadata.flui] layer = N`; the root `Cargo.toml` names the layers in `[workspace.metadata.flui] layers`. `cargo xtask workspace` checks every **normal** and build dependency between workspace packages: it points to the same layer or lower, never higher, and never at an example or tool (Cargo itself rejects cycles). See [ADR-0041](adr/ADR-0041-workspace-topology-contract.md) for the contract and [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md) for the target graph. Dev-dependencies may point up (tests use `flui-testing`) and Cargo permits cycles among them. A crate can narrow who depends on it: `allowed-dependents` (normal and build edges) and `allowed-dev-dependents` in its `[package.metadata.flui]`, which is how `flui-log` stays composition-only and how no crate but `flui-localizations`, `flui-app` and the facade depends on Material or Cupertino in any form ([ADR-0028](adr/ADR-0028-design-system-decoupling-contract.md)). Examples and tools are applications and may depend on anything.

A crate marked **DISABLED** is commented out in `Cargo.toml` `[workspace.members]` while integration is in progress; the source tree still exists but is not built by default.

## Layer 0 — Foundation (value types)

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-geometry` | ✅ ACTIVE | Geometry primitives and unit-safe coordinate spaces (`Point`, `Rect`, `Size`, `Offset`, `Matrix4`, Bézier, superellipse), re-exported by `flui-types` |
| `flui-types` | ✅ ACTIVE | Base value types and units (px, dp); styling (colors, paint values); typography; layout enums; gestures; physics value types; platform value types. **ID newtypes** (`ElementId`, `RenderId`, `LayerId`, etc. — all `NonZeroUsize`-backed) live in `flui-foundation`, not here. |

## Layer 1 — Framework primitives

`flui-foundation` is responsible for framework primitives above raw value types, but its current runtime manifest intentionally stays leaf-like: `flui-types` is a dev-dependency only. See `Note on flui-foundation placement` in [Architecture](architecture.md).

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-foundation` | ✅ ACTIVE | Framework primitives: `ChangeNotifier` / `Listenable`, `Id` system, `Key`, the `observe` tree-observer seam (ADR-0040) + `RebuildReason`, the shared structured-field vocabulary in `diagnostics`, error helpers. There is no `BindingBase` — the ambient-singleton machinery it backed was retired workspace-wide. Emits `tracing` events; owns no subscriber. |
| `flui-macros` | ✅ ACTIVE | Proc-macro crate for framework derives and generated boilerplate |

## Layer 2 — Substrate

These crates compose the rendering and platform substrate largely without knowing about each other. The one intra-layer edge is `flui-interaction → flui-platform` ([ADR-0037](adr/ADR-0037-presentation-ownership-domains.md)): interaction owns the owner-local text-input state and names the OS-facing capability platform defines.

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-log` | ✅ ACTIVE | Composition-only cross-platform logging backend: desktop `fmt` (optionally hierarchical), Android logcat, Apple unified logging, browser console/performance timeline, behind an explicit subscriber-ownership policy. **No framework crate but `flui-app`, `flui-cli` and the facade may link it** (examples, being applications, may) — framework crates use `tracing` directly, and its manifest's `allowed-dependents` (checked by `cargo xtask workspace`) enforces that mechanically. |
| `flui-tree` | ✅ ACTIVE | Generic tree abstractions: `TreeRead` / `TreeNav` / `TreeWrite` trio, iterators / slots, arity markers (`Leaf` / `Single` / `Optional` / `Variable`), depth markers. A workspace audit deleted the unused speculative `visitor` / `diff` modules; concrete trees adopt the trio directly. |
| `flui-platform` | ✅ ACTIVE | Native Win32 / AppKit / Headless backends + `winit` fallback. Sole home of OS-specific code. Loses `BackgroundExecutor`/`PlatformExecutor` when host-injected runtime execution lands. |
| `flui-scheduler` | ✅ ACTIVE | Frame scheduling, microtasks, task prioritization. Narrows to logical update phases, tickers, callback ordering, and owner-local post-frame behavior; presentation clocks and raster backpressure move to presentation/runtime ownership. |
| `flui-painting` | ✅ ACTIVE | `Canvas` API, `DisplayList`, paths, paint commands, text recording |
| `flui-interaction` | ✅ ACTIVE | Hit-testing, gestures, focus, pointer events, owner-local text input |
| `flui-assets` | ✅ ACTIVE | Asset loading, caching, image decoding |

## Layer 3 — Compositing / a11y / animation

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-layer` | ✅ ACTIVE | Layer composition tree (compositor input) |
| `flui-semantics` | ✅ ACTIVE | Accessibility tree (semantics nodes, focus, labels) |
| `flui-animation` | ✅ ACTIVE | Curves, tweens, controllers, implicit animations (re-enabled for the Core.1 transition widgets) |

## Layer 4 — Render machine + render catalog

`flui-objects` sits above `flui-rendering` inside this layer and strictly below `flui-view`: the real production graph is `flui-rendering ← flui-objects ← flui-view ← flui-widgets`. The `flui-objects → flui-rendering` edge is a sanctioned same-layer exemption; the inverse (`flui-rendering → flui-objects`) stays forbidden.

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-engine` | ✅ ACTIVE | GPU pipeline (build → layout → paint → composite). Owns all `wgpu` state. |
| `flui-rendering` | ✅ ACTIVE | `RenderObject`, `RenderBox<Arity>`, layout protocol, paint context |
| `flui-objects` | ✅ ACTIVE | Concrete `RenderBox` / `RenderSliver` catalog. Wrapped by `flui-widgets`, and named directly by `flui-view` for framework machinery whose element and render halves cooperate (`RenderLayoutBuilder`, `RenderSliverList`, `RenderSliverGrid`) |

## Layer 5 — Framework spine

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-view` | ✅ ACTIVE | View + Element tree, `BuildContext`, view trait |

## Layer 6 — Widget catalog + DX tooling

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-widgets` | ✅ ACTIVE | User-facing Flutter-style widget catalog (configuration objects over `flui-objects`); owns the `Localizations`/`Directionality`/`WidgetsLocalizations` ambient-theming and localization substrate |
| `flui-testing` | ✅ ACTIVE | Deterministic non-singleton headless frame driver: `HeadlessBinding::pump_frame(dt)` advances a virtual `ManualClock` and polls clock-bound gesture-arena deadlines — sleep-free time-based gesture tests (long-press, double-tap). It is the workspace's **test-support** package: `WidgetTester`, virtual time, fake platform capabilities, deterministic replay, and golden helpers belong here as they land. Runtime and framework crates take *development* edges into it only. |
| `flui-hot-reload` | ✅ ACTIVE | Runtime half of hot reload: `HotReloadDriver`, `DynLib`, worker/host ABI (dlopen). The dev-time watcher lives in `flui-cli` |

## Layer 7 — Design systems

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-material` | ✅ ACTIVE | Material Design theming foundation — `ColorScheme`, `Typography`/`TextTheme`, `ThemeData`, and the `Theme` inherited widget (constants-first M3 baseline; `fromSeed` deferred). Depends on `flui-widgets` (implements its `InheritedTheme` trait) |
| `flui-cupertino` | ✅ ACTIVE | iOS-style (Cupertino) theming foundation — `CupertinoDynamicColor`/`CupertinoColors`, `CupertinoTextThemeData`, `CupertinoThemeData`, the `CupertinoTheme` inherited widget, and `CupertinoButton` (constants-first V1; brightness-only dynamic-color resolution, one component). Depends on `flui-widgets` (implements its `InheritedTheme` trait); independent sibling of `flui-material` (ADR-0028 — neither depends on the other) |

Neither design system may depend on `flui-localizations` — that direction is a `[[forbidden_edge]]` in the layer policy. See Layer 8.

## Layer 8 — Global localizations

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-localizations` | ✅ ACTIVE | Global (multi-language) localized resources — `GlobalWidgetsLocalizations`, the analog of Flutter's `flutter_localizations`. It is the **implementation** package: it depends on the catalogs that *define* the contracts it implements (`flui-widgets` today; `flui-material`/`flui-cupertino` once `GlobalMaterialLocalizations`/`GlobalCupertinoLocalizations` land), never the reverse. That is why it sits above the design systems. |

## Layer 9 — Application / tooling

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-app` | ✅ ACTIVE (migration) | App runner, root widget, application lifecycle. The **private composition root** for runtime ownership during Runtime.1 — a `flui-runtime` crate is not extracted from it until two entry points prove the boundary ([ADR-0041](adr/ADR-0041-workspace-topology-contract.md)). Owns **no design tokens** ([ADR-0042](adr/ADR-0042-theming-ownership.md)); hot reload is behind its optional `hot-reload` feature. |
| `flui-cli` | ✅ ACTIVE | The `flui` CLI: `create`/`run` (hot reload with hot-keys)/`build`/`doctor`/`devices`/`emulators`, one output policy (`--json`, `--quiet`, `--non-interactive`) and a documented exit-code table. The per-target build pipeline (Android/iOS/desktop/web) lives in its own `src/build/` module; depends on `flui-hot-reload`; no edge to `flui-devtools`. |
| `flui-devtools` | ✅ ACTIVE (partial) | Profiler, timeline, inspector counters |

## Layer 10 — Facade

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui` | ✅ ACTIVE | The root package / app-author facade. Feature-selective: `material` (default), `cupertino`, `localizations`, `hot-reload`. A module whose feature is off is absent, not empty. Every supported combination is compiled in isolation by `cargo xtask facade-combos` (run by the CI feature-matrix job), so a combination cannot pass through workspace feature unification. |

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
| `tools/desktop-mcp` | ✅ ACTIVE | MCP server that drives desktop apps from the outside (windows, screenshots, UI Automation, real input) for agent testing |
| `tools/device-checks` | ✅ ACTIVE (scripts, not a crate) | The macOS and iOS device-check scripts `cargo xtask device` runs; the Windows gates live in `tools/xtask/src/device/` |
| `tools/live-smoke` | ✅ ACTIVE | Real X11/Wayland input smoke behind `cargo xtask live-smoke` |

## Build Order

The workspace builds bottom-up automatically. For manual incremental builds:

```bash
cargo build -p flui-geometry
cargo build -p flui-types
cargo build -p flui-foundation
cargo build -p flui-log
cargo build -p flui-tree
cargo build -p flui-platform
# ... continue up the layers
cargo build -p flui-app
```

## Adding a New Crate

A new crate is a topology change, so it starts with the contract, not the directory: a crate is a layer, not a feature ([ADR-0041](adr/ADR-0041-workspace-topology-contract.md)). A crate created before its second consumer exists freezes a guessed boundary — `flui-runtime`, for example, waits for two entry points driving the same proven core plus a measurable dependency reduction.

1. Decide its layer from what it depends on: a dependency points to the same layer or lower, never higher.
2. Add the directory under `crates/<flui-name>/` with a standard layout (`Cargo.toml`, `src/lib.rs`, `src/error.rs`). The manifest inherits the shared `[workspace.package]` keys and the workspace lints, and declares `[package.metadata.flui] layer = N` (plus `wasm = false`, with the reason beside it, if it cannot build for wasm32).
3. Add the path to `[workspace.members]` in the root `Cargo.toml`.
4. Run `cargo xtask workspace`. It fails on a crate without a layer, an upward dependency, a manifest that skips the workspace keys or lints, and a test file an `autotests = false` crate never compiles.
5. Update this page (`docs/crates.md`) and [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md), the human-readable graph. If the crate changes what an agent should read first, extend the decision tables in [`AGENTS.md`](../AGENTS.md).

## See Also

- Root `Cargo.toml` `[workspace.metadata.flui] layers` — the layer names each crate's `[package.metadata.flui] layer` indexes
- [ADR-0041](adr/ADR-0041-workspace-topology-contract.md) — the workspace topology contract and its enforcement rules
- [Foundations](FOUNDATIONS.md) — architecture contract, target crate graph
- [Roadmap](ROADMAP.md) — construction phases from current to target
- [Architecture](architecture.md) — three-tree pipeline + layered DAG (current state)
- [Getting Started](getting-started.md) — build and run instructions
- [`AGENTS.md`](../AGENTS.md) — current cross-tool rules (`.ai-factory/ARCHITECTURE.md` and `.specify/memory/constitution.md` were the historical originals; neither exists in this checkout)
