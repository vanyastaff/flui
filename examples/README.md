# FLUI Examples

This directory contains examples demonstrating FLUI framework features, from the minimal
"hello world" through engine-internals probes. Most are plain `examples/*.rs` files runnable
with `cargo run --example <name>`; a few need a Cargo feature (noted below), and a few are
separate crates or WASM targets with their own build step.

## Start Here

| Example | Run |
|---|---|
| **counter** — the minimal FLUI app: one `StateCell` and a button (start here — this is the README's own "Hello World" code sample, and the exact shape `flui create`'s `counter` template generates) | `cargo run --example counter` |
| **todo** — the step past `counter`: a list in `StateHandle<Vec<Item>>` (not `StateCell`, which needs `T: Copy`), with add/toggle/delete. See the book's [Tutorial: counter → todo](../book/src/getting-started/tutorial-todo.md) | `cargo run --example todo --features material` |
| **widgets_gallery** — a tour of the wider `flui-widgets` catalog through `flui::prelude` + `run_app` | `cargo run --example widgets_gallery` |
| **platform_window** — the platform layer *without* widgets: a raw window and event loop, no `View`/`Element`/render tree. Useful for debugging platform integration itself, not as a first example of the framework | `cargo run --example platform_window` |
| **colored_box_app** — the first FLUI application through the real pipeline, at the low-level `flui-view`/`flui-objects` layer (no facade) | `cargo run --example colored_box_app` |
| **direct_render** — first working FLUI application | `cargo run --example direct_render` |
| **text_app** — the text pipeline end-to-end through the real framework | `cargo run --example text_app` |

## Widget catalogs (Material / Cupertino)

| Example | Run |
|---|---|
| **material_demo** | `cargo run --example material_demo --features material` |
| **cupertino_demo** | `cargo run --example cupertino_demo --features cupertino` |
| **sliver_demo** — a pinned `SliverAppBar` collapsing demo | `cargo run --example sliver_demo --features material` |
| **multi_window_demo** — a secondary window opened from a Material button | `cargo run --example multi_window_demo --features material` |
| **screenshot** — headless screenshot of a demo widget tree, no window | `cargo run --example screenshot --features material,cupertino` |

## Rendering / engine internals

| Example | Run |
|---|---|
| **aa_showcase** — visual check for the engine's anti-aliasing paths | `cargo run --example aa_showcase` |
| **animated_box_app** — the animation engine driving the real pipeline on GPU | `cargo run --example animated_box_app` |
| **color_filter_demo** — `ColorFilter` live via `SceneBuilder` | `cargo run --example color_filter_demo` |
| **filter_demo** — GPU Gaussian blur via `SceneBuilder` + `ImageFilterLayer` | `cargo run --example filter_demo` |
| **scene_render** — end-to-end GPU compositor proof | `cargo run --example scene_render` |
| **wgpu_window** — platform-driven GPU rendering integration test | `cargo run --example wgpu_window` |
| **image_demo** — interactive visual check of `RenderImage` | `cargo run --example image_demo` |
| **window_features** — cross-platform window API demo | `cargo run --example window_features` |

## Windows-specific

| Example | Run |
|---|---|
| **windows11_demo** — Mica backdrop, dark title bar, rounded corners, Snap Layouts | `cargo run --example windows11_demo` (Windows 11 Build 22000+; dark mode needs Windows 10 Build 17763+) |

## iOS

| Example | Run |
|---|---|
| **ios_demo** — the Material sample app on the native UIKit backend | `cargo xtask device ios-sim` (simulator-only; every other target compiles a no-op `main`) |

## Self-driving probes

Used by the `cargo xtask device macos-*` acceptance gates and `docs/BETA.md`'s dated evidence rows — not
interactive demos. Each drives itself (synthesized input, scripted resize/lifecycle
transitions) and asserts a `*_RESULT=PASS`/`FAIL` marker; macOS-only unless noted.

