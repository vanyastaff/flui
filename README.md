# FLUI

[![CI](https://github.com/vanyastaff/flui/actions/workflows/ci.yml/badge.svg)](https://github.com/vanyastaff/flui/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](README.md#license)
[![MSRV: 1.98](https://img.shields.io/badge/MSRV-1.98-orange.svg)](README.md#minimum-supported-rust-version)

> A modular, Flutter-inspired declarative UI framework for Rust with GPU-accelerated rendering.

FLUI brings the proven three-tree architecture (View → Element → Render) to Rust, adapted to native ownership, type-safe arity, and a strict layered crate DAG. The Core.1 vertical slice is complete: the widget catalog (`flui-widgets`) is live, the full build → layout → paint → composite pipeline is exercised end-to-end, and the gesture/animation integration ships.

**Project stage: 0.x, beta candidate.** The `flui` CLI is on crates.io (`cargo install flui-cli --locked`; `flui create` scaffolds a project that pins the framework's `v0.1.0` git tag). The framework crates themselves are not yet published: they build and run from a clone (instructions below) or from that tag, and APIs may still change between minor versions. See [`CHANGELOG.md`](CHANGELOG.md) for notable changes and [`docs/ROADMAP.md`](docs/ROADMAP.md) for what lands next.

**Documentation:** the book at <https://vanyastaff.github.io/flui/> — still a skeleton being filled in (tracked as H2 in the beta roadmap). That URL 404s until GitHub Pages is enabled for this repository (Settings → Pages → Source = GitHub Actions, a one-time setting only the repo owner can make); use [`docs/getting-started.md`](docs/getting-started.md) below in the meantime.

The next milestone is a beta release; its user workflows and required evidence
are defined in [Beta release criteria](docs/BETA.md).

## Status

- ✅ Foundation: `flui-geometry`, `flui-types`, `flui-foundation`, `flui-macros`, `flui-log`, `flui-tree`, `flui-platform`
- ✅ Core: `flui-painting`, `flui-engine`, `flui-rendering`, `flui-scheduler`, `flui-layer`, `flui-semantics`, `flui-interaction`, `flui-hot-reload`
- ✅ Framework/application: `flui-view`, `flui-objects`, `flui-widgets`, `flui-localizations`, `flui-material`, `flui-cupertino`, `flui-testing`, `flui-animation`, `flui-assets`, `flui-app` (migration)
- ✅ DX/tooling: `flui-devtools` (partial), `flui-cli` (with the per-target build pipeline in `crates/flui-cli/src/build/`)

See [`docs/crates.md`](docs/crates.md) for the full layered map and per-crate status.

## Quick Start

Prerequisites: Rust 1.98 (edition 2024). The repository is a Cargo workspace consumed by path — clone and build. A `rust-toolchain.toml` is committed, so `rustup` will install and select the correct toolchain automatically.

```bash
git clone https://github.com/vanyastaff/flui
cd flui
cargo build --workspace
cargo run --example widgets_gallery
```

Repository tasks beyond plain `cargo` (the CI gates, the change-scoped pre-PR check, the device checks) are `cargo xtask <command>` — `cargo xtask --help` lists them. There is no separate task runner to install; `cargo xtask doctor` names the tools the gates use (cargo-nextest, typos, taplo, lychee, Python 3.10+) and how to install each.

For a step-by-step setup including platform notes (Windows / macOS / Android NDK / WASM), see [`docs/getting-started.md`](docs/getting-started.md).

## Choosing a catalog

The `flui` facade is **Material-first by default** and feature-selective. The
base surface — the widget catalog, the View/Element layer, animation, and
`run_app` — needs no feature at all.

```toml
# Default: the Material catalog, exactly as the quick start teaches it.
flui = { path = "…" }

# Cupertino only, no Material compiled.
flui = { path = "…", default-features = false, features = ["cupertino"] }

# Both catalogs.
flui = { path = "…", default-features = false, features = ["material", "cupertino"] }

# Catalog-free: still gets widgets, navigation, focus, and media information.
flui = { path = "…", default-features = false }
```

| Feature | Default | Enables |
|---|---|---|
| `material` | **on** | `flui::material` and the Material half of `flui::prelude` |
| `cupertino` | off | `flui::cupertino` |
| `localizations` | off | `flui::localizations` — global (multi-language) resources |
| `hot-reload` | off | desktop/Android development reload machinery; absent from an ordinary production graph |
| `a11y` | off | native accessibility: the AccessKit adapters that hand the semantics tree to VoiceOver / Narrator / Orca (off by default because the Linux adapter carries a D-Bus stack) |

A module whose feature is off is *absent*, not empty. Every supported
combination is compiled in isolation by CI (`cargo xtask facade-combos`), so a
combination cannot pass only because a sibling crate happened to enable a
feature. Web and iOS currently have no hot-reload runner integration; enabling
the additive feature there remains compile-safe but does not install a reload
driver.

## Key Features

- **Three-tree pipeline.** Immutable `View` → mutable `Element` → layout/paint `Render`. Build / Layout / Paint phases run on demand only.
- **Type-safe arity.** Render children parameterized by `Leaf`, `Single`, `Optional`, `Variable` — child-count mismatches become compile-time errors.
- **GPU-first rendering.** `wgpu` 30 backend with `lyon` tessellation, `cosmic-text` shaping, and an engine-owned glyph atlas for text.
- **Cross-platform, unevenly verified.** Native Win32 and AppKit backends, headless mode for CI, an Android NDK target, WASM/WebGPU, and a `winit` fallback all build, but how far each has actually been run and checked differs sharply by platform — macOS has live, operator-equivalent input evidence; Windows, Android, and Web/WASM are compile-checked only; Linux and iOS Simulator are experimental. See the [per-platform status table](docs/BETA.md#platform-status--candidate-this-branch-at-v010-and-after) before relying on a platform this project has not verified for you.
- **Hot-reload scenes.** `dlopen`-based plugin host (`flui-hot-reload`) for desktop iteration without process restarts.
- **Strict architecture.** Layered crate DAG with no upward edges. `unsafe` is *not* confined to a fixed crate list — it concentrates wherever a crate touches an FFI or ABI boundary. By unsafe-site count in `src/` (`rg -c '\bunsafe\s+(fn|impl|trait|extern)\b|\bunsafe\s*\{'`, measured 2026-08-04): `flui-platform` (Win32/AppKit/Android FFI) dominates by a wide margin, followed by `flui-rendering` (a miri-audited arena, `subtree_arena.rs`), `flui-hot-reload` (the `dlopen` ABI boundary), and `flui-engine` (wgpu/raw-window-handle FFI); smaller counts exist in `flui-layer`, `flui-foundation`, `flui-types`, `flui-log`, `flui-view`, and `flui-app`. `flui-painting` carries zero unsafe code today. Reviewed at the workspace level — see `docs/PANIC-POLICY.md` and each crate's `ARCHITECTURE.md`.

## Why FLUI

- **Flutter's layout protocol, not CSS.** The Rust GUI stacks that are not game engines lay out with Taffy (flexbox/grid): GPUI, Dioxus Native, Bevy UI, Vexo. FLUI implements Flutter's box and sliver protocols — constraints down, sizes up, one pass, with intrinsic dimensions, baselines, relayout boundaries, and `RenderSliver` for pinned, floating, and overlapping scrolling — because that is the model a Flutter developer expects, and the one Jetpack Compose independently converged on. Flutter's own rendering and widget tests are the oracle: 72 test files in this workspace are adapted from `packages/flutter/test` (see [`NOTICE`](NOTICE)).
- **Pure Rust, one toolchain.** rinf and flutter_rust_bridge put Rust logic inside a real Flutter app, so the Dart VM, the Flutter SDK, and a second build system ship with it. FLUI keeps the same mental model — declarative widgets over a retained three-tree, keys, lifecycle — as ordinary crates: `cargo build` is the whole build, `wgpu` is the one renderer on every platform, and there is no VM in the binary.
- **Resilience that is tested, not assumed.** The renderer recovers from GPU device loss and rebuilds its surface; a window reported as fully occluded stops submitting GPU work while input is still serviced; and both are exercised by a live end-to-end smoke test that drives a real window with real X11 input under Xvfb, checks the captured pixels and the exit code, and verifies occlusion against a real cover window — plus a Wayland variant for the close-path teardown order. Synthetic-event tests stayed green through every one of the platform-layer regressions that suite now catches.

## Hello World

An app is a `View` that builds other views — the widget layer drives the whole
pipeline (element tree → render objects → layout → paint → `wgpu`). The
minimal one is a piece of state and a button that updates it — the same
`CounterView`/`CounterState` shape `flui create`'s `counter` template
generates, kept in sync deliberately:

```rust
//! examples/counter.rs (excerpt)
use flui::prelude::*;
use flui::widgets::{SafeArea, column};

#[derive(Clone, StatelessView)]
struct CounterApp;

impl StatelessView for CounterApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(ThemeData::light(), SafeArea::new().child(CounterView))
    }
}

#[derive(Clone, StatefulView)]
struct CounterView;

struct CounterState {
    count: StateCell<usize>,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState { count: StateCell::new(0) }
    }
}

impl ViewState<CounterView> for CounterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count.bind(ctx);
    }

    fn build(&self, _view: &CounterView, _ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count.clone();
        Center::new().child(
            Column::new(column![
                Text::new(self.count.get().to_string()),
                ElevatedButton::new(Text::new("Increment"))
                    .on_pressed(move || count.update(|n| n + 1)),
            ])
            .main_axis_alignment(MainAxisAlignment::Center),
        )
    }
}

fn main() {
    run_app(CounterApp);
}
```

Run with `cargo run --example counter`. For a tour of the wider widget
catalog, see `examples/widgets_gallery.rs`; for the platform layer without
widgets (raw window + event loop, useful when debugging platform integration
itself), see `examples/platform_window.rs`. More examples live under
`examples/` and per-target crates (`examples/desktop_scene/`,
`examples/web_demo/`, `examples/painting_demo/`) — see
[`examples/README.md`](examples/README.md) for the full index.

## Minimum Supported Rust Version

The MSRV is **Rust 1.98**, declared as `rust-version` in the workspace
manifest (clippy reads it from there). `rust-toolchain.toml`'s `channel` is
the *development* toolchain pin; under the policy below the two are the same
release, so every CI job builds on the MSRV. `cargo xtask toolchain` (part of
`cargo xtask checks`) checks that `Cargo.toml`, the `flui-cli` project
templates, this README's badge and `llms.txt` all agree with it.

**Policy:** pre-1.0, the MSRV tracks the latest stable release and is bumped
within a week of each new stable (Rust ships every 6 weeks); after 1.0 it
follows N-2 (tolerates the two most recent stable releases behind current).
Every bump updates the manifest, the toolchain pin, this section, and CI
together (the procedure lives in `rust-toolchain.toml`'s header).

## Documentation

| Guide | Description |
|-------|-------------|
| **[Foundations](docs/FOUNDATIONS.md)** | **Architecture contract** — target architecture, locked contracts, target crate graph |
| **[Roadmap](docs/ROADMAP.md)** | **Port roadmap / construction plan** — dependency-ordered phases from current state to target |
| [Getting Started](docs/getting-started.md) | Prerequisites, build, run examples, platform-specific setup |
| [Architecture](docs/architecture.md) | Three-tree pipeline + layered crate DAG overview (current state) |
| [Crates Map](docs/crates.md) | Per-layer crate inventory with status and purpose |
| [Testing](docs/testing.md) | Build / test / clippy / fmt commands, coverage targets, benchmarks |

For deep architectural rules (dependency DAG, pipeline contracts, anti-patterns) see [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md).
For AI-agent guidance (build commands, architecture, troubleshooting) see [`AGENTS.md`](AGENTS.md).

## Community

- [Contributing](CONTRIBUTING.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Security Policy](SECURITY.md)

## License

Licensed under either the [MIT License](LICENSE) or the [Apache License, Version 2.0](LICENSE-APACHE) at your option. The workspace `Cargo.toml` declares `MIT OR Apache-2.0`.

## Acknowledgments

FLUI takes [Flutter](https://flutter.dev) as its behavioral reference: the three-tree model, the box/sliver layout protocol, lifecycle ordering, and Flutter's test corpus are the floor it must meet, and 72 test files here are adapted from `packages/flutter/test`. Structure and mechanisms are designed for Rust and diverge deliberately, with each divergence recorded in an ADR or a crate's `## Mapping decisions`. Flutter is Copyright 2014 The Flutter Authors, BSD-3-Clause — see [`NOTICE`](NOTICE) for the attribution that ships with the affected crates. Flutter is a trademark of Google LLC; FLUI is not affiliated with or endorsed by Google.

[GPUI](https://www.gpui.rs/) (Zed Industries, Apache-2.0) is consulted as a design reference for the platform layer; nothing is copied from it. Maintainer checkouts may include local `.flutter/` and `.gpui/` mirrors for that reference work.
