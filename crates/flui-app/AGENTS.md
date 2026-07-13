# AGENTS.md — flui-app

Top-level application framework. Hosts owner-affine UI realms and transitional
process services while ADR-0027 extracts the remaining global bindings.

## What lives here

- **UiRealm** — owner-affine widget tree (`WidgetsBinding`) + scoped GlobalKey/local post-frame activation + bounded command inbox
- **AppBinding / AppConfig** — transitional process host for renderer, gesture, lifecycle, and scheduler services; it does not own widget state
- **RootRenderElement / RootRenderView** — root of the render/element tree
- **run_app / run_app_with_config / run_direct** — entry points for starting the app
- **embedder / overlay / theme** — app-level subsystems (PORT-CHECK-OK-SP4 marked)
- **Re-exports** — `GestureBinding`, `PaintingBinding`, `PipelineOwner`, `RenderingFlutterBinding`, `Scheduler`, `SemanticsBinding`, `WidgetsBinding` from constituent crates

## Key constraints

- **Depends on ALL other crates** — flui-view, flui-rendering, flui-types, flui-foundation, flui-interaction, flui-scheduler, flui-painting, flui-layer, flui-semantics, flui-engine, flui-platform, flui-hot-reload
- **Platform features** — `desktop` (default), `android`, `ios`, `web`. Platform-specific entry points gated by `cfg(target_os)`.
- **Debug features** — `debug-overlay`, `performance-overlay` (both off by default)
- **Transitional singleton state** — scheduler/gesture/renderer services remain process-scoped; widget state and GlobalKey identity are realm-owned. Tests mutating the remaining binding globals must use the existing serialization guard.
- **flui-interaction `testing` feature** enabled in dev-dependencies for synthetic pointer events in tests
- **Android entry point** — `run_app_android` / `run_app_android_with_config` gated behind `cfg(target_os = "android")`
- **WASM** — `wasm-bindgen-futures` for `spawn_local` frame loop on wasm32