| Example | Run | What it proves |
|---|---|---|
| **lifecycle_probe** | `cargo xtask device macos-lifecycle` | Frame production survives minimize/restore, hide/unhide, resize |
| **workload_probe** | `cargo xtask device macos-workload` | Scroll/type p99 latency and RSS growth budgets under a representative workload |
| **a11y_probe** — the generated counter, run for an assistive technology | `cargo xtask device macos-a11y`, `cargo xtask device windows-a11y`, `cargo xtask device windows-input` (`cargo run --example a11y_probe --features material,a11y`) | An AXUIElement (macOS) or UI Automation (Windows) client can read the texts by name and press the button; on Windows, real clicks and Tab + Enter press it too |
| **resize_jitter_probe** | `cargo xtask device macos-resize-jitter` | Swapchain/surface size stays consistent through a live-resize burst |

## Hot reload

| Example | Run |
|---|---|
| **desktop_scene** — hot-reloadable scene plugin for desktop (Windows/macOS/Linux) | two terminals, see below |
| **hot_reload_counter** — the counter template through `flui run --hot` (host/logic/types split) | `cd examples/hot_reload_counter && flui run --hot` |
| **hot_reload_lifecycle_fixture** | Dev-only `app_plugin!` fixture for `flui-hot-reload`'s own integration test — not meant to be run directly. |

**desktop_scene** is a `cdylib` plugin, not a binary — there is no `cargo run -p flui-desktop-scene`. It's built in one terminal and loaded by a separate host process in another (full workflow: [`docs/hot-reload.md`](../docs/hot-reload.md#desktop-plugin-workflow)):

```bash
# Terminal 1 — rebuild the plugin on change (needs cargo-watch)
cargo watch -w examples/desktop_scene -x "build -p flui-desktop-scene"

# Terminal 2 — run the host with in-process reload (Linux/macOS; see
# docs/hot-reload.md for the Windows .dll path)
FLUI_SCENE_PLUGIN=target/debug/libflui_scene.so cargo run --example scene_render
```

## Web / WASM

| Example | Run |
|---|---|
| **web_demo** — Web/WASM platform demo | `cd examples/web_demo && wasm-pack build --target web --out-dir pkg`, then `cargo run -p flui-web-server` |
| **web_counter** — the counter template through `flui::run_app`, plain `cargo` + `wasm-bindgen` (no `wasm-pack`) | `cargo build -p flui-web-counter --locked --release --target wasm32-unknown-unknown`, then `wasm-bindgen --target web --out-dir examples/web_counter/pkg ${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/flui_web_counter.wasm`; serve `examples/web_counter/` over HTTP |
| **painting_demo** — Web/WASM painting + engine demo | `cd examples/painting_demo && wasm-pack build --target web --out-dir pkg` |

## Android

Excluded from default workspace members (require the Android NDK toolchain); build with
`cargo ndk`, e.g.:

```bash
cargo ndk -t arm64-v8a build -p flui-android-demo
```

| Crate | Purpose |
|---|---|
| **android_demo** | Colored rectangles via the GPU compositor |
| **android_scene** | Hot-reloadable scene plugin |
| **android_app** | Widget-based hot-reloadable plugin (`app_plugin!`) |

## Test fixtures (not directly runnable)

**vertical_slice_demo** has no `[[example]]` target — `tests/vertical_slice_demo.rs` and
`tests/demo_layer_snapshots.rs` `#[path]`-include its `main.rs`/`tree.rs` directly for the
vertical-slice acceptance test's render-tree inspection and layer-snapshot coverage.

## Building examples

```bash
cargo build --examples          # everything that doesn't need a feature
cargo run --example             # list every example target (the per-target crates are packages)
cargo run --example <name>      # run one by name
```

## Notes

- Windows-specific examples (`windows11_*`) only compile on Windows.
- macOS-only probes (`lifecycle_probe`, `workload_probe`, `a11y_probe`, `resize_jitter_probe`)
  skip with a message on other hosts, and so do the `cargo xtask device macos-*` checks that
  drive them (exit 0; `macos-hot-reload-loop` refuses with exit 1).
- `ios_demo` compiles a no-op `main` on every target except `aarch64-apple-ios-sim`.
- Examples with `required-features` fail to build under `--no-default-features` without the
  listed feature(s) — that's deliberate: it's the same false signal the isolated feature
  builds (`cargo xtask feature-matrix`) exist to catch.
