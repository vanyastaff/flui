[← Architecture](architecture.md) · [Back to README](../README.md) · [Crates](crates.md)

# Hot Reload

FLUI targets **Flutter-parity hot reload** (state preserved, `build()` re-run). The operational two-layer model below is the dev-time/build side; the runtime parity protocol is in the [Flutter-parity design](designs/2026-06-28-flutter-parity-hot-reload.md).

## Target vs today

| Capability | Flutter | FLUI today | FLUI target |
|------------|---------|------------|-------------|
| Hot reload (state kept) | Yes | Yes — `flui run` worker dylib swap (`perform_reassemble`) | Keep |
| Hot restart | Yes | Partial (types change → process restart; in-process remount not wired) | `HotReloadTier::HotRestart` |
| Scene plugin reload | N/A | Yes | Keep for GPU demos |

**Critical insight:** state must live in the **host binary** (Element tree), not in the reloadable `.so`. See the design doc §2.

## Getting a hot-reload project

```bash
flui create my-app --hot-reload
cd my-app
flui run
```

`--hot-reload` emits the three-crate workspace the protocol requires
(`my-app-types` + `my-app-logic` + `my-app-host`) and a `[hot_reload]` section
in `flui.toml`. Edit `my-app-logic/src/lib.rs` and save: `flui run` rebuilds the
worker, the host reloads it in-process, and `State` is preserved. Edit
`my-app-types/src/` and the CLI restarts the host instead (the shared type
layout changed — bump `TYPE_FINGERPRINT`).

### Staging: why the worker path changes on every rebuild

`flui run` does not hand the host the raw `target/<profile>/lib<worker>.dylib`.
It stages each build at a **content-addressed** path,
`{stem}-hot-{fnv1a-hash}{ext}` (e.g. `libcounter_logic-hot-3f9a1c04.dylib`), and
writes that path into a `.flui_worker_plugin` sidecar manifest next to the
canonical output. The host's `WorkerReloadDriver` resolves the sidecar first
(`resolve_worker_path`), so it loads the staged copy.

