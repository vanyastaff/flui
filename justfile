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
active_crates := "flui-animation flui-app flui-assets flui-testing flui-build flui-cli flui-cupertino flui-devtools flui-engine flui-foundation flui-geometry flui-hot-reload flui-interaction flui-layer flui-localizations flui-log flui-macros flui-material flui-objects flui-painting flui-platform flui-rendering flui-scheduler flui-semantics flui-tree flui-types flui-view flui-widgets"

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
    cargo build -p flui-log
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
    cargo build -p flui-localizations
    cargo build -p flui-material
    cargo build -p flui-cupertino
    cargo build -p flui-testing
    cargo build -p flui-app
    cargo build -p flui-devtools
    cargo build -p flui-build
    cargo build -p flui-cli

[group("build")]
[doc("Type-check the wasm-capable crates for wasm32-unknown-unknown (mirrors the CI wasm-check job)")]
wasm-check:
    cargo check --workspace --locked --target wasm32-unknown-unknown \
      --exclude flui-assets --exclude flui-build --exclude flui-cli \
      --exclude flui-web-server --exclude hot-reload-counter-host \
      --exclude hot-reload-counter-logic --exclude hot-reload-counter-types
    # The development feature has no web runner integration, but enabling it
    # must remain a well-formed build rather than exposing native-only imports.
    cargo check -p flui --locked --target wasm32-unknown-unknown \
      --no-default-features --features hot-reload

# `cargo check` does not link, and on wasm32 even a link does not fail on an
# undefined symbol (rust-lld turns it into an import) — hence the committed
# import allowlist. Requires wasm-tools (cargo binstall wasm-tools).
[group("build")]
[doc("Really link the two wasm cdylibs and check their import surface (mirrors the CI wasm-check link steps)")]
wasm-link-check:
    cargo build --locked --target wasm32-unknown-unknown -p flui-web-demo -p flui-painting-demo
    bash scripts/check-wasm-imports.sh \
      target/wasm32-unknown-unknown/debug/flui_web_demo.wasm \
      target/wasm32-unknown-unknown/debug/flui_painting_demo.wasm

# `cargo clippy` does not link, so the per-OS backends in flui-platform can be
# type-checked from any host. Without this they are only ever compiled by
# whoever happens to develop on that OS — which is how the Windows backend
# came to have a hard `missing_docs` error, both desktop backends accumulated
# missing Debug impls, and Android accumulated ~24 more of its own (found the
# day it joined this matrix) that no gate could see.
#
# The triples are the ones actually shipped: MSVC (what `gpu-test` runs on
# windows-latest), not the GNU ABI, aarch64 for macOS, and aarch64 for
# Android. `--all-targets` so per-OS test targets are compiled too — omitting
# it is what made live code look dead on wasm32.
#
# LINT, NO LINK: clippy (not plain check) because cfg(windows)/cfg(macos)/
# cfg(target_os = "android") code is invisible to every other lint gate —
# check-only let ~80 deny-level violations accumulate unseen on Windows/macOS
# and a further ~24 on Android. It still does not link and runs no tests —
# Android's own build needs the NDK's cross-linker, which `cargo clippy`
# never reaches. The `test` job's dedicated flui-platform step only runs the
# Linux-buildable backends (headless + winit-on-X11); the Windows, macOS, and
# Android backends this lints are never linked or executed by anything else.
# Green here means "compiles clean under the workspace lints", nothing more.
# Requires: rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin aarch64-linux-android
[group("build")]
[doc("Clippy flui-platform's Windows, macOS, and Android backends from this host (mirrors the CI cross-typecheck job)")]
cross-typecheck:
    # `--features a11y` on every line: the UIA/NSAccessibility bridges are
    # feature-gated and this job is the ONLY gate that compiles them at all
    # (the a11y-off configuration is a strict subset — no cfg(not(a11y))
    # code exists — so checking with the feature supersedes checking without).
    cargo clippy -p flui-platform --locked --all-targets --features a11y --target x86_64-pc-windows-msvc -- -D warnings
    cargo clippy -p flui-platform --locked --all-targets --features a11y --target aarch64-apple-darwin -- -D warnings
    cargo clippy -p flui-platform --locked --all-targets --features a11y --target aarch64-linux-android -- -D warnings

# =============================================================================
# Testing
# =============================================================================

[group("test")]
[doc("Run all tests across the workspace")]
test *args:
    cargo test --workspace {{args}}

