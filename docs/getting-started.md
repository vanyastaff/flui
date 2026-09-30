[Back to README](../README.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Architecture →](architecture.md)

# Getting Started

This page covers prerequisites, the first build, and how to run the bundled examples.

## Prerequisites

| Tool | Minimum version | Notes |
|------|-----------------|-------|
| Rust | 1.98 | MSRV floor in `workspace.package.rust-version`; development toolchain pinned separately in `rust-toolchain.toml`. `rustup` installs/selects it automatically on first `cargo` invocation. |
| Cargo | bundled with Rust | Workspace uses `resolver = "3"` (MSRV-aware) and edition 2024. |
| Git | any recent | Required to clone the repo. |
| Python | 3.10+ | Only for the font-fixture generator (`tools/decoy-face/generate.py`, which `cargo xtask checks` re-runs to verify the committed bytes) and the macOS/iOS drivers in `tools/device-checks/`. Not required to build or run an application. |
| `cargo-ndk` | 3.x | Required only for Android targets. |
| `wasm-pack` | 0.13+ | Required only for `examples/web_demo` and `examples/painting_demo`. |
| Native toolchain | platform-specific | MSVC on Windows, Xcode CLT on macOS, NDK on Android. |

Repository tasks run as `cargo xtask <command>` (`cargo xtask --help` lists
them); there is no other task runner to install. `cargo xtask doctor` checks
every tool `cargo xtask ci` needs and prints the install command for each
missing one (`cargo xtask doctor full` adds what `cargo xtask ci-full` needs).

The GPU dependency version is defined by `wgpu` in `[workspace.dependencies]`
in `Cargo.toml`; consult that manifest when checking driver or backend requirements.

### MSRV policy

Pre-1.0, the MSRV tracks the latest stable release and is bumped within a
week of each new stable (Rust ships every 6 weeks); post-1.0 it follows N-2
(tolerates the two most recent stable releases behind current). A bump
touches `rust-toolchain.toml`, `Cargo.toml` (clippy reads the MSRV from it),
the `flui-cli` project templates, the README badge and `llms.txt` together —
`cargo xtask toolchain` (part of `cargo xtask checks`) fails if any of them
drift from `rust-toolchain.toml`'s channel. Full procedure:
`rust-toolchain.toml`'s header comment.

## Clone and Build

```bash
git clone https://github.com/vanyastaff/flui
cd flui
cargo build --workspace
```

The workspace builds in dependency order automatically (foundation → core → rendering → framework → application). See [`crates.md`](crates.md) for the crate map and `Cargo.toml` for workspace membership.

For a clean rebuild:

```bash
cargo clean
cargo build --workspace
```

## Create an Application

The `flui` CLI (`flui-cli` 0.1.0) is on crates.io; the framework crates are
not. A CLI installed with `cargo install flui-cli --locked` generates a
project that depends on the FLUI git tag matching the CLI's version (see
[`crates/flui-cli/README.md`](../crates/flui-cli/README.md)). To build against
this checkout instead, run these commands from the checkout root:

```bash
cargo install --path crates/flui-cli --locked
flui create my_app --local --path ../apps
cd ../apps/my_app
flui run
```

The generated application can live outside the FLUI repository. Bare `--local`
uses the current directory as its source checkout; from another directory, use
`--local=/path/to/flui`. Quote paths with spaces, for example
`--local="/path with spaces/flui"`. The source must remain available because
the generated `flui` dependency points to the checkout root by absolute path.
`flui` is the application's only framework dependency. Start UI code with
`use flui::prelude::*;`; Cargo dependency renames are supported by its derives.
The facade turns no design system on by default: to use `flui::material` or
`flui::cupertino`, add `features = ["material"]` (or `"cupertino"`) to the `flui`
dependency in the generated `Cargo.toml`.

Add `--hot-reload` to `flui create` to generate the host/worker/types workspace
used by the reload runner. See the [CLI guide](../crates/flui-cli/README.md) for
template and build options. Current release requirements and unverified areas
are tracked in [Beta release criteria](BETA.md).

## Run an Example

The simplest entry point is the counter — one piece of state and a button,
the same shape `flui create`'s `counter` template generates (see the
[Hello World example in the README](../README.md#hello-world) for the
source):

```bash
cargo run --example counter
```

A window should open showing a count and an "Increment" button; press it to
watch the count go up. Close the window to terminate the process. For a
tour of the wider widget catalog, run the gallery instead:

