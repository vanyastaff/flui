# --- FLUI justfile ---
# Cross-platform task runner for the FLUI Rust workspace.
# Usage: just [recipe]
# Install: https://just.systems/man/en/

set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load
set export
set positional-arguments

# --- Variables ---
version := `git describe --tags --always --dirty 2>/dev/null || echo "dev"`
commit  := `git rev-parse --short HEAD 2>/dev/null || echo "unknown"`

# Active workspace members (must match crates/* in Cargo.toml [workspace.members])
active_crates := "flui-animation flui-app flui-assets flui-binding flui-build flui-cli flui-devtools flui-engine flui-foundation flui-geometry flui-hot-reload flui-interaction flui-layer flui-macros flui-objects flui-painting flui-platform flui-rendering flui-scheduler flui-semantics flui-tree flui-types flui-view flui-widgets"

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
    cargo build -p flui-geometry
    cargo build -p flui-types
    cargo build -p flui-foundation
    cargo build -p flui-macros
    cargo build -p flui-tree
    cargo build -p flui-platform
    cargo build -p flui-assets
    cargo build -p flui-painting
    cargo build -p flui-semantics
    cargo build -p flui-scheduler
    cargo build -p flui-layer
    cargo build -p flui-interaction
    cargo build -p flui-animation
    cargo build -p flui-engine
    cargo build -p flui-hot-reload
    cargo build -p flui-rendering
    cargo build -p flui-objects
    cargo build -p flui-view
    cargo build -p flui-widgets
    cargo build -p flui-binding
    cargo build -p flui-app
    cargo build -p flui-devtools
    cargo build -p flui-build
    cargo build -p flui-cli

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
[doc("Run rustdoc examples as tests (CI gate; nextest does not execute doctests)")]
test-doc:
    cargo test --workspace --exclude flui-platform --doc

[group("test")]
[doc("Run the flui-assets/Image feature-gated tests CI also runs (default = [] hides them otherwise)")]
test-assets:
    cargo nextest run -p flui-assets --features full
    cargo nextest run -p flui-widgets --features images --test image

[group("quality")]
[doc("Dependency audit: advisories, bans, licenses, sources (requires cargo-deny)")]
deny:
    cargo deny check

[group("test")]
[doc("Run miri on the flui-rendering subtree arena (the unsafe hot spot; requires nightly + miri component)")]
miri:
    cargo +nightly miri test -p flui-rendering --lib pipeline::owner::subtree_arena

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

[group("quality")]
[doc("Check docs/justfile crate inventories against Cargo metadata")]
inventory-check:
    bash scripts/check-workspace-inventory.sh

# =============================================================================
# Port methodology
# =============================================================================

[group("port")]
[doc("Run refusal-trigger grep regressions (21 triggers + FR-033 from docs/PORT.md)")]
port-check:
    bash scripts/port-check.sh

[group("port")]
[doc("Run refusal-trigger checks with verbose pass/fail per trigger + marker totals")]
port-check-verbose:
    bash scripts/port-check.sh -v

[group("port")]
[doc("Per-file breakdown of TODO(port) / PERF(port) / PORT NOTE markers across crates/")]
port-markers:
    bash scripts/port-check.sh -b

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
[doc("Run local CI gates (fmt-check + inventory + port-check + clippy + test + doctests)")]
ci: fmt-check inventory-check port-check clippy test test-doc

# =============================================================================
# Maintenance
# =============================================================================

[group("maintenance")]
[doc("Prune stale build artifacts: current-toolchain sweep + anything older than 7 days (requires cargo-sweep)")]
sweep:
    cargo sweep --installed
    cargo sweep --time 7

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
