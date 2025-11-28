# FLUI Build Guide

Complete guide for building FLUI across all platforms.

## Table of Contents

- [Quick Start](#quick-start)
- [Cross-Platform Builds](#cross-platform-builds)
- [Platform Prerequisites](#platform-prerequisites)
- [Running Examples](#running-examples)
- [Benchmarks](#benchmarks)
- [Troubleshooting](#troubleshooting)

## Quick Start

```bash
# Build entire workspace
cargo build --workspace

# Run tests
cargo test --workspace

# Run example
cargo run --example counter_reactive

# Lint and format
cargo clippy --workspace -- -D warnings
cargo fmt --all
```

## Cross-Platform Builds

FLUI uses the **xtask** build system for cross-platform builds.

### Basic Commands

```bash
# Check environment and installed tools
cargo xtask info

# Build for Android (debug)
cargo xtask build android

# Build for Android (release)
cargo xtask build android --release

# Build for Web
cargo xtask build web --release

# Build for Desktop (Windows/Linux/macOS)
cargo xtask build desktop --release

# Clean build artifacts
cargo xtask clean --all
```

### Convenient Aliases

Defined in `.cargo/config.toml`:

```bash
cargo build-android-release
cargo build-web-release
cargo build-desktop-release
```

### Output Locations

- **Android**: `target/flui-out/android/flui-{debug|release}.apk`
- **Web**: `target/flui-out/web/` (ready to serve)
- **Desktop**: `target/flui-out/desktop/flui_app[.exe]`

## Platform Prerequisites

### Android

Required tools:
- Android SDK
- Android NDK
- Java JDK 11+
- `cargo install cargo-ndk`
- `rustup target add aarch64-linux-android`

**Setup:**

```bash
# Install cargo-ndk
cargo install cargo-ndk

# Add Android target
rustup target add aarch64-linux-android

# Set environment variables (add to your shell profile)
export ANDROID_HOME=/path/to/android/sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/25.2.9519653
```

**Building:**

```bash
# Debug build
cargo xtask build android

# Release build
cargo xtask build android --release

# Install on connected device
adb install target/flui-out/android/flui-release.apk
```

### Web (WASM)

Required tools:
- `cargo install wasm-pack`
- `rustup target add wasm32-unknown-unknown`

**Setup:**

```bash
# Install wasm-pack
cargo install wasm-pack

# Add wasm32 target
rustup target add wasm32-unknown-unknown
```

**Building:**

```bash
# Build for web
cargo xtask build web --release

# Serve locally (requires http server)
cd target/flui-out/web
python -m http.server 8000
# Open http://localhost:8000 in browser
```

### Desktop

**Windows:**
- MSVC Build Tools (Visual Studio 2019+)
- Windows 10 SDK

**macOS:**
- Xcode Command Line Tools
- macOS SDK

**Linux:**
- GCC/Clang
- Development libraries:
  ```bash
  # Ubuntu/Debian
  sudo apt install build-essential libx11-dev libxrandr-dev libxi-dev

  # Fedora/RHEL
  sudo dnf install gcc libX11-devel libXrandr-devel libXi-devel
  ```

**Building:**

```bash
# Build for current platform
cargo xtask build desktop --release

# Output: target/flui-out/desktop/flui_app[.exe]
```

## Running Examples

### Basic Examples

```bash
# Run hello world
cargo run --example hello_world_view

# Run reactive counter
cargo run --example counter_reactive

# Run todo app
cargo run --example todo_app
```

### Pipeline Examples

```bash
# Custom pipeline implementation
cargo run --example custom_pipeline

# Multi-threaded builds
cargo run --example parallel_builds
```

### Rendering Examples

```bash
# Custom RenderObject
cargo run --example custom_render

# Animation system
cargo run --example animation_demo

# Shader mask effects
cargo run --example shader_mask_gradient
cargo run --example shader_mask_vignette

# Backdrop filter
cargo run --example backdrop_filter_frosted
```

### With Logging

```bash
# Enable debug logging
RUST_LOG=debug cargo run --example counter_reactive

# Enable trace logging for specific module
RUST_LOG=flui_core=trace cargo run --example hello_world_view
```

## Benchmarks

```bash
# Run benchmarks for specific layers
cargo bench -p flui-reactivity   # Signal performance
cargo bench -p flui-pipeline     # Pipeline coordination
cargo bench -p flui_core         # Core framework
cargo bench -p flui_types        # Basic types
```

## Building Individual Crates

When making structural changes, build crates in dependency order:

### Foundation Layer

```bash
cargo build -p flui_types
cargo build -p flui-foundation
cargo build -p flui-tree
```

### Framework Layer

```bash
cargo build -p flui-view
cargo build -p flui-pipeline
cargo build -p flui-reactivity
cargo build -p flui-scheduler
cargo build -p flui_core
```

### Rendering Layer

```bash
cargo build -p flui_painting
cargo build -p flui_engine
cargo build -p flui_rendering
```

### Widget & Application Layer

```bash
cargo build -p flui_widgets
cargo build -p flui_animation
cargo build -p flui_interaction
cargo build -p flui_app
cargo build -p flui_assets
```

### Development Tools

```bash
cargo build -p flui_devtools
cargo build -p flui_cli
cargo build -p flui_build
```

## Testing

### All Tests

```bash
# Run all workspace tests
cargo test --workspace

# Run tests with logging
RUST_LOG=debug cargo test --workspace
```

### Foundation Layer

```bash
cargo test -p flui-foundation
cargo test -p flui-tree
cargo test -p flui-reactivity
```

### Framework Layer

```bash
cargo test -p flui_core
cargo test -p flui-pipeline
```

### Specific Crate

```bash
cargo test -p flui_widgets

# Run specific test
cargo test -p flui_widgets --test widget_tests

# Run with output
cargo test -p flui_core -- --nocapture
```

## Linting

```bash
# Check for warnings
cargo clippy --workspace -- -D warnings

# Fix automatically
cargo clippy --workspace --fix

# Format code
cargo fmt --all

# Check formatting without modifying
cargo fmt --all -- --check
```

## Release Builds

### Profile Configuration

Defined in `Cargo.toml`:

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "debuginfo"  # Strip debug symbols but keep android_main symbol
panic = "abort"
```

### Build Commands

```bash
# Desktop release
cargo build --release

# Android release
cargo xtask build android --release

# Web release
cargo xtask build web --release
```

## Troubleshooting

### Common Issues

#### Android Build Fails

**Error:** `ANDROID_NDK_HOME not set`

**Solution:**
```bash
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/25.2.9519653
```

**Error:** `cargo-ndk not found`

**Solution:**
```bash
cargo install cargo-ndk
```

#### Web Build Fails

**Error:** `wasm-pack not found`

**Solution:**
```bash
cargo install wasm-pack
rustup target add wasm32-unknown-unknown
```

#### Desktop Build Fails on Linux

**Error:** Missing X11 libraries

**Solution:**
```bash
# Ubuntu/Debian
sudo apt install libx11-dev libxrandr-dev libxi-dev

# Fedora/RHEL
sudo dnf install libX11-devel libXrandr-devel libXi-devel
```

#### wgpu Compilation Issues

FLUI uses wgpu 25.x due to known issues with wgpu 26.0+ and 27.0.x:

**Error:** `codespan-reporting` feature flag incompatibility

**Solution:** Stay on wgpu 25.x (already configured in workspace)

See: https://github.com/gfx-rs/wgpu/issues/7915

### Clean Build

If you encounter strange build errors:

```bash
# Clean everything
cargo clean

# Clean xtask outputs
cargo xtask clean --all

# Rebuild
cargo build --workspace
```

### Dependency Issues

```bash
# Update dependencies
cargo update

# Check for outdated dependencies
cargo outdated

# Audit for security issues
cargo audit
```

## Development Workflow

### Recommended Workflow

1. **Make changes**
2. **Format code**: `cargo fmt --all`
3. **Check lints**: `cargo clippy --workspace -- -D warnings`
4. **Run tests**: `cargo test --workspace`
5. **Build**: `cargo build --workspace`
6. **Test example**: `cargo run --example counter_reactive`
7. **Commit changes**

### Fast Iteration

For fast iteration during development:

```bash
# Build only changed crate
cargo build -p flui_widgets

# Run tests for specific crate
cargo test -p flui_widgets

# Use dev profile (faster compile, slower runtime)
cargo build  # instead of cargo build --release
```

### Documentation

```bash
# Generate and open documentation
cargo doc --workspace --no-deps --open

# Check documentation
cargo doc --workspace --no-deps

# Check for broken links
cargo doc --workspace --no-deps 2>&1 | grep warning
```

## Performance Profiling

### Tracy Profiling

```bash
# Build with tracy (when enabled)
# cargo build --features tracy

# Note: Tracy support currently disabled pending dependency fixes
```

### Criterion Benchmarks

```bash
# Run all benchmarks
cargo bench --workspace

# Run specific benchmark
cargo bench -p flui-reactivity

# Compare with baseline
cargo bench --workspace -- --save-baseline my-baseline
# Make changes
cargo bench --workspace -- --baseline my-baseline
```

## Asset Management

### Flui Assets

```bash
# Build with assets support
cargo build -p flui_assets --features images,network

# Run asset examples
cargo run --example basic_usage -p flui_assets
```

## Environment Variables

Useful environment variables for development:

```bash
# Logging
export RUST_LOG=debug                    # Enable debug logging
export RUST_LOG=flui_core=trace         # Trace specific module
export RUST_BACKTRACE=1                  # Enable backtraces

# Android
export ANDROID_HOME=/path/to/android/sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/version

# Build
export RUSTFLAGS="-C target-cpu=native"  # Optimize for current CPU
```

## CI/CD

### GitHub Actions

Example workflow for CI:

```yaml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo fmt --all -- --check
```

## Additional Resources

- **Main README**: [README.md](README.md)
- **Architecture Docs**: [docs/arch/](docs/arch/)
- **Development Patterns**: [PATTERNS.md](PATTERNS.md)
- **CLI Documentation**: [crates/flui_cli/README.md](crates/flui_cli/README.md)
