[← Architecture](architecture.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md) · [Testing →](testing.md)

# Crates Map

> **Scope.** This page describes the **current** workspace as it is built today. `flui-material` and `flui-cupertino` have landed; the remaining target crate decomposition — the formal `flui` facade — is defined in [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md); the migration is sequenced in [`ROADMAP.md`](ROADMAP.md).

The FLUI workspace contains 24 crates under `crates/`, the official packages under `packages/` (`flui-material`, `flui-cupertino`, `flui-devtools`), and the `flui` facade, organized into a strict layered DAG. This page is the canonical inventory: what each crate does, what layer it sits in, and whether it is currently active.

> **Tier and layer assignments here mirror the manifests, which are the authority.** Each crate and the facade declare `[package.metadata.flui] tier`, `tier-kind` and `order`; the root `Cargo.toml` names the tiers in `[workspace.metadata.flui] tiers`, bottom to top ([ADR-0081](adr/ADR-0081-workspace-tiers-and-reach-facts.md)). `cargo xtask workspace` checks every **normal** and build dependency between workspace packages: it points to a lower tier, or to a smaller `order` in the same tier, unless the dependent lists the edge in `edge-exceptions` with the ADR that removes it (an entry for an edge no rule of ADR-0081 refuses is itself a finding, so the list only shrinks); and nothing with a tier depends on a `tier-kind = "tool"` package. Examples and tools declare only `tier-kind = "tool"`. Until the `layer` key is removed, each crate also declares `layer = N` (names in the root `layers`), and the same edges must point to the same layer or lower, never at an example or tool ([ADR-0041](adr/ADR-0041-workspace-topology-contract.md)); the layer sections below follow that key. Cargo itself rejects cycles. See [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md) for the target graph. Dev-dependencies may point anywhere (tests use `flui-testing`) and Cargo permits cycles among them: `flui-view`, `flui-interaction`, `flui-scheduler`, `flui-widgets` and `flui-runtime` each form one with `flui-testing`, and `flui-rendering` one with `flui-objects`. The kind rule ([ADR-0081](adr/ADR-0081-workspace-tiers-and-reach-facts.md) §3, [ADR-0088](adr/ADR-0088-official-packages-sdk-and-facade.md) §2) reads dev edges too: a crate that is neither an official package nor an application names a `tier-kind = "official"` package in no dependency kind, and an official package's normal and build dependencies are `flui-sdk` and the contract crates (`flui-platform-api`, `flui-protocol`); each refused edge needs the dependent's `edge-exceptions` entry, like a tier refusal. Official packages that build on the SDK live under `packages/`, and a member there is official with no `edge-exceptions`. A crate can also narrow who depends on it in its `[package.metadata.flui]`: `allowed-dependents` for normal and build edges, which is how `flui-log` stays composition-only, how only the host links `flui-platform`, and how only the host and the test driver link `flui-runtime` (their dev-dependents are not restricted), and `allowed-dev-dependents` for dev edges, which no manifest lists since the design systems moved to the kind rule. Examples and tools are applications and may depend on anything.

> **Reach.** `cargo xtask reach` checks what each crate's resolved graph contains, not just its direct edges ([ADR-0081](adr/ADR-0081-workspace-tiers-and-reach-facts.md) §2): in every root build (the facade under each supported feature combination and each feature alone, every crate at its defaults and with all features), no crate reaches a package its tier forbids in the root `[workspace.metadata.flui.reach]`, or that its own `reach-forbid` adds. A crate that must reach one lists `reach-exceptions = [{ to = "<package>", exit = "ADR-NNNN", reason = "…" }]` (`grant` in place of `exit` for a standing permission): the entry excuses the paths that run through that crate, and an entry that excuses nothing is itself a finding.

> **Process-global state is listed in the manifests too.** Each crate's `[package.metadata.flui] globals` names every `static` and `thread_local!` it keeps outside `#[cfg(test)]`, except immutable data and private `fetch_add`-only ID counters: `{ item = "<path in the crate>", exit = "ADR-NNNN", reason = "…" }` for debt an ADR removes, or `{ item = "…", grant = "ADR-0097", class = "trampoline" | "counter" | "immutable" | "process" | "diagnostic", reason = "…" }` for state that stays. `cargo xtask globals` fails on a global without an entry and on an entry without a global ([ADR-0097](adr/ADR-0097-no-process-global-state-gate.md)); `cargo xtask globals --seed` prints the entries a crate is missing.

