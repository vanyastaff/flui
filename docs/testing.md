[← Crates Map](crates.md) · [Back to README](../README.md) · [Contributing →](contributing.md)

# Testing

This page documents the test, lint, format, and benchmark commands enforced for FLUI. All gates listed here must pass before a change is merged.

## Quality Gates

The following commands must succeed on every change before review:

```bash
cargo fmt --all -- --check          # formatter gate (rustfmt.toml is authoritative)
cargo clippy --workspace -- -D warnings   # lint gate — zero warnings
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
cargo clippy --workspace -- -D warnings
cargo clippy -p flui-engine -- -D warnings
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

## CI Expectations

The same three quality gates run in CI on every PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

A change cannot be merged if any of these fail. If you encounter a flaky test, file a fix issue rather than retrying CI.

## See Also

- [Getting Started](getting-started.md) — toolchain setup and first build
- [Contributing](contributing.md) — workflow, commits, speckit
- [`.specify/memory/constitution.md`](../.specify/memory/constitution.md) — constitutional performance and testing requirements
