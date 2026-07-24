# AGENTS.md — flui-app

Top-level application framework. Hosts owner-affine UI realms and transitional
process services while ADR-0027 extracts the remaining global bindings.

## What lives here

- **UiRealm** — owner-affine widget tree (`WidgetsBinding`) + single-presentation gesture state + scoped GlobalKey/local post-frame activation + bounded command inbox
- **AppBinding / AppConfig** — transitional process host for renderer, lifecycle, focus, and scheduler services; it does not own widget or gesture state
- **RootRenderElement / RootRenderView** — root of the render/element tree
- **run_app / run_app_with_config / run_direct** — entry points for starting the app
- **embedder / overlay / theme** — app-level subsystems (PORT-CHECK-OK-SP4 marked)
- **Re-exports** — `GestureBinding`, `PaintingBinding`, `PipelineOwner`, `RenderingFlutterBinding`, `Scheduler`, `SemanticsBinding`, `WidgetsBinding` from constituent crates

## Key constraints

- **Depends on ALL other crates** — flui-view, flui-rendering, flui-types, flui-foundation, flui-interaction, flui-scheduler, flui-painting, flui-layer, flui-semantics, flui-engine, flui-platform, flui-hot-reload
- **Platform features** — `desktop` (default), `android`, `ios`, `web`. Platform-specific entry points gated by `cfg(target_os)`.
- **Debug features** — `debug-overlay`, `performance-overlay` (both off by default)
- **Transitional singleton state** — scheduler/renderer/focus services remain process-scoped; widget state, gesture state, and GlobalKey identity are realm-owned. Gesture state currently models one presentation per realm and moves to `PresentationRuntime` only when a second real presentation consumer exists. Tests mutating the remaining binding globals must use the existing serialization guard.
- **Root scopes** — both attach paths wrap the app root in outer `GestureArenaScope` (the realm binding's shared `BindingDriven` arena) and inner `VsyncScope`; keep production input and recognizers on that same arena
- **flui-interaction `testing` feature** enabled in dev-dependencies for synthetic pointer events in tests
- **Android entry point** — `run_app_android` / `run_app_android_with_config` gated behind `cfg(target_os = "android")`
- **WASM** — `wasm-bindgen-futures` for `spawn_local` frame loop on wasm32
