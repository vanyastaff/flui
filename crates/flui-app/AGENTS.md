# AGENTS.md — flui-app

Top-level application framework. Combines all bindings into `WidgetsFlutterBinding`.

## What lives here

- **WidgetsFlutterBinding** — the singleton that combines all bindings (build, layout, paint, input, scheduling, semantics)
- **AppBinding / AppConfig** — application configuration and lifecycle
- **RootRenderElement / RootRenderView** — root of the render/element tree
- **run_app / run_app_with_config / run_direct** — entry points for starting the app
- **embedder / overlay / theme** — app-level subsystems (PORT-CHECK-OK-SP4 marked)
- **Re-exports** — `GestureBinding`, `PaintingBinding`, `PipelineOwner`, `RenderingFlutterBinding`, `Scheduler`, `SemanticsBinding`, `WidgetsBinding` from constituent crates

## Key constraints

- **Depends on ALL other crates** — flui-view, flui-rendering, flui-types, flui-foundation, flui-interaction, flui-scheduler, flui-painting, flui-layer, flui-semantics, flui-engine, flui-platform, flui-hot-reload
- **Platform features** — `desktop` (default), `android`, `ios`, `web`. Platform-specific entry points gated by `cfg(target_os)`.
- **Debug features** — `debug-overlay`, `performance-overlay` (both off by default)
- **Singleton state flake** — CI runs nextest with `--test-threads=1` because `WidgetsFlutterBinding::instance()` is a process-wide singleton
- **flui-interaction `testing` feature** enabled in dev-dependencies for synthetic pointer events in tests
- **Android entry point** — `run_app_android` / `run_app_android_with_config` gated behind `cfg(target_os = "android")`
- **WASM** — `wasm-bindgen-futures` for `spawn_local` frame loop on wasm32
