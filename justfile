# --- FLUI justfile ---
# Cross-platform task runner for the FLUI Rust workspace.
# Usage: just [recipe]
# Install: https://just.systems/man/en/

set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]
set dotenv-load
set export
set positional-arguments

# --- Variables ---
version := `git describe --tags --always --dirty 2>/dev/null || echo "dev"`
commit  := `git rev-parse --short HEAD 2>/dev/null || echo "unknown"`

# Active workspace members (must match crates/* in Cargo.toml [workspace.members])
active_crates := "flui-types flui-foundation flui-tree flui-platform flui-painting flui-semantics flui-scheduler flui-layer flui-interaction flui-engine flui-log flui-hot-reload flui-rendering flui-view flui-app"

# Default recipe — show help
[doc("Show available recipes grouped by category")]
default:
    @just --list --unsorted

# =============================================================================
# Build
# =============================================================================

[group("build")]
[doc("Type-check the entire workspace (fast, no codegen)")]
check:
    cargo check --workspace --all-targets

[group("build")]
[doc("Build the workspace (default profile)")]
build:
    cargo build --workspace

[group("build")]
[doc("Build the workspace in release mode (LTO enabled)")]
build-release:
    cargo build --workspace --release

[group("build")]
[doc("Build a single crate by name (e.g. just build-crate flui-engine)")]
build-crate crate:
    cargo build -p {{crate}}

[group("build")]
[doc("Build foundation layer first, then up the DAG (manual incremental build)")]
build-layered:
    cargo build -p flui-types
    cargo build -p flui-foundation
    cargo build -p flui-tree
    cargo build -p flui-platform
    cargo build -p flui-painting
    cargo build -p flui-semantics
    cargo build -p flui-scheduler
    cargo build -p flui-layer
    cargo build -p flui-interaction
    cargo build -p flui-engine
    cargo build -p flui-log
    cargo build -p flui-hot-reload
    cargo build -p flui-rendering
    cargo build -p flui-view
    cargo build -p flui-app

# =============================================================================
# Testing
# =============================================================================

[group("test")]
[doc("Run all tests across the workspace")]
test *args:
    cargo test --workspace {{args}}

[group("test")]
[doc("Test a single crate (e.g. just test-crate flui-tree)")]
test-crate crate *args:
    cargo test -p {{crate}} {{args}}

[group("test")]
[doc("Run a single named test with stdout/stderr surfaced (e.g. just test-name flui-tree element_id)")]
test-name crate name:
    cargo test -p {{crate}} {{name}} -- --nocapture

[group("test")]
[doc("Run tests with debug logging (RUST_LOG=debug)")]
test-debug *args:
    RUST_LOG=debug cargo test --workspace {{args}}

[group("test")]
[doc("Run tests, keep going after the first failure")]
test-all:
    cargo test --workspace --no-fail-fast

[group("test")]
[doc("Run tests against the release profile")]
test-release:
    cargo test --workspace --release

[group("test")]
[doc("Generate an HTML coverage report (requires cargo-llvm-cov)")]
coverage:
    cargo llvm-cov --workspace --html
    @echo "Coverage report: target/llvm-cov/html/index.html"

# =============================================================================
# Quality gates
# =============================================================================

[group("quality")]
[doc("Run clippy on the workspace; fail on warnings (CI gate)")]
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

[group("quality")]
[doc("Run clippy and apply auto-fixes (uncommitted changes only)")]
clippy-fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty -- -D warnings

[group("quality")]
[doc("Format the entire workspace with rustfmt")]
fmt:
    cargo fmt --all

[group("quality")]
[doc("Check formatting without modifying files (CI gate)")]
fmt-check:
    cargo fmt --all -- --check

[group("quality")]
[doc("Build rustdoc for FLUI crates only")]
doc:
    cargo doc --workspace --no-deps

[group("quality")]
[doc("Build rustdoc and open in browser")]
doc-open:
    cargo doc --workspace --no-deps --open

[group("quality")]
[doc("Build rustdoc with -D warnings (CI gate)")]
doc-strict:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# =============================================================================
# Benchmarks
# =============================================================================