```bash
cargo run --example widgets_gallery
```

A window titled "FLUI App" (the `AppConfig` default) should open, showing a
dark padded surface with a title, a row of circular avatars, and a centred
card. Close it to terminate the process.

### Platform layer without widgets (advanced)

`examples/platform_window.rs` drives the platform layer directly — a raw
window and event loop, with no `View`/`Element`/render tree involved. It is
useful when debugging platform integration itself, not as a first example of
application code:

```bash
cargo run --example platform_window
```

Expected output (truncated; the platform name and display details reflect
your own OS and hardware, not the values below):

```
INFO FLUI platform window example
INFO Platform: <platform, e.g. macos / windows / linux>
INFO Platform initialized: "<Platform>"
INFO Found 1 display(s):
INFO   Display 1: <adapter name> (<width>x<height> @ <scale>x scale)
INFO Creating window...
```

A window titled "FLUI Platform Window" should open. Close it to terminate
the process.

### Other bundled examples

| Example | Command | Purpose |
|---------|---------|---------|
| `counter` | `cargo run --example counter` | Minimal stateful app — one `Signal` and a `RawButton` that writes it (start here) |
| `widgets_gallery` | `cargo run --example widgets_gallery` | Widget catalog through `flui::prelude` + `run_app` |
| `platform_window` | `cargo run --example platform_window` | Platform-layer smoke test (raw window, no widgets) |
| `direct_render` | `cargo run --example direct_render` | Manual GPU pipeline driving |
| `scene_render` | `cargo run --example scene_render` | Scene graph rendering |
| `wgpu_window` | `cargo run --example wgpu_window` | Raw `wgpu` window setup |
| `window_features` | `cargo run --example window_features` | Window option matrix |
| `windows11_demo` | `cargo run --example windows11_demo` | Windows 11 platform features |
| `desktop_scene` | `cargo build -p flui-desktop-scene` | Build the hot-reload scene library; this plugin is loaded by a host, not run as an executable |

### Web (WASM) examples

```bash
cd examples/web_demo
wasm-pack build --target web --out-dir pkg
```

Then serve `examples/web_demo/` with any static HTTP server and open its `index.html`. A project generated by `flui create` builds, serves and reloads itself with `flui run --device browser:chrome` (browser ids from `flui devices`).

### Android

An Android app is a project generated by `flui create`. From its directory, `flui build android` builds the APK and `flui run --device <id>` (ids from `flui devices`) installs and starts it; the toolchain it needs is in [`crates/flui-cli/README.md`](../crates/flui-cli/README.md).

## Verify the Toolchain

```bash
cargo check -p flui-foundation
cargo check -p flui-painting
cargo check -p flui-platform
```

If any of these fail, the toolchain or environment is misconfigured before any framework-level issue is reachable.

## Logging

All FLUI code logs through `tracing`. Set `RUST_LOG` to control verbosity:

```bash
RUST_LOG=debug cargo run --example platform_window
RUST_LOG=flui_platform=trace,flui_engine=info cargo test -p flui-platform
```

## Troubleshooting

| Symptom | Likely cause |
|---------|--------------|
| `error: package 'flui-X' not found` | Package names take a hyphen (`-p flui-assets`), crate paths an underscore (`flui_assets::`). [`crates.md`](crates.md) lists the packages. |
| `error[E0432]: unresolved import 'flui_rendering::prelude::*'` | The crate is not in the dependency graph or the `prelude` module does not exist yet. |
| `error: linking with 'link.exe'` on Windows | Install Visual Studio Build Tools 2022 (Desktop development with C++). |
| `wgpu` crashes or shows blank window | Update graphics drivers; `cargo update -p wgpu` to pick up any patch-level fixes. |
| Long build times | Use `cargo build --workspace` once, then incremental `cargo check -p <crate>`. `.cargo/config.toml` does not pin an alternate linker by default — the earlier macOS `lld` requirement was removed in favor of the default Apple linker (see [Beta release criteria](BETA.md)), and the Linux `mold`/`clang` opt-in there is commented out because not every environment has `mold` installed. Uncomment it locally if you have `mold` and want faster links. |

## See Also

- [Architecture](architecture.md) — three-tree pipeline and crate DAG
- [Crates Map](crates.md) — per-layer crate inventory and status
- [Testing](testing.md) — running the test suite and benchmarks
