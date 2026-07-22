[Back to README](../README.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Architecture →](architecture.md)

# Getting Started

This page covers prerequisites, the first build, and how to run the bundled examples.

## Prerequisites

| Tool | Minimum version | Notes |
|------|-----------------|-------|
| Rust | 1.96 | Pinned via `workspace.package.rust-version` **and** `rust-toolchain.toml` (channel `1.96.1`). `rustup` installs/selects it automatically on first `cargo` invocation. |
| Cargo | bundled with Rust | Workspace uses `resolver = "2"` and edition 2024. |
| Git | any recent | Required to clone the repo. |
| `cargo-ndk` | 3.x | Required only for Android targets. |
| `wasm-pack` | 0.13+ | Required only for `examples/web_demo` and `examples/painting_demo`. |
| Native toolchain | platform-specific | MSVC on Windows, Xcode CLT on macOS, NDK on Android. |

`wgpu` is currently at **29.x** and tracks the latest stable major (see `[workspace.dependencies]` in `Cargo.toml`).

## Clone and Build

```bash
git clone https://github.com/vanyastaff/flui
cd flui
cargo build --workspace
```

The workspace builds in dependency order automatically (foundation → core → rendering → framework → application). Several crates are intentionally disabled in `Cargo.toml` while integration is in progress; see [`crates.md`](crates.md) for the active set.

For a clean rebuild:

```bash
cargo clean
cargo build --workspace
```

## Run an Example

The simplest entry point is the platform smoke test:

```bash
cargo run --example hello_world
```

Expected output (truncated):

```
INFO flui Hello World!
INFO Platform: windows
INFO Platform initialized: "Windows"
INFO Found 1 display(s):
INFO   Display 1: Generic PnP Monitor (1920x1080 @ 1.0x scale)
INFO Creating window...
```

A window titled "Hello FLUI!" should open. Close it to terminate the process.

### Other bundled examples

| Example | Command | Purpose |
|---------|---------|---------|
| `hello_world` | `cargo run --example hello_world` | Platform initialization smoke test |
| `direct_render` | `cargo run --example direct_render` | Manual GPU pipeline driving |
| `scene_render` | `cargo run --example scene_render` | Scene graph rendering |
| `wgpu_window` | `cargo run --example wgpu_window` | Raw `wgpu` window setup |
| `window_features` | `cargo run --example window_features` | Window option matrix |
| `windows11_demo` | `cargo run --example windows11_demo` | Windows 11 platform features |
| `desktop_scene` | `cargo run -p desktop_scene` | Hot-reload-aware desktop scene plugin |

### Web (WASM) examples

```bash
# Built-in dev server (recommended)
cargo run -p web-server

# Or build manually with wasm-pack
cd examples/web_demo
wasm-pack build --target web --out-dir pkg
```

Open `http://localhost:8080` once `web-server` reports it is ready.

### Android examples

Android crates (`examples/android_demo`, `examples/android_scene`, `examples/android_app`) are excluded from `workspace.members` because they require the NDK toolchain. Build them with:

```bash
cargo ndk -t arm64-v8a build -p flui-android-demo
```

## Verify the Toolchain

```bash
cargo check -p flui-types
cargo check -p flui-foundation
cargo check -p flui-tree
cargo check -p flui-platform
```

If any of these fail, the toolchain or environment is misconfigured before any framework-level issue is reachable.

## Logging

All FLUI code logs through `tracing`. Set `RUST_LOG` to control verbosity:

```bash
RUST_LOG=debug cargo run --example hello_world
RUST_LOG=flui_platform=trace,flui_engine=info cargo test -p flui-platform
```

## Troubleshooting

| Symptom | Likely cause |
|---------|--------------|
| `error: package 'flui-X' not found` | The crate is currently disabled in `Cargo.toml` `[workspace.members]`. Check [`crates.md`](crates.md). |
| `error[E0432]: unresolved import 'flui_rendering::prelude::*'` | The crate is not in the dependency graph or the `prelude` module does not exist yet. |
| `error: linking with 'link.exe'` on Windows | Install Visual Studio Build Tools 2022 (Desktop development with C++). |
| `wgpu` crashes or shows blank window | Update graphics drivers; `cargo update -p wgpu` to pick up any patch-level fixes. |
| Long build times | Use `cargo build --workspace` once, then incremental `cargo check -p <crate>`. The release linker is pinned per-target in `.cargo/config.toml`. |

## See Also

- [Architecture](architecture.md) — three-tree pipeline and crate DAG
- [Crates Map](crates.md) — per-layer crate inventory and status
- [Testing](testing.md) — running the test suite and benchmarks