[group("bench")]
[doc("Run benchmarks for a single crate (criterion)")]
bench crate:
    cargo bench -p {{crate}}

[group("bench")]
[doc("Run benchmarks across the workspace")]
bench-all:
    cargo bench --workspace

# =============================================================================
# Examples
# =============================================================================

[group("examples")]
[doc("Run the hello_world platform smoke test")]
example-hello:
    cargo run --example hello_world

[group("examples")]
[doc("Run an example by name (e.g. just example direct_render)")]
example name:
    cargo run --example {{name}}

[group("examples")]
[doc("Run the desktop_scene hot-reload example")]
example-desktop-scene:
    cargo run -p desktop_scene

[group("examples")]
[doc("List all available examples")]
example-list:
    @ls examples/*.rs 2>/dev/null | xargs -n1 basename | sed 's/\.rs$//'
    @echo "(plus per-target crates under examples/: desktop_scene, web_demo, painting_demo, android_*)"

# =============================================================================
# Web / WASM
# =============================================================================

[group("web")]
[doc("Run the built-in dev server (wasm-pack + HTTP serve)")]
web-server:
    cargo run -p web-server

[group("web")]
[doc("Build examples/web_demo to WASM (requires wasm-pack)")]
web-demo-build:
    cd examples/web_demo && wasm-pack build --target web --out-dir pkg

[group("web")]
[doc("Build examples/painting_demo to WASM (requires wasm-pack)")]
painting-demo-build:
    cd examples/painting_demo && wasm-pack build --target web --out-dir pkg

# =============================================================================
# Android (NDK)
# =============================================================================

[group("android")]
[doc("Build the Android GPU demo for arm64 (requires cargo-ndk + Android NDK)")]
android-demo target="arm64-v8a":
    cargo ndk -t {{target}} build -p flui-android-demo

[group("android")]
[doc("Build the Android scene plugin (requires cargo-ndk + Android NDK)")]
android-scene target="arm64-v8a":
    cargo ndk -t {{target}} build -p flui-android-scene

[group("android")]
[doc("Build the widget-based Android plugin (requires cargo-ndk + Android NDK)")]
android-app target="arm64-v8a":
    cargo ndk -t {{target}} build -p flui-android-app

# =============================================================================
# Setup
# =============================================================================

[group("setup")]
[doc("Install development tools used by the workspace")]
setup:
    rustup component add clippy rustfmt
    cargo install --locked cargo-llvm-cov
    cargo install --locked cargo-watch
    @echo ""
    @echo "Optional, for cross-target builds:"
    @echo "  cargo install --locked wasm-pack       # for examples/web_demo, examples/painting_demo"
    @echo "  cargo install --locked cargo-ndk        # for examples/android_*"

[group("setup")]
[doc("Show installed Rust toolchain and FLUI workspace info")]
info:
    @rustc --version
    @cargo --version
    @echo "Active workspace members: {{active_crates}}"
    @echo "Version: {{version}} (commit {{commit}})"

# =============================================================================
# Watch mode
# =============================================================================

[group("watch")]
[doc("Re-run check on file change (requires cargo-watch)")]
watch:
    cargo watch -x "check --workspace"

[group("watch")]
[doc("Re-run tests on file change (requires cargo-watch)")]
watch-test crate="":
    cargo watch -x "test {{ if crate == '' { '--workspace' } else { '-p ' + crate } }}"

# =============================================================================
# CI aggregate
# =============================================================================

[group("ci")]
[doc("Run the three quality gates (fmt-check + clippy + test)")]
ci: fmt-check clippy test

# =============================================================================
# Maintenance
# =============================================================================

[confirm("Remove target/ and all build artifacts?")]
[group("maintenance")]
[doc("Wipe target/ directory and Cargo build artifacts")]
clean:
    cargo clean

[group("maintenance")]
[doc("Update workspace dependencies (Cargo.lock)")]
update:
    cargo update --workspace

[group("maintenance")]
[doc("Audit dependencies for known vulnerabilities (requires cargo-audit)")]
audit:
    cargo audit

[group("maintenance")]
[doc("Show outdated dependencies (requires cargo-outdated)")]
outdated:
    cargo outdated --workspace
