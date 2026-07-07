[← Crates Map](crates.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md) · [Contributing →](contributing.md)

# Testing

This page documents the test, lint, format, and benchmark commands enforced for FLUI. All gates listed here must pass before a change is merged.

## Quality Gates

The local pre-review gate is:

```bash
just ci
```

Expanded, that currently runs:

```bash
cargo fmt --all -- --check          # formatter gate (rustfmt.toml is authoritative)
bash scripts/check-workspace-inventory.sh # crate inventory drift guard
bash scripts/port-check.sh           # architecture refusal triggers
cargo clippy --workspace --all-targets -- -D warnings # lint gate — zero warnings
cargo test --workspace               # full test suite
```

## Build

```bash
cargo build --workspace              # full workspace build
cargo build --release --workspace    # optimized build (LTO enabled in release profile)
cargo check -p <crate>               # incremental type check for a single crate
cargo clean                          # wipe target/ before a fresh build
```

The `[default-members]` section of `Cargo.toml` excludes Android-only crates because `ndk-sys` does not compile on the host. Use `cargo ndk` for Android targets (see [Getting Started](getting-started.md)).

## Test Commands

### Workspace-wide

```bash
cargo test --workspace                            # all tests, all crates
cargo test --workspace --no-fail-fast             # keep going after failures
cargo test --workspace --release                  # run tests against the release profile
```

### Per crate

```bash
cargo test -p flui-types
cargo test -p flui-foundation
cargo test -p flui-tree
cargo test -p flui-platform
```

### A single test or filter

```bash
cargo test -p flui-tree element_id_offset                 # filter by name
cargo test -p flui-tree element_id_offset -- --nocapture  # surface stdout/println from tests
cargo test -p flui-tree -- --test-threads=1               # serialize tests (debugging)
```

### With logging

All FLUI code logs through `tracing`. To see `debug!` traces during a test:

```bash
RUST_LOG=debug cargo test -p flui-platform
RUST_LOG=flui_engine=trace cargo test -p flui-engine
```

## Coverage Targets

The constitution sets minimum coverage thresholds per crate category:

| Category | Minimum | Examples |
|----------|---------|----------|
| Core | 80 % | `flui-types`, `flui-foundation`, `flui-tree`, `flui-rendering`, `flui-view` |
| Platform | 70 % | `flui-platform` |
| Widget | 85 % | (future widget crates) |

