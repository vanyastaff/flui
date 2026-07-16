# FLUI

[![CI](https://github.com/vanyastaff/flui/actions/workflows/ci.yml/badge.svg)](https://github.com/vanyastaff/flui/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](README.md#license)
[![MSRV: 1.96](https://img.shields.io/badge/MSRV-1.96-orange.svg)](README.md#minimum-supported-rust-version)

> A modular, Flutter-inspired declarative UI framework for Rust with GPU-accelerated rendering.

FLUI brings the proven three-tree architecture (View → Element → Render) to Rust, adapted to native ownership, type-safe arity, and a strict layered crate DAG. The Core.1 vertical slice is complete: the widget catalog (`flui-widgets`) is live, the full build → layout → paint → composite pipeline is exercised end-to-end, and the gesture/animation integration ships.

**Project stage: pre-release — build from source.** FLUI is not published to crates.io; the workspace builds and runs from a clone (instructions below), and APIs may still change between commits. See [`CHANGELOG.md`](CHANGELOG.md) for notable changes and [`docs/ROADMAP.md`](docs/ROADMAP.md) for what lands next.

## Status

- ✅ Foundation: `flui-geometry`, `flui-types`, `flui-foundation`, `flui-macros`, `flui-tree`, `flui-platform`
- ✅ Core: `flui-painting`, `flui-engine`, `flui-rendering`, `flui-scheduler`, `flui-layer`, `flui-semantics`, `flui-interaction`, `flui-hot-reload`
- ✅ Framework/application: `flui-view`, `flui-objects`, `flui-widgets`, `flui-localizations`, `flui-binding`, `flui-animation`, `flui-assets`, `flui-app` (migration)
- ✅ DX/tooling: `flui-devtools` (partial), `flui-cli`, `flui-build`
- ⏸️ Disabled until integration completes: `flui-reactivity`

See [`docs/crates.md`](docs/crates.md) for the full layered map and per-crate status.

## Quick Start

Prerequisites: Rust 1.96 (edition 2024). The repository is a Cargo workspace consumed by path — clone and build. A `rust-toolchain.toml` is committed, so `rustup` will install and select the correct toolchain automatically.

```bash
git clone https://github.com/vanyastaff/flui
cd flui
cargo build --workspace
cargo run --example widgets_gallery
```

A [`justfile`](justfile) is provided for common tasks — install [`just`](https://just.systems) and run `just` for the recipe list (`just check`, `just test`, `just clippy`, `just ci`, ...). Raw `cargo` commands always work too.

For a step-by-step setup including platform notes (Windows / macOS / Android NDK / WASM), see [`docs/getting-started.md`](docs/getting-started.md).

## Key Features

- **Three-tree pipeline.** Immutable `View` → mutable `Element` → layout/paint `Render`. Build / Layout / Paint phases run on demand only.
- **Type-safe arity.** Render children parameterized by `Leaf`, `Single`, `Optional`, `Variable` — child-count mismatches become compile-time errors.
- **GPU-first rendering.** `wgpu` 29.x backend with `lyon` tessellation and `cosmic-text` / `glyphon` for high-quality text.
- **Cross-platform.** Native Win32 and AppKit backends, headless mode for CI, Android NDK target, WASM/WebGPU, plus a `winit` fallback.
- **Hot-reload scenes.** `dlopen`-based plugin host (`flui-hot-reload`) for desktop iteration without process restarts.
- **Strict architecture.** Layered crate DAG with no upward edges. `unsafe` is confined to `flui-platform`, `flui-painting`, `flui-engine`. Constitution-mandated and reviewed at the workspace level.

## Hello World

An app is a `View` that builds other views — the widget layer drives the whole
pipeline (element tree → render objects → layout → paint → `wgpu`):

```rust
//! examples/widgets_gallery.rs (excerpt)
use flui_app::run_app;
use flui_widgets::prelude::*;
use flui_widgets::{column, row};

/// A circular colour avatar: a coloured box clipped to an inscribed oval.
fn avatar(color: Color) -> ClipOval {
    ClipOval::new().child(ColoredBox::new(color).child(SizedBox::square(64.0)))
}

#[derive(Clone, StatelessView)]
struct Gallery;

impl StatelessView for Gallery {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Container::new()
            .color(Color::rgb(18, 18, 24))
            .padding(EdgeInsets::all(px(24.0)))
            .child(Column::new(column![
                Text::new("FLUI widget gallery"),
                SizedBox::height(16.0),
                Row::new(row![
                    avatar(Color::rgb(229, 57, 53)),
                    SizedBox::width(12.0),
                    avatar(Color::rgb(30, 136, 229)),
                ]),
                Container::new()
                    .color(Color::rgb(38, 38, 48))
                    .padding(EdgeInsets::all(px(16.0)))
                    .child(Center::new().child(Text::new("centered in a card"))),
            ]))
    }
}

fn main() {
    run_app(Gallery);
}
```

Run with `cargo run --example widgets_gallery`. For the platform layer without
widgets (raw window + event loop), see `examples/hello_world.rs`; more examples
live under `examples/` and per-target crates (`examples/desktop_scene/`,
`examples/web_demo/`, `examples/painting_demo/`).

## Minimum Supported Rust Version

The MSRV is **Rust 1.96**, declared as `rust-version` in the workspace
manifest, pinned by `rust-toolchain.toml`, and verified by a dedicated CI job.
Policy: pre-release, the MSRV may be bumped in any commit when a dependency or
language feature warrants it; every bump updates the manifest, the toolchain
pin, this section, and CI together (the procedure lives in
`rust-toolchain.toml`'s header).

## Documentation

| Guide | Description |
|-------|-------------|
| **[Foundations](docs/FOUNDATIONS.md)** | **Architecture contract** — target architecture, locked contracts, target crate graph |
| **[Roadmap](docs/ROADMAP.md)** | **Port roadmap / construction plan** — dependency-ordered phases from current state to target |
| [Getting Started](docs/getting-started.md) | Prerequisites, build, run examples, platform-specific setup |
| [Architecture](docs/architecture.md) | Three-tree pipeline + layered crate DAG overview (current state) |
| [Crates Map](docs/crates.md) | Per-layer crate inventory with status and purpose |
| [Testing](docs/testing.md) | Build / test / clippy / fmt commands, coverage targets, benchmarks |
| [Contributing](docs/contributing.md) | Constitution, commits, speckit workflow, AI Factory skills |

For deep architectural rules (dependency DAG, pipeline contracts, anti-patterns) see [`docs/FOUNDATIONS.md`](docs/FOUNDATIONS.md).
For Claude Code-specific guidance (build commands, troubleshooting) see [`CLAUDE.md`](CLAUDE.md).

## Community

- [Contributing](CONTRIBUTING.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Security Policy](SECURITY.md)

## License

Licensed under either the [MIT License](LICENSE) or the [Apache License, Version 2.0](LICENSE-APACHE) at your option. The workspace `Cargo.toml` declares `MIT OR Apache-2.0`.

## Acknowledgments

Patterns are adapted from the [Flutter](https://flutter.dev) framework and the [GPUI](https://www.gpui.rs/) Rust UI library. Maintainer checkouts may include local `.flutter/` and `.gpui/` reference mirrors for parity work; they are studied, never copied, and patterns are translated to FLUI idioms.