[group("test")]
[doc("Live E2E smoke: a REAL windowed demo driven by REAL X11 input (XTEST) — launch, mid-drag tracking, scroll, clean WM_DELETE close. Needs xvfb-run (apt install xvfb). Covers the band synthetic tests can't: platform translation, the wake chain, teardown")]
live-smoke:
    cargo build --package flui --features material --example sliver_demo
    cargo build --package flui-live-smoke
    xvfb-run -a -s "-screen 0 1200x800x24" target/debug/flui-live-smoke target/debug/examples/sliver_demo

[group("test")]
[doc("Wayland live-smoke: close-path teardown ordering under a headless weston compositor — the demo self-closes (FLUI_SELF_CLOSE_AFTER_MS drives the real CloseRequested arm) and must exit 0, five cycles. Catches the post-quit wl_proxy use-after-free class (issue #713) the X11 smoke can never see. SKIPs with a message when weston is absent (apt install weston)")]
live-smoke-wayland:
    cargo build --package flui --features material --example sliver_demo
    cargo build --package flui-live-smoke
    target/debug/flui-live-smoke target/debug/examples/sliver_demo wayland

[group("test")]
[doc("Run the workspace test scope used by CI (the flui-platform step needs xvfb-run on Linux — apt install xvfb; skipped with a message on other hosts)")]
test-ci:
    cargo nextest run --workspace --exclude flui-platform --locked --no-fail-fast
    # The facade defaults to Material only, so the run above skips
    # `tests/cupertino_demo.rs` (required-features) and the localizations
    # assertions in `tests/facade_smoke.rs`. Same precedent as CI's
    # flui-assets/flui-widgets feature-gated run.
    cargo nextest run -p flui --locked --features cupertino,localizations --no-fail-fast
    # Mirrors CI's dedicated flui-platform step, guarded by host OS:
    # `--all-features` is required just to compile the winit backend
    # (crates/flui-platform/AGENTS.md — invisible under `default =
    # ["desktop"]`); `FLUI_HEADLESS=1` routes `current_platform()` to the
    # `HeadlessPlatform` mock so most of the suite needs no display server;
    # a handful of winit-internals unit tests construct `WinitPlatform::new()`
    # directly and need a real (if virtual) X11 connection for clipboard
    # init, which `xvfb-run` supplies — a Linux-only tool, hence the guard.
    # On Windows this is not a missing-tool gap: STATUS_HEAP_CORRUPTION
    # (H9, docs/ROADMAP-TRACKER.md) is an unresolved crash in this crate's
    # Windows backend, so the tests must not run there at all. 175/175
    # pass on Linux, 5x-verified stable — see AGENTS.md Testing Quirks for
    # what stays excluded and why.
    {{ if os() == "linux" { "FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast" } else if os() == "windows" { "echo 'Skipping flui-platform tests: STATUS_HEAP_CORRUPTION (H9, docs/ROADMAP-TRACKER.md) is an unresolved Windows crash in this crate -- do not run its tests on a Windows host until that investigation lands a fix.'" } else { "echo 'Skipping flui-platform tests on this host: the CI-mirroring invocation needs xvfb-run (Linux-only) for the winit backend X11-dependent tests; see crates/flui-platform/AGENTS.md.'" } }}

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

# Excludes flui-platform to match the CI `test` job — that crate's suite is red
# independently of the profile (STATUS_HEAP_CORRUPTION investigation, see
# AGENTS.md), so including it would make this recipe permanently red and
# useless as a gate.
[group("test")]
[doc("Run tests against the release profile (excludes flui-platform, as CI does)")]
test-release:
    cargo test --workspace --exclude flui-platform --release

[group("test")]
[doc("Run rustdoc examples as tests (CI gate; nextest does not execute doctests)")]
test-doc:
    # flui-platform is not excluded: its doctests need neither
    # `--all-features` nor a display server (verified locally with and
    # without DISPLAY set), so there is no green-by-construction reason to
    # carve it out — see CI's `doc-test` job comment.
    cargo test --workspace --locked --doc

[group("test")]
[doc("Structural snapshots of the demo trees' painted layer trees (no GPU; CI runs these in the facade non-default-catalogs step)")]
demo-snapshots:
    # Both catalogs: the suite snapshots the Material and the Cupertino demo,
    # and Material is the facade default.
    cargo nextest run -p flui --locked --features cupertino --test demo_layer_snapshots