Generate a coverage report with [`cargo-tarpaulin`](https://crates.io/crates/cargo-tarpaulin) or [`cargo-llvm-cov`](https://crates.io/crates/cargo-llvm-cov):

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --html
```

## Benchmarks

`criterion` is used for regression detection. Per-crate benchmark commands:

```bash
cargo bench -p flui-foundation
cargo bench -p flui-rendering
cargo bench -p flui-engine
```

Benchmark results are written under `target/criterion/` as HTML reports.

Performance targets defined by the constitution:

- Widget rebuild: < 1 ms for 1000 widgets.
- Layout pass: single-pass O(n) where possible.
- Frame target: 60 fps on desktop (16 ms frame budget).
- Hot-path allocations: zero allocations in layout and paint after the initial build.

## Linting

`cargo clippy` is the canonical lint command. The constitution requires `clippy::all` and `clippy::pedantic` at warn level workspace-wide.

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p flui-engine --all-targets -- -D warnings
cargo clippy --workspace --fix --allow-dirty       # auto-fix where Clippy can
```

## Formatting

`rustfmt.toml` is authoritative. Edition 2024, `max_width = 100`, `fn_params_layout = "Tall"`, `use_try_shorthand = true`, `use_field_init_shorthand = true`, `force_explicit_abi = true`.

```bash
cargo fmt --all                       # format the entire workspace
cargo fmt --all -- --check            # CI gate: fail if anything is unformatted
cargo fmt -p flui-engine              # format a single crate
```

## Documentation Build

```bash
cargo doc --workspace --no-deps                       # build rustdoc for FLUI crates only
cargo doc --workspace --no-deps --open                # open in browser
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps  # treat doc warnings as errors
```

The constitution requires `///` doc comments on every public item and `//!` overview at every crate root.

## Test Conventions

- **Unit tests** live in the same file under `#[cfg(test)] mod tests { ... }`.
- **Integration tests** live in `tests/` per crate. Cross-crate pipelines are tested in `flui-engine`.
- **Property-based tests** use [`proptest`](https://docs.rs/proptest) for layout algorithms and geometric operations.
- **Visual regression tests** (planned) will use snapshot-based comparison against the headless backend.
- **No mocking frameworks.** Use trait-based test doubles. The `HeadlessPlatform` backend is the canonical test surface for platform-dependent code.

## Test Harnesses (`testing` feature)

The rendering stack ships opt-in test harnesses (off by default so they never
land in normal/release builds). Each crate enables `testing` for its own
tests/benches/examples via a self dev-dependency; downstream crates opt in with
`features = ["testing"]`.

**Per-crate guides (API reference + examples):**

| Crate | Doc | Entry point |
|-------|-----|-------------|
| `flui-rendering` | [crates/flui-rendering/docs/TESTING.md](../crates/flui-rendering/docs/TESTING.md) | `RenderTester`, `Probe`, `box_node` / `sliver_node`, multi-frame `FrameRun` |
| `flui-layer` | [crates/flui-layer/docs/TESTING.md](../crates/flui-layer/docs/TESTING.md) | `LayerTester`, `layer`, `inspect::structure` |
| `flui-painting` | [crates/flui-painting/docs/TESTING.md](../crates/flui-painting/docs/TESTING.md) | `record`, `command_count`, `bounds`, `diagnostics` |
| `flui-foundation` | [crates/flui-foundation/docs/TESTING.md](../crates/flui-foundation/docs/TESTING.md) | `DiagnosticsNode` / `DiagnosticsBuilder` for structured assertions (no `testing` module) |

| Crate | What it gives you |
|-------|-------------------|
| `flui-painting` | Builds a `DisplayList` without `Canvas::new()` / `finish()` boilerplate. |
| `flui-layer` | Declarative `LayerTree` builder and layer walkers reused by `flui-rendering`. |
| `flui-rendering` | Real `PipelineOwner` trees (Box + Sliver), layout/frame depths, animation helpers. |
| `flui-foundation` | Diagnostics substrate: `find_descendant`, `get_property`, typed property builders. |

Diagnostics dumps are backed by `flui_foundation::Diagnosticable`: every node
self-describes its own **user-config** properties (a `RenderFlex`'s
`main_axis_alignment`, a `RenderPadding`'s `padding`), while `PipelineOwner`
adds committed **runtime** fields (`offset`, `size`, sliver `geometry`) when
building the tree. Property names use **snake_case** (Rust idiom, not Dart
camelCase). Prefer typed builder helpers (`add_enum`, `add_default_double`,
`add_flag`, `add_size`) over raw `format!("{:?}")` strings — defaults are
hidden automatically and kinds format cleanly in dumps.

Structured assertions should use `Probe::property` / `property_f64` /
`descendant_property` (or `DiagnosticsNode::get_property` /
`find_descendant`) instead of substring-matching `Probe::dump()`. Use
`to_string_deep_at_level(DiagnosticLevel::Info)` when fine-grained debug
properties should be omitted.

A `Probe::dump()` is what a failing assertion should print to show *why*.

```bash
cargo run -p flui-rendering --example render_inspector --features testing
cargo test -p flui-rendering --test render_object_harness
```

### Render-object harness catalog

`crates/flui-rendering/tests/render_object_harness.rs` is the CI-facing
catalog: every concrete `RenderBox` / `RenderSliver` type is mounted
through `RenderTester`, laid out (or painted when hit-test / layer
structure matters), and asserted via `Probe` + structured diagnostics
queries. The file header lists a per-type coverage map; `RENDER_OBJECT_TYPES`
is the manifest of all 37 exported render types; and
`catalog_covers_every_render_object_name` fails CI if any type is missing
from the harness file. Add a harness test when landing a new render object
so layout, hit-test, and config/runtime diagnostics stay pinned without
visual inspection.

Parent metadata that widgets normally write before layout (stack
positioning, flex factors, future animation parent slots) can be expressed
in harness trees via [`ParentDataSeed`](../../crates/flui-rendering/src/testing/parent_data.rs)
on [`TreeNode::with_parent_data_seed`](../../crates/flui-rendering/src/testing/tree.rs).
The pipeline clones each seed into the per-walk child slots before
`perform_layout` runs.

### Multi-frame and animation testing

After `.run_frame()`, [`FrameRun`](../../crates/flui-rendering/src/testing/harness.rs)
supports deterministic multi-frame scenarios (no wall clock):

| Method | Use when |
|--------|----------|
| `update` + `pump` | Layout changed (padding, size, sliver extent) |
| `update_paint` + `pump` | Paint-only change (color, opacity) |
| `advance_layout` / `advance_paint` | Shorthand: mutate + one frame |
| `simulate(ticks, \|t, run\| …)` | Tick loop: mutate in closure, auto-pump each step |
| `pump_frames(n)` | Skip `n` frames (idle frames produce no layer tree) |
| `pump_idle_frames(n)` | Strict: panic if any skipped frame paints or stays dirty |

Pair with `AnimationController::tick_at(t)` inside `simulate` for
production-faithful animation tests. Assert per frame via `Probe` (`offset`,
`box_geometry`, `picture_bounds`, `property`) and layer helpers
(`opacity_alpha`, `has_picture_layer`). See
`crates/flui-rendering/tests/harness_animation.rs` and
`crates/flui-rendering/tests/animation_pipeline.rs`.

## CI Expectations

CI runs the same local gates plus repository-wide source checks:

```bash
cargo fmt --all -- --check
taplo fmt --check
typos
bash scripts/check-workspace-inventory.sh
bash scripts/port-check.sh -v
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --exclude flui-platform --no-fail-fast --test-threads 1
cargo test --workspace --exclude flui-platform --doc
cargo check --workspace --all-targets           # repeated on Rust 1.96 (MSRV job)
cargo miri test -p flui-rendering --lib pipeline::owner::subtree_arena  # advisory
```

The `gpu-test` job additionally runs the full `enable-wgpu-tests` readback
suite on a windows-latest runner (WARP software rasterizer) and is
merge-blocking.

A change cannot be merged if any of these fail. If you encounter a flaky test, file a fix issue rather than retrying CI.

## See Also

- [Getting Started](getting-started.md) — toolchain setup and first build
- [Contributing](contributing.md) — workflow, commits, speckit
- [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — constitutional performance and testing requirements
