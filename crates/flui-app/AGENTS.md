# AGENTS.md — flui-app

Top-level application framework. Hosts owner-affine UI realms and transitional
process services while ADR-0027 extracts the remaining global bindings.

## What lives here

- **UiRealm** — owner-affine widget tree (`WidgetsBinding`) + single-presentation gesture state + scoped GlobalKey/local post-frame activation + bounded command inbox
- **AppBinding / AppConfig** — transitional process host for renderer, lifecycle, focus, and scheduler services; it does not own widget or gesture state
- **RootRenderElement / RootRenderView** — root of the render/element tree
- **run_app / run_app_with_config** — the supported entry points for starting the app
- **run_direct** — experimental direct-engine escape hatch (no widget tree, no input, broken on the winit backend pending ADR-0039's `on_ready` reorder); do not describe it as a supported cross-platform path
- **embedder / overlay** — app-level subsystems (PORT-CHECK-OK-SP4 marked)
- **Re-exports** — `GestureBinding`, `PaintingBinding`, `PipelineOwner`, `RenderingFlutterBinding`, `Scheduler`, `SemanticsBinding`, `WidgetsBinding` from constituent crates

## Key constraints

- **Depends on ALL other crates** — flui-view, flui-rendering, flui-types, flui-foundation, flui-interaction, flui-scheduler, flui-painting, flui-layer, flui-semantics, flui-engine, flui-platform, plus flui-hot-reload behind the optional `hot-reload` feature
- **No design tokens live here.** Colours, typography, spacing, radius, motion, and any other design token are design-system concerns (`flui-material` / `flui-cupertino`, L7); this crate is L9 application composition. A token type appearing under `src/` is a review failure whatever it is named — the parked `AppTheme`/`AppColorScheme` surface was removed for exactly this reason ([ADR-0042](../../docs/adr/ADR-0042-theming-ownership.md)), and `just inventory-check` fails if a `theme` module reappears here. Appearance is per-presentation: the OS signal is `MediaQueryData::platform_brightness` on one window's tree, and the resolved theme is published by an in-tree inherited widget. The app shells that own theme selection (`WidgetsApp` / `MaterialApp` / `CupertinoApp`) are issue #573.
- **Platform features** — `desktop` (default), `android`, `ios`, `web`. Platform-specific entry points gated by `cfg(target_os)`.
- **`hot-reload` feature (off by default)** — `flui-hot-reload` is an *optional* dependency, so an ordinary production graph does not contain it. Everything that would name a `flui_hot_reload` item goes through `app::hot_reload`, which has desktop and Android implementations; web/iOS builds remain cfg-complete but install no reload driver. Add to that seam rather than reaching for the crate directly. The typed worker-plugin fields and builders on `AppConfig` exist only with this feature, so an application cannot configure a capability its build omitted.
- **Debug features** — `debug-overlay`, `performance-overlay` (both off by default)
- **Transitional singleton state** — scheduler/renderer/focus services remain process-scoped; widget state, gesture state, and GlobalKey identity are realm-owned. Gesture state currently models one presentation per realm and moves to `PresentationRuntime` only when a second real presentation consumer exists. Tests mutating the remaining binding globals must use the existing serialization guard.
- **Root scopes** — both attach paths wrap the app root in outer `GestureArenaScope` (the realm binding's shared `BindingDriven` arena) and inner `VsyncScope`; keep production input and recognizers on that same arena
- **flui-interaction `testing` feature** enabled in dev-dependencies for synthetic pointer events in tests
- **Android entry point** — `run_app_android` / `run_app_android_with_config` gated behind `cfg(target_os = "android")`
- **WASM** — `wasm-bindgen-futures` for `spawn_local` frame loop on wasm32