[group("test")]
[doc("Review and accept changed demo snapshots interactively (needs cargo-insta: cargo install cargo-insta)")]
demo-snapshots-review:
    # Never blanket-accept: a snapshot diff is the regression report, so read
    # it before blessing it. `cargo insta review` shows one diff at a time.
    cargo insta review

[group("test")]
[doc("Run the flui-assets/Image feature-gated tests CI also runs (default = [] hides them otherwise)")]
test-assets:
    cargo nextest run -p flui-assets --features full
    cargo nextest run -p flui-widgets --features images --test image
    cargo nextest run -p flui-widgets --features asset-images --lib
    cargo nextest run -p flui-widgets --features asset-images --test image_async
    cargo nextest run -p flui-widgets --features network-images --test image_network

[group("quality")]
[doc("Dependency audit: advisories, bans, licenses, sources (requires cargo-deny)")]
deny:
    cargo deny check

# SCOPE: widened from `pipeline::owner::subtree_arena` to `pipeline::owner`
# to pick up `cell.rs`'s PipelineCell checkout tests and two new
# real-NodePtr walks alongside the original subtree_arena suite. The
# `subtree_arena` unit tests still include the two pre-existing real-NodePtr
# walks that drive `layout_dirty_root` through every reborrow phase of
# `layout_subtree_borrowed_impl` — one straight parent→leaf pass, one cyclic
# `children()` edge that exercises the in-flight gate on the baseline
# callback (removing that gate makes miri fail this suite) — untouched by
# the widening. New alongside them: an owner-local traversal (a full
# run_frame over a real 3-node tree, driven through PipelineCell::with_mut)
# and a reentrant-layout walk (a Sliver child that issues a mid-layout
# child-build request against the checked-out owner, then mark_needs_layout
# right after). Measured at 55 tests / ~21s wall, in budget. Still narrow:
# only `pipeline::owner`, only box + leaf-sliver
# layout — deeper sliver walks and intrinsics queries are not interpreted
# here.
[group("test")]
[doc("Run miri on flui-rendering's pipeline::owner tests, incl. real layout walks + PipelineCell checkouts (requires nightly + miri)")]
miri:
    cargo +nightly miri test -p flui-rendering --lib pipeline::owner

[group("test")]
[doc("Generate an HTML coverage report (requires cargo-llvm-cov)")]
coverage:
    cargo llvm-cov --workspace --html
    @echo "Coverage report: target/llvm-cov/html/index.html"

# =============================================================================
# Quality gates
# =============================================================================

[group("quality")]
[doc("Run clippy exactly as CI does: workspace, then flui-engine's GPU-gated code")]
clippy:
    # Both invocations, both `--locked`, because that is what the CI job runs.
    # The second one is not optional: `enable-wgpu-tests` gates a body of code
    # -- the readback suite and the deterministic-replay tests -- that the
    # workspace pass never compiles, so a break there is invisible until CI.
    # A marker sweep missed an entire file for exactly this reason.
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo clippy -p flui-engine --all-targets --locked --features enable-wgpu-tests -- -D warnings

[group("quality")]
[doc("Run clippy and apply auto-fixes (uncommitted changes only)")]
clippy-fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty -- -D warnings

[group("quality")]
[doc("Per-feature clippy via cargo-hack (mirrors the CI feature-matrix job; requires cargo-hack)")]
feature-matrix: facade-combos
    cargo hack clippy --workspace --locked --each-feature --optional-deps --keep-going -- -D warnings
    cargo hack clippy --workspace --locked --each-feature --optional-deps --keep-going --tests --benches --examples -- -D warnings