## Tiers

| Tier | Crates, by `order` | `tier-kind` |
|------|--------------------|-------------|
| V values | `flui-macros` (3), `flui-foundation` (4) | internal |
| C contracts | `flui-platform-api` (1), `flui-protocol` (2) | stable |
| S substrate | `flui-log` (1), `flui-scheduler` (2), `flui-painting` (3), `flui-interaction` (4), `flui-semantics` (5), `flui-animation` (6), `flui-assets` (7) | internal |
| R render machine | `flui-layer` (1), `flui-rendering` (2), `flui-objects` (3), `flui-engine` (4) | internal |
| K spine and runtime | `flui-view` (1), `flui-widgets` (3), `flui-runtime` (4), `flui-sdk` (5), `flui-testing` (6) | internal; `flui-sdk` evolving (its own `0.N` version, which `cargo xtask workspace` requires of an evolving crate) |
| H hosts | `flui-platform` (1), `flui-app` (2), `flui-cli` (3), `flui` (4) | internal; `flui-cli` tool; `flui` stable |
| pkg official packages | `flui-material` (1, under `packages/`, on `flui-sdk`), `flui-cupertino` (2, under `packages/`, on `flui-sdk`), `flui-devtools` (3, under `packages/`, on `flui-sdk`), `flui-hot-reload` (4) | official |

The `edge-exceptions` in force:

- `flui → flui-hot-reload` until [ADR-0094](adr/ADR-0094-hot-reload-through-subsecond.md); `flui → flui-material` and `flui → flui-cupertino` until [ADR-0088](adr/ADR-0088-official-packages-sdk-and-facade.md);
- the kind rule's: the official package not yet on the SDK, `flui-hot-reload` → `flui-foundation`, `flui-layer`, `flui-rendering`, `flui-view` until ADR-0094.

A crate marked **DISABLED** is commented out in `Cargo.toml` `[workspace.members]` while integration is in progress; the source tree still exists but is not built by default.

## Layer 0 — Foundation (value types)

Empty: no crate declares `layer = 0`. The value types have no crate of their own; each lives with its owner ([ADR-0098](adr/ADR-0098-owned-f64-geometry-values.md) §8): geometry in `flui_foundation::geometry`, paint, styling and typography values in `flui-painting`, constraints in `flui-rendering`, gesture details in `flui-interaction`, and platform values (`Brightness`, `Locale`, IME) in `flui-platform-api`. The index stays so no manifest's `layer` renumbers.

