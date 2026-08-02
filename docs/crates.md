[← Port](PORT.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md) · [Testing →](testing.md)

# Crates Map

> **Scope.** This page describes the **current** workspace as it is built today. `flui-localizations`, `flui-material`, and `flui-cupertino` (Catalog.1) have landed; the remaining target crate decomposition — the formal `flui` facade — is defined in [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md); the migration is sequenced in [`ROADMAP.md`](ROADMAP.md).

The FLUI workspace contains 20+ crates organized into a strict layered DAG. This page is the canonical inventory: what each crate does, what layer it sits in, and whether it is currently active.

> **Layer assignments here mirror [`workspace-layers.toml`](workspace-layers.toml), which is the authoritative policy.** `scripts/check-workspace-inventory.sh` (`just inventory-check`) validates every **normal** Cargo dependency edge against that file: strictly downward, with an ordered-pair allowlist for same-layer edges, named forbidden pairs, and an acyclicity check that includes projected future edges. See [ADR-0041](adr/ADR-0041-workspace-topology-contract.md) for the contract and [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md) for the target graph. Dev-dependencies are deliberately out of scope — they cross layers freely and Cargo permits cycles among them.

A crate marked **DISABLED** is commented out in `Cargo.toml` `[workspace.members]` while integration is in progress; the source tree still exists but is not built by default. A crate may be active but omitted from `default-members`; `cargo build --workspace` still includes every active workspace member.