[group("quality")]
[doc("Compile every supported facade feature combination in isolation")]
facade-combos:
    #!/usr/bin/env bash
    # Each combination is its own cargo invocation on the `flui` package alone.
    # A `--workspace` build proves nothing here: workspace feature unification
    # would enable `material` from a sibling and turn a broken combination
    # green. `--all-targets` is deliberate — a missing `required-features` on an
    # example or test is exactly the kind of wiring these builds exist to catch.
    set -euo pipefail
    for combo in "--no-default-features" \
                 "--no-default-features --features material" \
                 "--no-default-features --features cupertino" \
                 "--no-default-features --features material,cupertino" \
                 "--no-default-features --features localizations" \
                 "--no-default-features --features material,localizations" \
                 "--no-default-features --features cupertino,localizations" \
                 "--no-default-features --features material,cupertino,localizations" \
                 "--no-default-features --features hot-reload" \
                 "--no-default-features --features serde" \
                 "--all-features" \
                 ""; do
        echo "==> cargo clippy -p flui --locked --all-targets ${combo:-(default features)}"
        # shellcheck disable=SC2086
        cargo clippy -p flui --locked --all-targets $combo -- -D warnings
    done
    # Hot reload must be absent from an ordinary production graph, not merely
    # unused by it.
    echo "==> cargo tree -p flui-app: flui-hot-reload must be absent"
    if cargo tree -p flui-app --locked -e normal | grep -q flui-hot-reload; then
        echo "flui-hot-reload is in flui-app's default normal dependency graph" >&2
        exit 1
    fi
    if ! cargo tree -p flui-app --locked -e normal --features hot-reload | grep -q flui-hot-reload; then
        echo "the hot-reload feature did not bring in flui-hot-reload" >&2
        exit 1
    fi
    # The first-party host is the executable contract for `flui run`. A direct
    # dependency on flui-hot-reload does not activate flui-app's feature.
    if ! cargo tree -p hot-reload-counter-host --locked -e features -i flui-app \
        | grep -q 'flui-app feature "hot-reload"'; then
        echo "hot-reload-counter-host does not enable flui-app/hot-reload" >&2
        exit 1
    fi

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
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --document-private-items

[group("quality")]
[doc("Check crate inventories + the docs/workspace-layers.toml layer policy against Cargo metadata")]
inventory-check:
    bash scripts/check-workspace-inventory.sh

[group("quality")]
[doc("Validate the Runtime contract registry (docs/runtime-contract.toml) against the source tree")]
runtime-conformance-check:
    bash scripts/check-runtime-conformance.sh

[group("quality")]
[doc("Gate the expect(\"BUG: …\") panic-policy convention (docs/PANIC-POLICY.md) via its per-file ratchet")]
panic-policy-check:
    bash scripts/check-panic-policy.sh

# =============================================================================
# Port methodology
# =============================================================================

[group("port")]
[doc("Run refusal-trigger grep regressions (22 triggers + named guards from docs/PORT.md)")]
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

# Criterion writes baselines under target/criterion; this quiet dev box is
# where real A/B comparisons belong (CI runners are noisy — the weekly job
# only uploads trend artifacts, it never compares or gates). The script
# enumerates the real [[bench]] targets because plain `cargo bench` also
# selects libtest-harness targets, which reject criterion CLI flags.
[group("bench")]
[doc("Run every criterion bench and save the numbers under a named baseline (e.g. just bench-save before-fix)")]
bench-save name:
    bash scripts/bench-collect.sh {{name}}

[group("bench")]
[doc("Compare two saved baselines (requires critcmp: cargo binstall critcmp)")]
bench-compare old new:
    critcmp {{old}} {{new}}

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
    @echo "  cargo install --locked cargo-hack       # for just feature-matrix (CI per-feature gate)"
    @echo "  cargo install --locked zizmor           # workflow security audit (CI checks gate)"

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

# `just ci` stays the FAST local gate on purpose. The heavy CI-only jobs have
# their own recipes — run them deliberately before pushing risky changes:
#   just feature-matrix   (per-feature clippy, minutes)
#   just wasm-check       (wasm32 target check)
#   just cross-typecheck  (windows + macos + android backends, type-check only)
#   just deny             (advisories / bans / licenses / sources)
#   just miri             (nightly UB check, narrow scope — see its comment)
# Everything in `ci` except the test suites: ~2 minutes on a warm tree, and
# what the pre-push hook runs. Every gate this repository lost time to
# recently was caught by something in here, not by a test.
[group("ci")]
[doc("The non-test half of `ci` — what the pre-push hook runs")]
gate: fmt-check inventory-check runtime-conformance-check panic-policy-check port-check clippy doc-strict

[group("ci")]
[doc("Run local CI gates (gate + test + doctests)")]
ci: gate test-ci test-doc

# =============================================================================
# Maintenance
# =============================================================================

[group("maintenance")]
[doc("Point git at the repo's checked-in hooks (runs `just gate` before every push)")]
install-hooks:
    git config core.hooksPath scripts/githooks
    @echo "core.hooksPath -> scripts/githooks (git push --no-verify still bypasses it)"

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