## Layer 1 — Framework primitives

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-foundation` | ✅ ACTIVE | Framework primitives and the plain-`f64` geometry values in `geometry` (`Point`, `Offset`, `Size`, `Rect`, `RRect`, `Radius`, `EdgeInsets`, `Matrix4`, `Axis`, the device-grid `DevicePoint`/`DeviceSize`/`DeviceRect`, `DevicePixelRatio`, the snap and cover rounding functions and `canonical_bits`; [ADR-0098](adr/ADR-0098-owned-f64-geometry-values.md)); `ChangeNotifier` / `Listenable`, `Id` system, `Key`, the `observe` tree-observer seam (ADR-0040) + `RebuildReason`, the shared structured-field vocabulary in `diagnostics`, error helpers. There is no `BindingBase` — the ambient-singleton machinery it backed was retired workspace-wide. Emits `tracing` events; owns no subscriber. |
| `flui-macros` | ✅ ACTIVE | Proc-macro crate for framework derives and generated boilerplate |
| `flui-protocol` | ✅ ACTIVE | The vocabulary shared with tests, devtools and agents ([ADR-0095](adr/ADR-0095-agent-protocol-schema-crate.md)): `SemanticsRole`/`SemanticsAction` (re-exported by `flui-semantics`) and the ADR-0080 wire `Role`/`ActionName`/`Checked` (used by `tools/desktop-mcp`); `serde`/`schemars` behind features. No workspace dependency. |
| `flui-platform-api` | ✅ ACTIVE | Platform contracts with no OS backend ([ADR-0082](adr/ADR-0082-platform-api-contract-crate.md)): the per-window contract `PlatformWindow`, the capability traits (`PlatformTextInput`, `PlatformHaptics`, `PlatformDisplay`, `Clipboard`, the `data_transfer` transport) and the window and input vocabulary. Names no OS, winit, AccessKit or tokio type and has no `unsafe`, so a crate or plugin that programs against a capability links none of the backends; `cargo xtask reach` holds that (tier C plus its own `reach-forbid` of `accesskit` and `tokio`). `Platform` and `HostWindow` (a `PlatformWindow` plus its AccessKit accessibility bridge) stay in `flui-platform`, which re-exports everything here at its old paths. |

## Layer 2 — Substrate

These crates compose the rendering and platform substrate largely without knowing about each other. Interaction owns the owner-local text-input state and names the OS-facing capability through `flui-interaction → flui-platform-api` (layer 1), not through the backends ([ADR-0037](adr/ADR-0037-presentation-ownership-domains.md), [ADR-0082](adr/ADR-0082-platform-api-contract-crate.md)).

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-log` | ✅ ACTIVE | Composition-only cross-platform logging backend: desktop `fmt` (optionally hierarchical), Android logcat, Apple unified logging, browser console/performance timeline, behind an explicit subscriber-ownership policy. **No framework crate but `flui-app`, `flui-cli` and the facade may link it** (examples, being applications, may) — framework crates use `tracing` directly, and its manifest's `allowed-dependents` (checked by `cargo xtask workspace`) enforces that mechanically. |
| `flui-scheduler` | ✅ ACTIVE | Frame scheduling, microtasks, task prioritization. Narrows to logical update phases, tickers, callback ordering, and owner-local post-frame behavior; presentation clocks and raster backpressure move to presentation/runtime ownership. |
| `flui-painting` | ✅ ACTIVE | `Canvas` API, `DisplayList`, paint commands, text recording; owns the paint values (`paint`: `Paint`, `Path`, shaders, `BlendMode`, `Clip`, images), `styling` (`Color`, borders, `BoxDecoration`, gradients, shadows), `typography` (`TextStyle`, spans) and `Alignment`, `BoxFit`, `BoxShape`, `TextBaseline` |
| `flui-interaction` | ✅ ACTIVE | Hit-testing, gestures, focus, pointer events, owner-local text input |
| `flui-assets` | ✅ ACTIVE | Asset loading, caching, image decoding |

## Layer 3 — Compositing / a11y / animation

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-layer` | ✅ ACTIVE | Layer composition tree (compositor input) |
| `flui-semantics` | ✅ ACTIVE | Accessibility tree (semantics nodes, focus, labels) |
| `flui-animation` | ✅ ACTIVE | Curves, tweens, controllers, implicit animations (re-enabled for the Core.1 transition widgets) |
| `flui-platform` | ✅ ACTIVE | Backends (native Win32 / AppKit / Headless + `winit` fallback) and the host-facing `Platform` / `HostWindow` surface; the contracts it implements, `PlatformWindow` among them, live in `flui-platform-api`. Sole home of OS-specific code. **Only `flui-app` may depend on it** (its manifest's `allowed-dependents`, checked by `cargo xtask workspace`; examples and tools are exempt). It sits at layer 3, above the substrate, because its accessibility bridge implements `PlatformAccessibility`, which lives in `flui-semantics` ([ADR-0082](adr/ADR-0082-platform-api-contract-crate.md) §2). Loses `BackgroundExecutor`/`PlatformExecutor` when host-injected runtime execution lands. |

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
| `flui-runtime` | ✅ ACTIVE (migration) | The frame runtime of [ADR-0083](adr/ADR-0083-one-frame-transaction-in-flui-runtime.md): the UI realm and the per-presentation machinery it drives, moved out of `flui-app` in steps (the ADR's `## Migration`). Holds the realm core (`UiRealm`, `PresentationState`, the presentation forest, lifecycle, frame-failure reporting, `RenderingFlutterBinding`), the held-input lane, the semantics host, the commit epoch, the performance-stats window, the frame sink seam (`FrameSink`, `SubmitVerdict`), the frame transaction every runner drives (`UiRealm::pump`, `pump_background`), each realm's `TextContext` over the app's font collection ([ADR-0092](adr/ADR-0092-per-realm-text-over-parley.md)) and the execution services (ADR-0047); the dispatch layer (`OwnerHost`) is still to move. Internal (not an embedder API), except the execution host-injection seam (`HostExecutors` and its companions) and the frame-failure report types, which `flui-app` and the facade re-export and which carry the Stable promise (ADR-0089 §1); `flui-app` and the test driver `flui-testing` are its only allowed normal dependents (`allowed-dependents`), and its normal graph reaches no platform backend, windowing, GPU or engine crate. A realm reads its time from one `ClockSource`, so a test driver's manual clock drives its frame time, gesture deadlines and produce gate. |
| `flui-sdk` | ✅ ACTIVE | The package-author surface of [ADR-0088](adr/ADR-0088-official-packages-sdk-and-facade.md): whole-module re-exports (`animation`, `foundation`, `geometry`, `view`, `widgets`) and curated `platform`/`interaction`/`painting`/`rendering` subsets at the facade's paths, plus the Evolving `pipeline` (render internals) and `hooks` (development hooks: `FrameSnapshot`) modules; the same items as the facade's, never wrappers. Evolving, versioned `0.N` apart from the train. Its normal graph reaches no host, engine or GPU crate. `flui-material`, `flui-cupertino` and `flui-devtools` build on it alone. The FLUI derives resolve through it first, so a package on the SDK alone can use the view, inherited and animation derives; the `Diagnosticable` derive has no SDK path yet. The train guard it relies on is `flui-foundation`'s `links = "flui_train"`. |
| `flui-testing` | ✅ ACTIVE (migration) | The workspace's **test-support** package, above the runtime and the widget catalog (ADR-0083 §4): `HeadlessRealm` hosts a `UiRealm` over a headless window and sink and pumps its frame transaction on a virtual `ManualClock`; the widget harness (`widgets::lay_out`, `widgets::harness::mount`) runs every frame through it. `HeadlessBinding::pump_frame(dt)` remains the substrate driver for the raw-owner suites until they move to the pump. Virtual time, fake platform capabilities, deterministic replay, accessibility queries, the text-store kit and golden helpers belong here. Runtime and framework crates take *development* edges into it only, and `flui-widgets` forbids reaching it. |
| `flui-hot-reload` | ✅ ACTIVE | Runtime half of hot reload: `HotReloadDriver`, `DynLib`, worker/host ABI (dlopen). The dev-time watcher lives in `flui-cli` |

