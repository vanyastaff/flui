# FLUI Examples

This directory contains examples demonstrating FLUI framework features, from the minimal
"hello world" through engine-internals probes. Most are plain `examples/*.rs` files runnable
with `cargo run --example <name>`; a few need a Cargo feature (noted below), and a few are
separate crates or WASM targets with their own build step.

## Start Here

| Example | Run |
|---|---|
| **counter** — the minimal FLUI app: one `Signal` and a `RawButton` whose press writes it (start here — this is the README's own "Hello World" code sample, and the exact shape `flui create`'s `counter` template generates) | `cargo run --example counter` |
| **two_screens** — the Notes showcase: navigation as a value (a `#[derive(Routable)]` route enum, `WidgetsApp::router`, pages that push and pop through the `RouterHandle` they take in `init_state`), state kept above the router, loading/error/retry, a lazy 10,000-row list and validated editing. See its [README](two_screens/README.md) | `cargo run --example two_screens --features material` |
| **widgets_gallery** — a tour of the wider `flui-widgets` catalog through `flui::prelude` + `run_app` | `cargo run --example widgets_gallery` |
| **interaction_gallery** — one screen per interaction widget (swipe actions, dismissible rows, pull to refresh, a collapsing header, a pager, a bottom sheet, zoom and pan, drag and drop, a context menu, a gesture pad, keyboard focus), each with a readout of its last outcome; one widget tree for desktop, Android (`android_main`) and the web (`start`). `cargo xtask device windows-gallery` drives every screen with real input on Windows | `cargo run -p flui-interaction-gallery --features a11y` |
| **colored_box_app** — the first FLUI application through the real pipeline, at the low-level `flui-view`/`flui-objects` layer (no facade) | `cargo run --example colored_box_app` |
| **text_app** — the text pipeline end-to-end through the real framework | `cargo run --example text_app` |

## Widget catalogs (Material / Cupertino)

| Example | Run |
|---|---|
| **material_demo** | `cargo run --example material_demo --features material` |
| **cupertino_demo** | `cargo run --example cupertino_demo --features cupertino` |
| **sliver_demo** — a pinned `SliverAppBar` collapsing demo | `cargo run --example sliver_demo --features material` |
| **screenshot** — headless screenshot of a demo widget tree, no window | `cargo run --example screenshot --features material,cupertino` |

## Rendering / engine internals

| Example | Run |
|---|---|
| **animated_box_app** — the animation engine driving the real pipeline on GPU | `cargo run --example animated_box_app` |
| **scene_render** — end-to-end GPU compositor proof | `cargo run --example scene_render` |

## iOS

| Example | Run |
|---|---|
| **ios_demo** — the Material sample app on the native UIKit backend | `cargo xtask device ios-sim` (simulator-only; every other target compiles a no-op `main`) |

## Self-driving probes

Used by the `cargo xtask device macos-*` acceptance gates and the runs `docs/platform-parity.md` cites — not
interactive demos. Each drives itself (synthesized input, scripted resize/lifecycle
transitions) and asserts a `*_RESULT=PASS`/`FAIL` marker; macOS-only unless noted.

| Example | Run | What it proves |
|---|---|---|
| **lifecycle_probe** | `cargo xtask device macos-lifecycle` | Frame production survives minimize/restore, hide/unhide, resize |
| **workload_probe** | `cargo xtask device macos-workload` | Scroll/type p99 latency and RSS growth budgets under a representative workload |
| **a11y_probe** — a Material counter, run for an assistive technology | `cargo xtask device macos-a11y`, `cargo xtask device windows-a11y`, `cargo xtask device windows-input` (`cargo run --example a11y_probe --features material,a11y`) | An AXUIElement (macOS) or UI Automation (Windows) client can read the texts by name and press the button; on Windows, real clicks and Tab + Enter press it too, and with `FLUI_PROBE_FIXTURE=roles` it shows one node per semantics role and per family state for `windows-a11y` |
| **resize_jitter_probe** | `cargo xtask device macos-resize-jitter` | Swapchain/surface size stays consistent through a live-resize burst |

## Hot reload

| Example | Run |
|---|---|
| **desktop_scene** — hot-reloadable scene plugin for desktop (Windows/macOS/Linux) | two terminals, see below |
| **hot_reload_counter** — the counter template through `flui run`, which hot-reloads by default (host/logic/types split) | `cd examples/hot_reload_counter && flui run` |
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
| **web_demo** — Web/WASM platform demo | `cd examples/web_demo && wasm-pack build --target web --out-dir pkg`; serve `examples/web_demo/` over HTTP |
| **painting_demo** — Web/WASM painting + engine demo | `cd examples/painting_demo && wasm-pack build --target web --out-dir pkg`; serve `examples/painting_demo/` over HTTP |

The `painting_demo` crate contains browser code only and targets
`wasm32-unknown-unknown`. Its surface takes ownership of the `HtmlCanvasElement`
through wgpu's safe `SurfaceTarget::Canvas` API. Native workspace checks compile
an empty cdylib; they do not exercise the browser demo.

## Android

An Android app is a project generated by `flui create`. From its directory, `flui build android`
builds the APK and `flui run --device <id>` (ids from `flui devices`) installs and starts it; see
`crates/flui-cli/README.md`.

## Test fixtures

**vertical_slice_demo** runs as an example (`cargo run --example vertical_slice_demo`), and
`tests/vertical_slice_demo.rs` and `tests/demo_layer_snapshots.rs` `#[path]`-include its
`tree.rs` for the vertical-slice acceptance test's render-tree inspection and layer-snapshot
coverage.

## Building examples

```bash
cargo build --examples          # everything that doesn't need a feature
cargo run --example             # list every example target (the per-target crates are packages)
cargo run --example <name>      # run one by name
```

## Notes

- macOS-only probes (`lifecycle_probe`, `workload_probe`, `a11y_probe`, `resize_jitter_probe`)
  skip with a message on other hosts, and so do the `cargo xtask device macos-*` checks that
  drive them (exit 0; `macos-hot-reload-loop` refuses with exit 1).
- `ios_demo` compiles a no-op `main` on every target except `aarch64-apple-ios-sim`.
- Examples with `required-features` fail to build under `--no-default-features` without the
  listed feature(s) — that's deliberate: it's the same false signal the isolated feature
  builds (`cargo xtask feature-matrix`) exist to catch.