The hash is over the built bytes, which is what makes an in-process reload
actually reload: **macOS's dyld is a deferred-unmap runtime**, so re-opening the
*same* path can serve the retained, stale image. A name that changes with the
content guarantees a different path. An identical rebuild reuses the existing
staged file untouched — never rewriting a library the host may still have
mapped — and superseded versions are pruned best-effort. The same scheme also
avoids the Windows lock on the canonical output (the host loads the copy, so the
next `cargo build` can overwrite the original). The pattern follows
[`hot-lib-reloader`](https://github.com/rksm/hot-lib-reloader-rs)'s
`{lib_name}-hot-{load_counter}` shadow files.

On macOS each staged dylib is also ad-hoc signed (`codesign --sign -`)
best-effort. This is defensive, not load-bearing: cargo already linker-signs
dylibs and the host runs un-hardened under `cargo run`, but the sign keeps a
future hardened-runtime / library-validation host from refusing the copy. A
missing or failing `codesign` is a warning, never a failed reload.

### One cargo invocation: why worker and host are built together

`flui run` builds the worker and the host in **a single** `cargo build -p worker
-p host`. This is a correctness requirement, not a convenience. Cargo unifies a
dependency's features **per invocation**, and Rust's `TypeId` is only stable
within one compiled instance of a crate. Building the worker on its own (or into
an isolated `--target-dir`) can resolve `flui-widgets` with a different feature
set than the host does, producing two instances of it — and therefore two
different `TypeId::of::<T>()` values for the same type. The worker then fails
every `TypeId`-keyed lookup against host state: the inherited-view map that
`GestureArenaScope::of` reads in `GestureDetector::init_state` misses, and the
first frame panics with *"gesture consumers must be mounted beneath
GestureArenaScope"*. Building both packages together makes cargo unify the graph
once, so both binaries link one instance and the `TypeId`s agree. The reload path
rebuilds both for the same reason (the host is unchanged and is not relinked).

### The host watches the artifact, not the clock

`poll_and_apply` runs at a frame boundary, which is enough while the app is
animating but wrong when it is idle: an idle event loop produces no frames, and
an unfocused or occluded window receives no AppKit display pass at all, so an
edit would not be noticed until something unrelated produced a frame (a click on
the window, in the observed failure). The desktop host therefore runs a small
**background watcher thread** (`WorkerReload::spawn_watcher`) that polls the
worker artifact's `(path, mtime)` stamp — the same identity the driver resolves
through the sidecar — and fires the realm's `wake` on a change. `wake` requests a
frame; that frame's `poll_and_apply` then performs the actual owner-thread
`dlopen`, exactly as before.

This is still layer 2: the watcher watches the **artifact**, never `src/` (layer
1, the CLI, owns source watching and the rebuild), and it adds no second
`notify`-based file watcher — it is the driver's own mtime/identity check, run
where it can wake a sleeping loop rather than only at a frame nothing is
producing.

`examples/hot_reload_counter/` is the hand-written equivalent of that workspace.

## Two-Layer Model (build orchestration)

```text
┌─────────────────────────────────────────────────────────────────────┐
│  Layer 1 — Build orchestration (dev-time)                           │
│  SourceWatcher  →  cargo build / ndk build  →  artifact on disk     │
│  Crate: flui-cli (`watch.rs`, `build/`)                             │
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
| **`flui-hot-reload`** | Runtime half, linked by the app: `DynLib`, `ScenePlugin`, `HotReloadDriver`, `ReloadStrategy`, the worker/host ABI |
| **`flui-cli`** | Dev-machine half: the `SourceWatcher` (`src/watch.rs`), the build pipeline (`src/build/`, once `flui-build`), `flui run`, `flui run --scene` |

Do **not** add a second file-watcher implementation. Extend `flui-cli`'s `watch::SourceWatcher`. The env-var names the CLI sets (`FLUI_HOT_RELOAD`, `FLUI_WORKER_PLUGIN`) are duplicated from `flui_hot_reload::{strategy, engine}::env` and pinned by a test in `commands/run.rs`.

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

## iOS

iOS runs the same host/worker path as desktop and Android (`run_app_ios_with_config` → `bootstrap_ios`): it loads the worker, starts the artifact watcher, and reassembles on change. This is usable in the **Simulator** (and a dev-signed build); a production App Store build has no mutable dylib to load, so `AppConfig`'s worker field is `None` and the capability is inert. Verified end-to-end on an iOS-Simulator: a worker edit re-stages the dylib and the realm reassembles with state preserved.

One iOS-specific requirement is now structural rather than a platform limit. A worker `cdylib` statically links its own `flui-painting` — and therefore its **own** `FONT_SYSTEM` — while never linking `flui-engine` (the crate whose renderer installs the empty-database font fallback). On macOS host discovery finds Latin faces so the gap is invisible; iOS exposes no system font to cosmic-text/fontdb's scan, so the worker's database was empty and the first shaped run panicked (`no default font found`). `flui-painting` now installs the embedded `Roboto-Regular` itself when discovery finds no Latin face — the ADR-0016 "lowest owner loads the baseline" rule, completed — so every image linking the shaper can render text without the engine. See [ADR-0016](adr/ADR-0016-unified-font-system-registration.md)'s 2026-09-18 amendment.

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
- **Sanctioned `dyn` boundary:** `flui_hot_reload::dynlib` is the only approved dynamic loading surface.
- **No WASM layer 2:** `HotReloadDriver` is `#[cfg(not(target_arch = "wasm32"))]`.
- **Dev-only:** hot-reload is not shipped in release builds; use static linking for production plugins.

## See Also

- [`crates/flui-hot-reload`](../crates/flui-hot-reload/src/lib.rs) — API and module docs
- [`examples/scene_render.rs`](../examples/scene_render.rs) — reference host integration
- [`examples/desktop_scene`](../examples/desktop_scene/) — reference plugin crate
- [Architecture](architecture.md) — workspace layers (hot-reload is layer 7)