## Layer 7 — Design systems

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-material` | ✅ ACTIVE | Material Design theming foundation — `ColorScheme`, `Typography`/`TextTheme`, `ThemeData`, and the `Theme` inherited widget (constants-first M3 baseline; `fromSeed` deferred), and the M3 widget catalog. An official package under `packages/flui-material`: its only FLUI dependency is `flui-sdk`, through which it implements `flui-widgets`' `InheritedTheme` trait |
| `flui-cupertino` | ✅ ACTIVE | iOS-style (Cupertino) theming foundation — `CupertinoDynamicColor`/`CupertinoColors`, `CupertinoTextThemeData`, `CupertinoThemeData`, the `CupertinoTheme` inherited widget, and `CupertinoButton` (constants-first V1; brightness-only dynamic-color resolution, one component). An official package under `packages/flui-cupertino`: its only FLUI dependency is `flui-sdk`, through which it implements `InheritedTheme`; independent sibling of `flui-material` (ADR-0028 — neither depends on the other) |

## Layer 8 — empty

Empty since [ADR-0081](adr/ADR-0081-workspace-tiers-and-reach-facts.md) deleted `flui-localizations`; `GlobalWidgetsLocalizations` and its delegate live in `flui_widgets::localization`. The index stays so no manifest's `layer` renumbers.

## Layer 9 — Application / tooling

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui-app` | ✅ ACTIVE (migration) | App runner, root widget, application lifecycle. The **composition root**: runners, platform wiring and the raster lane. The frame runtime is moving from it into `flui-runtime` ([ADR-0083](adr/ADR-0083-one-frame-transaction-in-flui-runtime.md), which supersedes ADR-0041's two-consumer gate): the realm core has moved; the realm dispatch layer and the loop-scoped `AppRuntime` are still here, and render the realm through the raster lane (`RealmRaster`). Owns **no design tokens** ([ADR-0042](adr/ADR-0042-theming-ownership.md)); it names no reload tool, and reaches hot reload only through a `DevReloadHook` the application installs ([ADR-0094](adr/ADR-0094-hot-reload-through-subsecond.md) §1). |
| `flui-cli` | ✅ ACTIVE | The `flui` CLI: `create`/`run` (hot reload with hot-keys)/`build`/`doctor`/`devices`/`emulators`, one output policy (`--json`, `--quiet`, `--non-interactive`) and a documented exit-code table. The per-target build pipeline (Android/iOS/desktop/web) lives in its own `src/build/` module; depends on `flui-hot-reload`; no edge to `flui-devtools`. |
| `flui-devtools` | ✅ ACTIVE (partial) | Profiler, timeline, inspector counters. An official package under `packages/flui-devtools`: its only FLUI dependency is `flui-sdk` (`hooks::FrameSnapshot` for the timeline, `foundation::observe` for the inspector's ADR-0040 seam). It holds the seam's end-to-end test and the observer-overhead bench |

## Layer 10 — Facade

| Crate | Status | Purpose |
|-------|--------|---------|
| `flui` | ✅ ACTIVE | The root package / app-author facade. Feature-selective, nothing on by default: `material`, `cupertino`, `hot-reload`; `localizations` is empty and deprecated. A module whose feature is off is absent, not empty. Every supported combination is compiled in isolation by `cargo xtask facade-combos` (run by the CI feature-matrix job), so a combination cannot pass through workspace feature unification. |

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
cargo build -p flui-foundation
cargo build -p flui-log
cargo build -p flui-platform
# ... continue up the layers
cargo build -p flui-app
```

## Adding a New Crate

A new crate is a topology change, so it starts with the contract, not the directory: a crate is a layer, not a feature ([ADR-0041](adr/ADR-0041-workspace-topology-contract.md)). A crate created before its boundary is known freezes a guessed one, so the ADR that places it names what it owns and what its normal graph may not reach — `flui-runtime`, for example, was created by [ADR-0083](adr/ADR-0083-one-frame-transaction-in-flui-runtime.md), which justifies it by one production consumer plus the test driver that is to run the same frame (`flui-testing` takes that edge: its `HeadlessRealm` drives the realm's pump).

1. Decide its tier, kind and order, and its layer, from what it depends on: a dependency points to a lower tier or a smaller `order` in the same tier, and to the same layer or lower. The kind states what it promises ([ADR-0081](adr/ADR-0081-workspace-tiers-and-reach-facts.md) §3).
2. Add the directory under `crates/<flui-name>/` (an official package that builds on `flui-sdk` alone goes under `packages/<flui-name>/`, with `tier-kind = "official"` and no `edge-exceptions`) with a standard layout (`Cargo.toml`, `src/lib.rs`, `src/error.rs`). The manifest inherits the shared `[workspace.package]` keys and the workspace lints, and declares `[package.metadata.flui] tier`, `tier-kind`, `order` (unique in the tier) and `layer = N` (plus `wasm = false`, with the reason beside it, if it cannot build for wasm32).
3. Add the path to `[workspace.members]` in the root `Cargo.toml`.
4. Run `cargo xtask workspace`. It fails on a crate without a tier, kind, order or layer, a duplicate order, a dependency against the tier or layer order, a stale `edge-exceptions` entry, a manifest that skips the workspace keys or lints, and a test file an `autotests = false` crate never compiles. Then run `cargo xtask globals`: a `static` or `thread_local!` the crate keeps needs a `globals` entry, which `cargo xtask globals --seed` prints.
5. Update this page (`docs/crates.md`) and [`FOUNDATIONS.md` Part IV](FOUNDATIONS.md), the human-readable graph. If the crate changes what an agent should read first, extend the decision tables in [`AGENTS.md`](../AGENTS.md).

## See Also

- Root `Cargo.toml` `[workspace.metadata.flui] tiers` and `layers` — the tier names each crate's `[package.metadata.flui] tier` names, and the layer names its `layer` indexes
- [ADR-0081](adr/ADR-0081-workspace-tiers-and-reach-facts.md) — tiers, kinds and the tier rule
- [ADR-0041](adr/ADR-0041-workspace-topology-contract.md) — the workspace topology contract and its enforcement rules
- [Foundations](FOUNDATIONS.md) — architecture contract, target crate graph
- [Roadmap](ROADMAP.md) — construction phases from current to target
- [Architecture](architecture.md) — three-tree pipeline + layered DAG (current state)
- [Getting Started](getting-started.md) — build and run instructions
- [`AGENTS.md`](../AGENTS.md) — current cross-tool rules (`.ai-factory/ARCHITECTURE.md` and `.specify/memory/constitution.md` were the historical originals; neither exists in this checkout)
