[← Architecture](architecture.md) · [Back to README](../README.md) · [Crates](crates.md)

# Hot Reload

FLUI targets **Flutter-parity hot reload** (state preserved, `build()` re-run). The operational two-layer model below is the dev-time/build side; the runtime parity protocol is in the [Flutter-parity design](designs/2026-06-28-flutter-parity-hot-reload.md).

## Target vs today

| Capability | Flutter | FLUI today | FLUI target |
|------------|---------|------------|-------------|
| Hot reload (state kept) | Yes | No (`PluginPipeline` = restart) | Host/worker split + `perform_reassemble` |
| Hot restart | Yes | Partial (`app_plugin!`) | `HotReloadTier::HotRestart` |
| Scene plugin reload | N/A | Yes | Keep for GPU demos |

**Critical insight:** state must live in the **host binary** (Element tree), not in the reloadable `.so`. See the design doc §2.

## Two-Layer Model (build orchestration)

```text
┌─────────────────────────────────────────────────────────────────────┐
│  Layer 1 — Build orchestration (dev-time)                           │
│  SourceWatcher  →  cargo build / ndk build  →  artifact on disk     │
│  Crates: flui-hot-reload (`source-watch`), flui-cli, flui-devtools  │
└───────────────────────────────┬─────────────────────────────────────┘
                                │ .so / .dll / binary updated
┌───────────────────────────────▼─────────────────────────────────────┐
│  Layer 2 — Artifact reload (runtime, native only)                   │
│  HotReloadDriver  →  mtime poll  →  dlopen reload  →  new Scene     │
│  Crate: flui-hot-reload (always on non-wasm targets)                │
└─────────────────────────────────────────────────────────────────────┘
```

| Layer | Trigger | Action | State preserved |
|-------|---------|--------|-----------------|
| 1 | Source file change (`notify`) | Rebuild artifact | N/A (build step) |
| 2 | Artifact mtime change | `unload` → `dlopen` → `build_scene()` | No (hot restart for widgets) |

**Rule:** layer 1 never reloads code directly. It only produces a new artifact. Layer 2 never watches `src/` — it only watches the plugin path on disk.

## Reload Strategies

[`ReloadStrategy`](https://github.com/flui-rs/flui/blob/main/crates/flui-hot-reload/src/strategy.rs) in `flui-hot-reload` describes how changes reach the running app:

| Strategy | Command / setup | Layer 1 | Layer 2 | Host process |
|----------|-----------------|---------|---------|--------------|
| `ProcessRestart` | `flui run` (default) | watch `src/` → `cargo build` → kill + respawn | — | restarted |
| `PluginDylib` | `FLUI_SCENE_PLUGIN=…` + host loop | manual / `cargo watch` | `HotReloadDriver::poll()` | kept alive |
| `BuildAndDeploy` | `flui run --scene` (Android) | watch scene `src/` → ndk build → `adb push` | host polls mtime on device | kept alive |
| `None` | `flui run --release`, WASM | — | — | — |

Constants and debounce intervals live in `flui_hot_reload::strategy::{env, timing}`.

## Host / Plugin Split

Following the standard Rust hot-reload pattern (host owns persistent state, worker is reloadable):

```text
┌──────────────── Host (binary) ─────────────────┐
│  Window, GPU renderer, event loop              │
│  HotReloadDriver::poll() each frame            │
│  ScenePlugin::load / unload / build_scene      │
└────────────────────┬───────────────────────────┘
                     │ FFI (extern "C")
┌────────────────────▼───────────────────────────┐
│  Plugin (cdylib)                               │
│  scene_plugin!(fn)  or  app_plugin!(Widget)    │
│  flui_scene_build / flui_app_build             │
└────────────────────────────────────────────────┘
```

### Scene plugin (low-level)

Build a `Scene` directly — best for GPU demos and custom painters.

```rust
// examples/desktop_scene/src/lib.rs
fn my_scene(width: f32, height: f32) -> Scene { /* ... */ }
scene_plugin!(my_scene);
```

### App plugin (high-level, `app-plugin` feature)

Runs Build → Layout → Paint inside the `.so` via `PluginPipeline`. Hot reload performs a **hot restart** (widget tree rebuilt from scratch).

## Crate Map

| Crate | Responsibility |
|-------|----------------|
| **`flui-hot-reload`** | Single source of truth: `DynLib`, `ScenePlugin`, `HotReloadDriver`, `ReloadStrategy`, optional `SourceWatcher` |
| **`flui-cli`** | Layer 1 orchestration: `flui run`, `flui run --scene` |
| **`flui-devtools`** | Callback wrapper `HotReloader` over `SourceWatcher` (future DevTools server) |
| **`flui-build`** | Android NDK build + `adb push` for scene plugins |

Do **not** add a third file-watcher implementation. Extend `flui_hot_reload::dev::SourceWatcher`.

## Desktop Plugin Workflow

Terminal 1 — build plugin on change:

```bash
cargo watch -w examples/desktop_scene -x "build -p flui-desktop-scene"
```

Terminal 2 — run host with in-process reload:

```bash
# Linux/macOS
FLUI_SCENE_PLUGIN=target/debug/libflui_scene.so cargo run --example scene_render

# Windows
set FLUI_SCENE_PLUGIN=target\debug\flui_scene.dll
cargo run --example scene_render
```

The host calls `HotReloadDriver::poll()` in its frame loop; when the `.so` mtime changes, it reloads without restarting.

> **Windows note:** stop the host before rebuilding if the linker cannot overwrite a locked DLL. On Unix, `RTLD_LOCAL` avoids symbol collisions across reloads.

## Android Scene Workflow

```bash
flui run --scene --scene-crate flui-android-scene --package com.example.app --target arm64-v8a
```

1. CLI watches scene crate `src/` (layer 1).
2. On change: `cargo ndk build` + `adb push` to device.
3. Android host polls plugin mtime (layer 2) and reloads in-process.

## WASM / Web

WASM has no `dlopen`. Use `tools/web-server` for rebuild + HTTP serve. Strategy: `ReloadStrategy::None` at runtime; layer 1 is manual `wasm-pack build`.

## Integrating Into a Custom Host

Minimal frame-loop integration:

```rust
use flui_hot_reload::{HotReloadDriver, strategy::env};

let plugin_path = std::env::var(env::SCENE_PLUGIN).ok();
let mut driver = plugin_path.map(|p| HotReloadDriver::new(p));

// each frame:
if let Some(ref mut d) = driver {
    d.poll(width, height);
    let scene = d.build_scene_or(width, height, fallback_scene);
    renderer.render_scene(&scene)?;
}
```

For a full `flui-app` desktop runner, the same `HotReloadDriver` can wrap a plugin-backed root widget once `app-plugin` integration lands in the runner.

## Design Constraints

- **Immediate-mode friendly:** scene plugins rebuild each frame path — code changes show up on next reload without stale retained state.
- **Sanctioned `dyn` boundary:** `flui_hot_reload::dynlib` is the only approved dynamic loading surface (see `PORT.md`).
- **No WASM layer 2:** `HotReloadDriver` is `#[cfg(not(target_arch = "wasm32"))]`.
- **Dev-only:** hot-reload is not shipped in release builds; use static linking for production plugins.

## See Also

- [`crates/flui-hot-reload`](../crates/flui-hot-reload/src/lib.rs) — API and module docs
- [`examples/scene_render.rs`](../examples/scene_render.rs) — reference host integration
- [`examples/desktop_scene`](../examples/desktop_scene/) — reference plugin crate
- [Architecture](architecture.md) — workspace layers (hot-reload is layer 7)