## Layer 0 — Foundation (value types)

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-geometry` | ✅ ACTIVE | Geometry primitives and unit-safe coordinate spaces (`Point`, `Rect`, `Size`, `Offset`, `Matrix4`, Bézier, superellipse), re-exported by `flui-types` |
| `flui-types` | ✅ ACTIVE | Base value types and units (px, dp); styling (colors, paint values); typography; layout enums; gestures; physics value types; platform value types. **ID newtypes** (`ElementId`, `RenderId`, `LayerId`, etc. — all `NonZeroUsize`-backed) live in `flui-foundation`, not here. |

## Layer 1 — Framework primitives

`flui-foundation` is responsible for framework primitives above raw value types, but its current runtime manifest intentionally stays leaf-like: `flui-types` is a dev-dependency only. See `Note on flui-foundation placement` in [Architecture](architecture.md).

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-foundation` | ✅ ACTIVE | Framework primitives: `ChangeNotifier` / `Listenable`, `Id` system, `BindingBase`, `Key`, diagnostics, error helpers. Loses process-global subscriber installation and the platform logging backends to a restored, composition-only logging crate ([#568](https://github.com/vanyastaff/flui/issues/568)). |
| `flui-macros` | ✅ ACTIVE | Proc-macro crate for framework derives and generated boilerplate |

## Layer 2 — Substrate

These crates compose the rendering and platform substrate largely without knowing about each other. The one intra-layer edge is `flui-interaction → flui-platform` ([ADR-0037](adr/ADR-0037-presentation-ownership-domains.md)): interaction owns the owner-local text-input state and names the OS-facing capability platform defines.

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-tree` | ✅ ACTIVE | Generic tree abstractions: `TreeRead` / `TreeNav` / `TreeWrite` trio, iterators / slots, arity markers (`Leaf` / `Single` / `Optional` / `Variable`), depth markers. The Cycle-3 audit deleted speculative `visitor` / `diff` modules (~10k LOC zombie surface) — concrete trees adopt the trio directly |
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
| `flui-objects` | ✅ ACTIVE | Concrete `RenderBox` / `RenderSliver` catalog. Wrapped by `flui-widgets`, and named directly by `flui-view` for framework machinery whose element and render halves cooperate (`RenderLayoutBuilder`, `RenderSliverList`, `RenderSliverGridLazy`) |

## Layer 5 — Framework spine

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-view` | ✅ ACTIVE | View + Element tree, `BuildContext`, view trait |

## Layer 6 — Widget catalog + DX tooling

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-widgets` | ✅ ACTIVE | User-facing Flutter-style widget catalog (configuration objects over `flui-objects`); owns the `Localizations`/`Directionality`/`WidgetsLocalizations` ambient-theming and localization substrate |
| `flui-binding` | ✅ ACTIVE | Deterministic non-singleton headless frame driver: `HeadlessBinding::pump_frame(dt)` advances a virtual `ManualClock` and polls clock-bound gesture-arena deadlines — sleep-free time-based gesture tests (long-press, double-tap). Animation-controller ticks (Phase 3) and tree-rebuild integration (Phase 1b) are deferred. **Renamed to `flui-testing`** by issue [#569](https://github.com/vanyastaff/flui/issues/569), before Runtime.1's conformance harness makes the package name a public testing contract. |
| `flui-hot-reload` | ✅ ACTIVE | Two-layer hot-reload: runtime `HotReloadDriver` (layer 2, dlopen) + optional `SourceWatcher` (layer 1, `source-watch` feature). See [hot-reload.md](hot-reload.md). Becomes feature-gated (removable) per issue [#569](https://github.com/vanyastaff/flui/issues/569). |
| `flui-build` | ✅ ACTIVE | Async cross-platform build pipeline (`PlatformBuilder` typestate) |

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
| `flui-app` | ✅ ACTIVE (migration) | App runner, root widget, application lifecycle. The **private composition root** for runtime ownership during Runtime.1 — a `flui-runtime` crate is not extracted from it until two entry points prove the boundary ([ADR-0041](adr/ADR-0041-workspace-topology-contract.md)). |
| `flui-cli` | ✅ ACTIVE | CLI tooling (`flui run` hot-reload orchestration, Android scene deploy). Depends on `flui-devtools` inside this layer. |
| `flui-devtools` | ✅ ACTIVE (partial) | Profiler; `HotReloader` delegates to `flui-hot-reload` |

## Layer 10 — Facade

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui` | ✅ ACTIVE | The root package / app-author facade. Compiles both design systems unconditionally today; feature-selection is issue [#569](https://github.com/vanyastaff/flui/issues/569). |

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

A new crate is a topology change, so it starts with the contract, not the directory. `just inventory-check` fails on any `crates/*` member missing from the policy — that failure is the gate, not a nuisance.

1. Check [`workspace-layers.toml`](workspace-layers.toml)'s `[[planned]]` section. If the crate is listed with `status = "gated"`, satisfy the gate first — `flui-runtime`, for example, requires two entry points driving the same proven core plus a measurable dependency reduction ([ADR-0041](adr/ADR-0041-workspace-topology-contract.md)). A crate created before its second consumer exists freezes a guessed boundary.
2. Decide its layer based on what it depends on. Lower-layer crates must not depend on higher-layer ones. If it needs a same-layer edge, that is an `[[same_layer_edge]]` entry with a written rationale, not a default.
3. Add a `[[member]]` entry to `workspace-layers.toml` with its layer and disposition (`keep` / `rename` / `narrow` / `optionalize` / `deferred-extraction`).
4. Add the directory under `crates/<flui-name>/` with a standard layout (`Cargo.toml`, `src/lib.rs`, `src/error.rs`).
5. Add the path to `[workspace.members]` in the root `Cargo.toml`; add it to `default-members` unless the crate is intentionally excluded from default local builds.
6. Update the constitution layer table in `.specify/memory/constitution.md` if it represents a new responsibility.
7. Update this page (`docs/crates.md`), [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md), and the `active_crates` / `build-layered` inventories in the `justfile`.

## See Also

- [`workspace-layers.toml`](workspace-layers.toml) — the authoritative, CI-checked layer policy
- [ADR-0041](adr/ADR-0041-workspace-topology-contract.md) — the workspace topology contract and its enforcement rules
- [Foundations](FOUNDATIONS.md) — architecture contract, target crate graph
- [Roadmap](ROADMAP.md) — construction phases from current to target
- [Architecture](architecture.md) — three-tree pipeline + layered DAG (current state)
- [Getting Started](getting-started.md) — build and run instructions
- [`.ai-factory/ARCHITECTURE.md`](../.ai-factory/ARCHITECTURE.md) — full architectural rules
- [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — constitution v2.3.0
