[← Crates Map](crates.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md) · [Contributing →](../CONTRIBUTING.md)

# Testing

This page documents the test, lint, format, and benchmark commands enforced for FLUI. All gates listed here must pass before a change is merged.

## Application tests through the facade

An application can use the framework's test support without depending on its
implementation crates. Add `features = ["testing"]` to the `flui` entry in
`[dev-dependencies]`, using the same source/version as the normal dependency.
Use `flui::testing::widgets::{lay_out, loose}` for widget layout and input,
`flui::testing::HeadlessBinding` for a virtual-clock frame driver, and
`flui::testing::rendering::{RenderTester, BoxQueryRun}` for custom render objects.
Accessibility and gesture replay live in `flui::testing::{a11y, replay}`.

The `testing` feature is off by default. Independent consumer tests check that
ordinary dependency graphs do not include the headless test driver. The layer
map below describes implementation ownership for framework contributors.

## Map of the testing layer

FLUI's test support is a stack, not one harness. Each tier drives the machine at
one depth; pick the **shallowest tier that can fail for the reason you care
about** — a layout bug found by a `RenderTester` test names the render object,
the same bug found by a whole-demo snapshot names a demo.

| Tier | Drives | Entry point | Enabled by |
|------|--------|-------------|------------|
| Diagnostics | Structured self-description of any node | `flui_foundation::{DiagnosticsNode, DiagnosticsBuilder}` | always |
| Painting | A `DisplayList`, no canvas boilerplate | `flui_painting::testing::{record, command_count, bounds}` | `flui-painting/testing` |
| Layer | Structural walkers over a `LayerTree` (built with `SceneBuilder` or `push_child`) | `flui_layer::testing::inspect` | `flui-layer/testing` |
| Render object | A real `PipelineOwner` — layout, paint, hit-test, intrinsics | `flui_rendering::testing::{RenderTester, Probe}` | `flui-rendering/testing` |
| **Frame** | A **whole headless frame** on a virtual clock: build → layout → paint → composite, gestures, animation, async tasks | `flui_testing::HeadlessBinding` | dev-dependency |
| **Widget** | A mounted widget tree with geometry probes and synthetic input | `flui_widgets::testing::{lay_out, LaidOut}` | `flui-widgets/testing` |
| Accessibility | The assembled semantics tree, queried by role | `flui_testing::a11y::{A11yTree, A11yQuery}` | dev-dependency |
| Gesture replay | A scripted gesture replayed with its timing | `flui_testing::replay::PointerScript` | dev-dependency |
| Log capture | The `tracing` events a frame emitted | `flui_testing::log_capture::capture` | dev-dependency |
| GPU readback | Real pixels off a real device (WARP in CI) | `flui-engine`'s readback suite | `flui-engine/testing` |
| Demo composition | A whole demo tree's committed `LayerTree`, as structured text | `tests/demo_layer_snapshots.rs` | `flui/material` + `flui/cupertino` |
| Live E2E | A real window, real X11/Wayland input, real exit code | `tools/live-smoke` | `just live-smoke` |

Two structural rules hold across the stack:

- **Test-only APIs live in `flui-testing`**, not behind a `testing` feature on a
  shipped crate. Where layering forbids the move — `flui_widgets::testing`
  mounts `FocusRoot`/`VsyncScope`/`GestureArenaScope`, which are widgets, and
  `flui-testing` may never depend on the widget catalog — the harness stays put
  but is *built on* `flui-testing`, so the shared machinery is not forked.
- **Mount through `HeadlessBinding::mount_root`.** It owns the eight-step
  bootstrap whose ordering is load-bearing, and its contract is that the
  bootstrap frame is the same frame `pump_frame` runs (same layout↔build
  fixpoint, same lazy-sliver service pass). Hand-rolled copies of that sequence
  have already drifted once, silently: of the eight that existed, one
  bootstrapped with a bare `PipelineOwner::run_frame` and captured every
  `SliverAppBar` delegate child unbuilt, and none ran the lazy-sliver service
  pass. All eight now go through `mount_root`.

### wasm32: compiled everywhere, executed in one place

Three of the commands above target wasm32 and only the last one runs anything. That distinction is
the whole content of the tier: `cargo check` and `cargo clippy` prove the code type-checks, and
`wasm-link-check` proves rust-lld resolves the cdylibs (with a committed import allowlist, because
on wasm32 an undefined symbol becomes an *import* rather than a link error). None of them execute a
single instruction.

It is not an academic gap. Swap `web_time::Instant` for `std::time::Instant` in
`crates/flui-foundation/src/clock.rs` — the substitution that module's own comment justifies as
*"the std one panics there"* — and every compile-only step stays green while the code panics
`time not implemented on this platform` the moment it runs. That was the state of the workspace
until issue #985.

`just wasm-test` (CI: the last steps of the `wasm-check` job) discovers the participating crates —
those declaring a wasm32 `wasm-bindgen-test` dev-dependency, resolved by
`scripts/wasm-test-crates.py` parsing the manifests rather than grepping them — and hosts their tests on node through
`wasm-bindgen-test-runner`. It runs **both** target kinds, because which one is possible depends on
visibility: an integration test (`tests/wasm32.rs`) sees only the public API, while a `pub(crate)`
seam is reachable *only* from a lib test. Two things about it are deliberate:

- **What belongs in that file** is behaviour that *differs* on wasm32, or a native-target
  substitution whose whole purpose is keeping wasm32 working. A test that would pass identically on
  native costs a wasm build and proves nothing extra.
- **The assertion count is checked, not trusted.** A runner that finds no tests exits 0 and prints
  `0 passed`, which is indistinguishable from a passing suite, so both the recipe and the CI step
  fail when the count is zero.

Three versions must agree or the runner refuses to start: the locked `wasm-bindgen`,
`wasm-bindgen-test` (pinned `=0.3.77` in flui-foundation — the unpinned `"0.3"` resolves to 0.3.78
and would bump the workspace lock as a side effect of adding a dev-dependency), and
`wasm-bindgen-cli`, whose version the recipe and the CI step both read out of `Cargo.lock`.

Discovery rather than a crate list is deliberate: a list is how the next crate to add wasm tests
gets silently never run — the same defect one level up. Its guards exist for that reason and each
covers a distinct way this goes inert while looking like success: a glob matching nothing never runs
the loop body; a runner finding no tests still exits 0 printing `0 passed`; and an aggregate-only
count lets *exactly the crate someone just added* be the silent one, so the non-zero check is **per
crate**. (Per crate, not per target — a crate legitimately has only one kind, and a zero there is
expected.)

**A `#[test]` function does not run on wasm32.** It compiles, and `wasm-bindgen-test-runner` reports
`no tests to run!`, because only `#[wasm_bindgen_test]` registers with the harness. That attribute is
therefore the opt-in: adding a wasm dev-dependency to a crate does not drag its whole native suite
onto wasm.

`ExecutionServices`' `Backend::Sequential` branch — compute inline at the spawn site, IO through
`spawn_local` — is now executed, from `crates/flui-app/src/app/execution.rs`'s
`wasm_sequential_backend_tests`. It had to be a **lib** test: `ExecutionServices` is `pub(crate)`,
so no integration test can reach it, and the only execution API one *can* reach there
(`DeterministicExecutors`) is a target-independent FIFO that would pass identically on native.

Still compile-only: `flui-platform`'s web backend, which is wasm32-only and has no executing
coverage at all. See #985.

## Quality Gates

The local pre-review gate is:

```bash
just ci
```

Expanded (see `ci:` in the `justfile` for the authoritative recipe list), that currently runs:

```bash
cargo fmt --all -- --check                                # fmt-check: formatter gate (rustfmt.toml is authoritative)
bash scripts/check-workspace-inventory.sh                  # inventory-check: crate inventory + layer-policy drift guard
bash scripts/check-runtime-conformance.sh                  # runtime-conformance-check: docs/runtime-contract.toml vs. source tree
bash scripts/check-toolchain-consistency.sh                # toolchain-consistency-check: MSRV agrees with rust-toolchain.toml everywhere it's declared
bash scripts/port-check.sh                                 # port-check: architecture refusal triggers
cargo clippy --workspace --all-targets -- -D warnings      # clippy: lint gate — zero warnings
cargo nextest run --workspace --exclude flui-platform --locked --no-fail-fast --lib --bins --tests --features flui/cupertino,flui/localizations --profile no-nested-cargo  # test-ci, stage 1 (scope: see "What `just test-ci` runs")
FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast  # test-ci: flui-platform, headless (Linux — apt install xvfb; Windows runs it without FLUI_HEADLESS/xvfb-run; skipped with a message on macOS, see justfile)
cargo nextest run <same scope> --profile nested-cargo       # test-ci, last stage: the nested-cargo tests (see below)
cargo test --workspace --locked --doc                      # test-doc: doc-tests (flui-platform included — its doctests need neither device above)
bash scripts/doc-strict.sh                                # doc-strict: cargo doc --workspace --no-deps --locked --document-private-items with every workspace `testing` feature on
```

**Adding a new gate** means two changes together, not one: a `just` recipe (so a
contributor can run it standalone) *and* a step in `.github/workflows/ci.yml`'s
`checks` job (so CI actually runs it — `just gate`/`just ci` are not
themselves invoked from CI; each script is its own explicit step there). A
recipe with no CI step only runs when someone remembers to run it by hand.

### What `just test-ci` runs

One scope for the whole local suite (`test_ci_scope` in the justfile):
`--workspace --exclude flui-platform --lib --bins --tests
--features flui/cupertino,flui/localizations`, run as the two stages below.
Two choices in it differ from CI on purpose:

- **One feature slice.** The facade's non-default catalogs (`cupertino`,
  `localizations`) join the workspace run through feature unification. The
  alternative, a second `cargo nextest run -p flui --features ...`, resolves
  features for `flui`'s own graph, without the dev-dependency features other
  members switch on (`testing` and friends), so every crate the two runs share
  was built twice under different hashes. No test is lost: the root crate has
  no `cfg(not(feature = ...))` code, so the default-feature facade's tests are
  a subset of these. **Not covered locally:** the facade in its default
  configuration (Material only, no Cupertino or localizations). CI's `test`
  job and `feature-matrix` build and test it; `just feature-matrix` does too.
- **Examples are not linked.** `cargo nextest run` with no target flags builds
  every example of every package it tests: about 60 binaries, each linking the
  whole render stack, on every run. `--lib --bins --tests` selects exactly the
  targets that have tests. Examples still **compile** in `just clippy`
  (`--all-targets`, part of `just gate`), so a type error in one still fails
  the local gate. What goes unchecked locally is only a *link* failure specific
  to an example; CI's `test` job (`cargo build --workspace --all-targets`) and
  `just build-all-targets` catch it.

Measured against the previous two-slice scope (2026-09-22, M1/8 GB,
`CARGO_BUILD_JOBS=6`, shared target, after an edit to `flui-types` so every
crate above it rebuilds): `--no-run` 437.1 s + 107.7 s = 544.8 s before,
326.1 s after; `debug/examples` 1.7 GB with 125 linked example binaries
before, empty after. Test names: 9754 + 58 runs = 9769 distinct tests before
(43 ran twice), 9769 after, none lost. This was measured on a target
directory shared between worktrees, before the per-checkout rule below; the
timings are indicative, and CI on the change is the authority for the test
set.

### Nested-cargo tests

The group is the nested-cargo tests that dominate the suite's wall-clock:
24 tests that run a `cargo` build of their own on a project they generate —
the trybuild `compile_fail` suites (`flui-engine`, `flui-rendering`,
`flui-view`'s `trybuild_ui`, `flui-types`' `unit_mixing_compile_fail`), the
`flui-cli` template tests (`cli_create::generated_*`), and every
`flui::facade_consumer` test. Locally, with their build caches cold, most take
one to five minutes; the other ~9,700 tests are quick. Tests that spawn a
`cargo` only for a trivial crate (`flui-cli`'s `cli_maintenance`, which runs
`cargo new` and tests an empty project in seconds) are deliberately left out
of the group. `.config/nextest.toml`
names them with one filter and two profiles that partition the suite
exactly: `no-nested-cargo` and `nested-cargo`.

- `just test-ci` (and so `just ci`) runs everything else first, then this
  group as its last stage. Nothing is dropped: the two stages together are
  the whole suite, and CI runs it as one invocation.
- `just test-ci-fast` is the quick local loop: the same scope without the
  group, ending with a line that names what it skipped.
- `just test-nested-cargo` runs only the group.

Measured on the workspace invocation (2026-09-22, M1/8 GB, `CARGO_BUILD_JOBS=6`,
wall-clock including the build):

| Nested-build caches | One invocation (before) | Two stages (`just test-ci`) |
|---|---|---|
| warm, nothing changed | 191.2 s | 101.6 + 13.7 = 115.3 s |
| after an edit to `flui-types` (registry deps warm) | 576.9 s | 406.5 + 87.2 = 493.7 s |
| cold (caches deleted) | 527.7 s | 101.6 + 563.1 = 664.7 s |

Only the fully cold case is slower: the quick tests no longer hide behind the
nested builds. That happens once per cache wipe: the caches live in the
target directory, not in the checkout (below). The group runs in parallel: serializing it was
slower cold (713.6 s vs 563.1 s) and warm (25.9 s vs 13.7 s).

Where their builds go: each nested build needs a target directory other than
the outer one, because under `cargo test` the outer Cargo holds its build lock
for the whole run. The template and facade-consumer builds use
`cli-template-check/` and `facade-consumer-check/` under the workspace target
directory as Cargo resolves it (`cargo metadata`'s `target_directory`), so a
`CARGO_TARGET_DIR` is honored. Before this, they wrote to `<checkout>/target`
whatever `CARGO_TARGET_DIR` said. trybuild keeps its own `tests/trybuild/` there, since
it builds with a different `--cfg` and would thrash a shared cache. Nothing
prunes these three directories: they grow with every FLUI version and feature
set built through them (1.5-3 GB each is normal). `just clean-nested` deletes
them, safe whenever no test run is using them; `just clean-stale` bounds the
main target directory the same way (oldest artifacts first, via cargo-sweep).

**One target directory per checkout; never share one between worktrees.**
Pointing several worktrees at one `CARGO_TARGET_DIR` looks like a cache and is
not sound. Cargo records a workspace crate's sources in its fingerprint
relative to the package, so the same unit (crate + features + profile) built
in two worktrees gets the same artifact name and the same fingerprint. When
worktree A rebuilds that unit later than B from different sources, B's next
build compares its own files' mtimes with A's newer artifact, finds it fresh,
and links A's code. On 2026-09-22 this compiled `flui-view` against a
`flui-foundation` without `RebuildReason::COUNT` although the checkout's own
source defines it, reproducibly, while `-p flui-view` alone (another feature
set, another unit) built fine. A green run can come from someone else's
source just as easily. trybuild adds a second failure: it writes each suite's
project to `<target>/tests/trybuild/<crate>/`, so two checkouts running the
same suite at once overwrite each other's project.

So:
- each worktree builds into its own `target/` (leave `CARGO_TARGET_DIR` unset,
  or point it inside the worktree);
- local compilation before a PR is optional; the proof is CI, which builds
  from scratch;
- a worktree's `target/` is deleted with the worktree once its branch merges.

## Build

```bash
cargo build --workspace              # full workspace build
cargo build --release --workspace    # optimized build (LTO enabled in release profile)
cargo check -p <crate>               # incremental type check for a single crate
cargo clean                          # wipe target/ before a fresh build
```

The `[default-members]` section of `Cargo.toml` excludes Android-only crates because `ndk-sys` does not compile on the host. Use `cargo ndk` for Android targets (see [Getting Started](getting-started.md)).

### Local machine mode (shared, memory-limited)

On a shared, memory-constrained dev machine — several agent worktrees against the same checkout,
one compiling worker at a time (see AGENTS.md's Commands table) — every worktree points at the
same `CARGO_TARGET_DIR`, and `CARGO_BUILD_JOBS` is sized to available RAM rather than core count.
A docs-only change never needs a `cargo` invocation at all: `just fmt-check text-check
inventory-check port-check` is the full local gate for it, which is what lets a docs worktree stay
green without contending for the shared build. One concrete consequence of the shared
`CARGO_TARGET_DIR`: the trybuild suites (`flui-engine::compile_fail`, `flui-rendering::compile_fail`,
`unit_mixing_compile_fail::ui`, `trybuild_ui::ui_tests` — see `.config/nextest.toml`) each drive a
real `rustc` invocation per fixture into scratch output under `target/`, so two of them compiling
concurrently from different worktrees against the same target dir can spuriously fail on artifact
contention rather than on the fixture's actual `compile_fail` assertion — keep trybuild runs
serialized with the rest of the machine's one-worker-at-a-time rule, not fanned out across parallel
agent sessions.

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

Generate a coverage report with [`cargo-llvm-cov`](https://crates.io/crates/cargo-llvm-cov)
— `just coverage` wraps it, and it is the only coverage tool this workspace uses:

```bash
cargo install cargo-llvm-cov
just coverage                        # or: cargo llvm-cov --workspace --html
```

These thresholds are a target, not a gate: no CI job enforces them today.

## Benchmarks

`criterion` is used for regression detection. Per-crate benchmark commands:

```bash
cargo bench -p flui-foundation
cargo bench -p flui-rendering
cargo bench -p flui-engine
```

Benchmark results are written under `target/criterion/` as HTML reports.

`just bench-signals` (`crates/flui-widgets/benches/signals_rebuilds.rs`, needs the
`signals` feature) runs ADR-0074's go/no-go: `setState` against realm-scoped signals on
the same widget tree, printing a table of elements rebuilt per `RebuildReason` and
layout roots per frame before the criterion timings. The counts come from two telemetry
accessors any test can use: `BuildOwner::last_frame_build_report()` (what the last
`build_scope` rebuilt, split by cause — read it after a pump, before the next one) and
`PipelineOwner::layout_roots_total()` (monotonic; a frame's figure is the difference
across it, because `run_layout` runs several times per frame).

Compiling benches (`bench-compile` in CI) proves they build; it does not
detect a regression — numbers have to be collected and compared. The
workflow for that:

- **Local A/B (the authoritative comparison).** Run on a quiet machine:
  `just bench-save before` on the baseline commit, apply the change, then
  `just bench-save after` and `just bench-compare before after` (needs
  `critcmp`, e.g. `cargo binstall critcmp`). Criterion also prints its own
  change estimate against the last run of the same bench.
- **Weekly trend (advisory).** The `bench` job in `weekly.yml` executes the
  full suite and uploads `target/criterion` as a 90-day artifact. Shared
  runners are noisy, so this is drift-over-weeks data — it never gates a
  merge and a single outlier means nothing. GPU benches self-exclude via
  their `required-features` gate.

Performance targets defined by the constitution:

- Widget rebuild: < 1 ms for 1000 widgets.
- Layout pass: single-pass O(n) where possible.
- Frame target: 60 fps on desktop (16 ms frame budget).
- Hot-path allocations: zero allocations in layout and paint after the initial build.

## Linting

`cargo clippy` is the canonical lint command. The constitution requires `clippy::all` and `clippy::pedantic` at warn level workspace-wide.

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
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
  A crate's root `tests/*.rs` files compile as modules of **one** integration-test
  binary (`tests/main.rs` with `#[path]` module declarations, `autotests = false`
  plus a `[[test]]` in `Cargo.toml`, named after the crate without its `flui-`
  prefix: `widgets_it`, `scheduler_it`, `app_it`, …; `flui_testing_it` is the one
  pre-existing exception), because every
  auto-discovered per-file target links the whole dependency stack again. A test
  that *writes* process-global state — a `#[global_allocator]`, the global
  `tracing` subscriber slot or callsite-interest cache, an environment variable —
  keeps its own `[[test]]`
  target, with the reason in a comment beside it; so do `harness = false`,
  trybuild/`compile_fail`, and feature-gated targets that CI runs by name.
  `flui-log` is the deliberate exception: each of its files owns one scenario
  that installs the global subscriber, so each stays a binary of its own.
  A crate whose suites share a `tests/common/` helper module (`flui-material`, `flui-cupertino`)
  declares it once, as `mod common;` in `tests/main.rs`, and each suite imports it
  with `use crate::common;`: a `mod common;` inside every `#[path]`-loaded suite
  would load the same file once per suite, which `clippy::duplicate_mod` rejects.
- **Property-based tests** use [`proptest`](https://docs.rs/proptest) for layout algorithms and geometric operations.
- **Demo composition tests** live in `tests/demo_layer_snapshots.rs`: each demo mounts headless and its committed `LayerTree` is compared, as structured text, against an `insta` snapshot. See [Demo composition snapshots](#demo-composition-snapshots) below for the run/review workflow and why they are structural rather than pixels.
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
| `flui-layer` | [crates/flui-layer/README.md](../crates/flui-layer/README.md) | `SceneBuilder`, `inspect::structure` / `clip_rects` / `first_picture_bounds` |
| `flui-painting` | [crates/flui-painting/docs/TESTING.md](../crates/flui-painting/docs/TESTING.md) | `record`, `command_count`, `bounds`, `diagnostics` |
| `flui-foundation` | [crates/flui-foundation/docs/TESTING.md](../crates/flui-foundation/docs/TESTING.md) | `DiagnosticsNode` / `DiagnosticsBuilder` for structured assertions (no `testing` module) |
| `flui-testing` | [crates/flui-testing/README.md](../crates/flui-testing/README.md) | `HeadlessBinding` (`pump_frame`, `mount_root`, `replay`), `a11y::A11yQuery` — a **dev-dependency**, not a `testing` feature |
| `flui-widgets` | [crates/flui-widgets/README.md](../crates/flui-widgets/README.md) | `testing::{lay_out, LaidOut, settle_lazy}` — the canonical widget harness, shared verbatim by `flui-material` / `flui-cupertino` |

| Crate | What it gives you |
|-------|-------------------|
| `flui-painting` | Builds a `DisplayList` without `Canvas::new()` / `finish()` boilerplate. |
| `flui-layer` | Declarative `LayerTree` builder and layer walkers reused by `flui-rendering`. |
| `flui-rendering` | Real `PipelineOwner` trees (Box + Sliver), layout/frame depths, animation helpers. |
| `flui-testing` | A whole headless frame on a virtual clock: gesture deadlines, animation ticks, async tasks, build/layout/paint/composite, semantics, scripted gesture replay. |
| `flui-widgets` | A mounted widget tree: geometry probes, synthetic pointer/scroll input with per-contact identity, root swap, lazy-sliver settling. |
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

`crates/flui-objects/tests/render_object_harness.rs` is the CI-facing
catalog: every concrete `RenderBox` / `RenderSliver` type is mounted
through `RenderTester`, laid out (or painted when hit-test / layer
structure matters), and asserted via `Probe` + structured diagnostics
queries. The file header lists a per-type coverage map; `RENDER_OBJECT_TYPES`
is the manifest of all 81 exported render types (count it yourself —
`RENDER_OBJECT_TYPES` in that file — if this drifts again); and
`catalog_covers_every_render_object_name` fails CI if any type is missing
from the harness file, and `render_object_types_match_exports` fails CI if
the catalog and this crate's `pub use` exports diverge. Add a harness test
when landing a new render object so layout, hit-test, and config/runtime
diagnostics stay pinned without visual inspection.

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

## Headless frames and widget trees

`flui_testing::HeadlessBinding` is the frame tier: a non-singleton, sleep-free
runtime whose `pump_frame(dt)` advances a virtual `ManualClock` and runs the
same frame the live `draw_frame` runs. Mount through `mount_root`; never
hand-roll the bootstrap (see the map above). `mount_root` wraps the caller in
`RootRenderView` — the same production root-bootstrap path as
`WidgetsBinding::attach_root_widget` — so `PipelineOwner.root_id` is installed
by `RootRenderElement`, not by a post-hoc parentless-node scan.

```rust,ignore
let mut binding = HeadlessBinding::new();
let mounted = binding.mount_root(&root, MountOwners::fresh(), MountOptions::tight(800.0, 600.0));
binding.pump_frame(Duration::from_millis(16));
```

`flui_widgets::testing::lay_out` is the widget tier over it, adding the
presentation scopes and geometry probes. It is one harness, shared verbatim by
`flui-widgets`, `flui-material`, and `flui-cupertino` — the per-crate
`tests/common/mod.rs` files are thin re-export shims, so mount ordering,
pointer-contact identity, and virtual-clock policy cannot drift apart between
crates again.

Two behaviours worth knowing before you write an assertion:

- **Lazy children build after paint**, not during layout as Flutter does, so a
  triggering change (initial mount, a root swap, a scroll) needs two ticks to
  settle. Use `settle_lazy`.
- **A contact's route is captured on its Down** and reused for that contact's
  remaining events, so a harness hit-test hook fires once per contact, not once
  per event. Give each contact its own `PointerId`.

### Scripted gestures

`flui_testing::replay` scripts a gesture as data with explicit virtual-time
offsets and replays it by advancing the clock — so the timing that decides a
deadline-driven recognizer's verdict is part of the script, not of how the test
process happened to be scheduled.

```rust,ignore
binding.replay(&PointerScript::long_press(at, Duration::from_millis(600)));
binding.replay(&PointerScript::fling(from, to));
```

Presets: `tap`, `double_tap`, `long_press`, `drag`, `fling`, `swipe`, `pinch`.
`GestureRecorder` captures a script off the same virtual clock, so a recording
round-trips to its own timing.

### Accessibility

`binding.enable_semantics()` then `binding.a11y_tree()` gives an `A11yTree` of
AccessKit nodes, translated by the same `flui_semantics::tree_to_update` a
platform adapter uses — so a test and a screen reader cannot disagree. Query by
role rather than by node index.

### Asserting on what was logged

Some contracts are only observable as a diagnostic — a misconfiguration
reported once rather than every frame, the text of a caught panic that
`RenderError` does not carry. Capture those with
`flui_testing::log_capture::capture`:

```rust,ignore
let (laid, log) = capture(|| harness::pump_widget(root, harness::screen()));
assert!(!log.is_empty(), "vacuous-pass guard: the frame must have logged something");
assert_eq!(log.count_containing("unbounded main axis declares"), 1, "{log}");
```

**Do not hand-roll this with `tracing::subscriber::with_default`.** `tracing`
computes a callsite's interest once, on whichever thread reaches it first, and
caches it process-globally, so a thread-local subscriber silently loses every
event from a callsite another test reached first. This suite carried two
hand-rolled copies of that technique, both documenting the caveat and neither
able to fix it: one failed 4 times in 25 runs of the `parity` binary while
passing 60/60 in isolation, and the other serialised its tests behind a mutex
that could not help, because the poisoner is every other test in the binary,
not the one it was serialised against.

`capture` fixes it at the cause, one level below the subscriber: it registers
two permissive sentinel dispatchers — never anyone's default, so they receive no
events — which makes `tracing`'s interest cache unable to resolve any callsite
to `never`, whichever thread reaches it first. With the cache disarmed it can
then install its own subscriber the ordinary composable way, thread-locally for
one closure. So it never takes the process-global default slot: a binary keeps
its own logging subscriber, events outside a capture still reach it, and
concurrent captures on different threads neither block nor see each other.
A crate whose capture helper is too specialised to replace keeps it, and calls
`log_capture::disarm_interest_cache` first — that is public for exactly this.
`flui-view`, `flui-interaction`, `flui-app` and `flui-devtools` do, through a
**dev-dependency cycle**: `flui-testing` depends on them normally, and cargo
permits the reverse edge for dev-dependencies precisely so a lower crate can
use the test support built on it.

Two crates deliberately do not, because they have nothing to poison — their
capture tests share no callsite with anything else in their binary, each
emitting at its own source line inside its own helper. `flui-log` additionally
has no in-workspace dependencies at all, which its layer entry states as a
contract; `flui-foundation` is emission-only and may not construct a subscriber,
which is also why the primitive lives in
`flui-testing` rather than at the bottom of the DAG where every crate could
reach it without an edge.

## Agent workflow

`tests/agent_workflow.rs` is the acceptance test for the "Agent workflow" row
of `docs/BETA.md`: evidence that an agent (or a human, working the same way)
can discover the public API, build a UI, inspect it two different ways,
drive an interaction through it, and assert the result — using only
`flui::…`, the same surface a consumer of the published crate has. It is
`flui`'s own `tests/`, not `flui-widgets`' or `flui-testing`'s, for exactly
that reason: those crates' own tests can see implementation details this one
must not use.

The tree under test is the CLI `counter` template's own shape (`Center` →
`Column` → prompt `Text` / count `Text` / `ElevatedButton`, a `StateCell`
bound in `init_state`) — see `crates/flui-cli/src/templates/counter.rs` for
the generator and `crates/flui-view/src/state_cell.rs` for the state
primitive. Five steps, each backed by a documented, facade-reachable API:

| Step | What it does | API |
|------|--------------|-----|
| 1. Mount | Bootstrap the tree headlessly | `flui::testing::HeadlessBinding::mount_root` |
| 2. Inspect structure | Dump the render tree, check a known node is there | `flui::testing::rendering::render_diagnostics` over `HeadlessBinding::pipeline_owner().with(...)` + `flui::foundation::DiagnosticsNode::to_string_deep` |
| 3. Inspect semantics | Find the button by its accessible label | `flui::testing::HeadlessBinding::{enable_semantics, a11y_tree}` + `flui::testing::a11y::A11yTree::find_by_label` |
| 4. Drive | Tap the label's own bounds | `flui::testing::HeadlessBinding::replay` with `flui::testing::replay::PointerScript::tap` |
| 5. Assert | Confirm the rendered count advanced | The diagnostics dump again, checked for the `RenderParagraph` `text` property |

Step 4 is deliberately **not** `flui_widgets::testing::lay_out`'s
`dispatch_pointer_down`/`find_text` convenience: those are widget-internal
shortcuts this package's own tests use freely (see
`tests/material_demo.rs`), but an outside agent driving the framework through
its documented surface has only `HeadlessBinding::replay` and a hit-test
target it found itself — semantics bounds, here — to tap with. Steps 2 and 5
reuse the same diagnostics dump for a structural check and a content check
respectively; that reuse is deliberate, not laziness — the semantics tree
carries a `Text`'s content as a label, but the diagnostics tree's `text`
property is the reading of what the `RenderParagraph` actually rendered,
which is what step 5 is asserting.

**A gap the first version of this test found, now closed:** the counter
template's `ElevatedButton` (`flui-material`'s `ButtonStyleButtonCore`
composition) attached no `SemanticsConfiguration` of its own, and neither
did a bare `Text`, so step 3 found nothing unless the test wrapped the
button in an explicit `flui::prelude::Semantics` — which meant the app `flui
create` scaffolds was not screen-reader accessible out of the box.
`RenderParagraph` now publishes its plain text as a semantics label with its
text direction (an empty paragraph publishes nothing — a recorded mapping
decision in `crates/flui-objects/ARCHITECTURE.md`), and `ButtonStyleButtonCore`
wraps every button it composes in `Semantics(container, button, enabled)`.
The test mounts the template's tree exactly and queries `"Increment"` from
the framework's own node.

The second test, `missing_label_query_reports_the_search_and_the_available_labels`,
is the acceptance criterion's "actionable command failures" half: it queries
a label that is not in the tree and asserts the resulting
`flui::testing::a11y::A11yQueryError::NotFound` names the search (`"Decrement"`)
and lists what *was* reachable (`"Increment"`) — the error `A11yTree::find_by_label`
already produces, needing no new helper.

Writing the test also found that the facade's `flui::testing::rendering`
module did not re-export `render_diagnostics` (`flui-rendering`'s render-tree
dump); it does now, and step 2 uses it.

## Demo composition snapshots

```bash
just demo-snapshots          # run
just demo-snapshots-review   # review and accept intended changes (cargo-insta)
```

Each demo tree mounts headless at 900x760 through `flui-testing`'s canonical
bootstrap, and the `LayerTree` its bootstrap frame commits is serialized to
structured text — the layer structure plus every draw command, with geometry,
colors, transforms, and text — and compared against a committed `insta`
snapshot in `tests/snapshots/`. A widget that moves or resizes, a shadow that
stops being emitted, a clip that disappears, a subtree that stops being built:
each changes those lines and fails the matching test, naming the layer and the
command.

No GPU, no device-specific baseline: CI's "facade non-default catalogs" step
(`cargo nextest run -p flui --features cupertino,localizations`) runs the suite
like any other test, and it takes about a tenth of a second.

### Why structural and not pixels

This suite replaced a pixel-golden suite that compared the same six demos
against committed PNGs. That suite ran on no CI job and could not be moved onto
one, and its own documentation blamed the GPU: "the goldens are specific to the
machine that generated them (GPU / driver differences move anti-aliased edges)".
Measured against a completely different rasterizer (llvmpipe vs. the reference
device), that was not where the binding was:

| Demo | Pixels past tolerance | Where |
|------|----------------------|-------|
| colored-box (flat fill) | 0 of 684 000 — bit-identical | — |
| gallery (vector shapes) | 0.20%, max channel delta 48 | anti-aliased edges |
| text, material, cupertino, vertical-slice | 0.52%–0.76%, against a 0.5% threshold, max delta 243–255 | **glyphs, and the widgets sized to them** |

Shape, fill, and shadow raster carried across devices. The whole difference was
text — the glyphs themselves, plus the button rectangles that hug them — and it
was a *font* difference, not an anti-aliasing one: the same string measured
24 px (9.4%) wider with a 2 px baseline shift, which a rasterizer cannot do.
The real baseline was the host's font installation, and the suite's pass/fail
line sat inside its own cross-machine noise.

What the change gives up is the raster of a *composed* scene. Its pieces are
covered elsewhere: blending, filters, gradients, and glyph raster per primitive
by flui-engine's readback/oracle suite on WARP (merge-blocking `gpu-test`), and
that a real window presents at all by `tools/live-smoke`. What is genuinely
lost — the anti-aliased pixels of a whole demo — was guarded by nothing before,
since the suite ran on no job.

### Determinism

Text measurement resolves against the host's fonts, and widgets sized to their
text inherit that: the same Cupertino button measured 61.18 px wide on a host
with fonts installed and 129.55 px on one without. `flui_testing::fonts::pin_font_faces`
builds the process-wide `FontSystem` from the faces this repository ships
(`flui_painting::fonts`), so the committed geometry is reproducible off any one
machine.

It *builds* the font system rather than editing it, and that distinction is
load-bearing: `FontSystem` freezes its fallback chain and monospace face list
at construction, so emptying its database afterwards and reloading known faces
leaves stale construction-time state — which is exactly how the 61.18 px
measurement arose. `flui_painting::text_layout::init_font_system_with_faces`
carries the details.

To *look* at what a tree renders without a window, capture it:

```bash
cargo run -p flui --example screenshot --features "material cupertino" -- material 900 760 out.png
```

## Live E2E smoke

```bash
just live-smoke           # X11 under Xvfb: real window, real XTEST input, real pixels
just live-smoke-wayland   # headless weston: the close-path teardown ordering
```

`tools/live-smoke` is the only executing coverage of the band **above**
synthetic event dispatch — platform translation, the event-loop wake chain,
window-close teardown — each of which has shipped broken while every synthetic
gesture test stayed green. It also verifies hidden-surface gating against a real
occlusion signal. Both variants run in CI.

## CI Expectations

CI runs the same local gates plus repository-wide source checks. Every job is
gated on the fast `checks` job and aggregates into the single required `ci`
check; all cargo commands run `--locked`; actions are SHA-pinned and the
workflow files themselves are linted:

```bash
cargo fmt --all -- --check
taplo fmt --check
typos
actionlint                                                    # workflow semantics
zizmor .                                                      # workflow security audit
bash scripts/check-workspace-inventory.sh
bash scripts/port-check.sh -v
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo hack clippy --workspace --locked --each-feature --optional-deps --keep-going -- -D warnings  # feature-matrix job, then a --tests --benches --examples pass
just facade-combos                                            # isolated per-combination facade builds (same job)
cargo check --workspace --locked --target wasm32-unknown-unknown --exclude ...                   # wasm-capable set — just wasm-check
cargo clippy --workspace --lib --bins --locked --target wasm32-unknown-unknown --exclude ... -- -D warnings  # the only lint pass over the wasm32-only web backend — just wasm-check
cargo test -p flui-foundation --locked --target wasm32-unknown-unknown --test wasm32              # the only step that EXECUTES wasm — just wasm-test
cargo check -p flui-platform --locked --all-targets --target x86_64-pc-windows-msvc            # cross-typecheck job — just cross-typecheck
cargo check -p flui-platform --locked --all-targets --target aarch64-apple-darwin              # (type-check only: no link, no tests)
cargo deny check                                              # advisories / bans / licenses / sources
cargo bench -p flui-rendering --no-run                        # bench-compile job
bash scripts/doc-strict.sh                                    # doc job: the same script as `just doc-strict`
cargo nextest run --workspace --exclude flui-platform --locked --no-fail-fast
FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast  # test job's dedicated flui-platform step
cargo nextest run -p flui-platform --locked [--all-features] --no-fail-fast                           # platform-windows job (windows-latest), both feature sets
cargo test --workspace --locked --doc
cargo check --workspace --all-targets --locked                # repeated on Rust 1.98 (MSRV job)
cargo +nightly miri test -p flui-rendering --lib pipeline::owner  # advisory (continue-on-error); NARROW — every
                                                              # unit test under that module, including PipelineCell
                                                              # checkout, an owner-local run_frame traversal, a
                                                              # reentrant-layout walk, and two real-NodePtr walks
                                                              # driving layout_dirty_root through every reborrow phase
                                                              # of layout_subtree_borrowed_impl. Deeper sliver walks
                                                              # and intrinsics queries are not interpreted.
```

### CI jobs and their local recipes

CI runs in two lanes, chosen by the `plan` job:

- **Fast lane**: an ordinary pull request. It runs `checks`, `deny`, and
  `fast-lane`. `fast-lane` runs clippy and nextest over the changed crates and
  every workspace crate that declares a dependency on them, including optional,
  dev and target-specific dependencies. It also checks the code a Linux build
  never compiles:
  - clippy on the other targets for `flui-platform`'s backends, the
    `flui-app`/`flui` Android runner and `flui-cli` on Windows, when they are
    in scope;
  - wasm32 clippy for the wasm-capable crates in scope;
  - a per-feature `cargo hack clippy` for the changed crates that have
    features, for any crate whose `Cargo.toml` changed, and for each
    dependent whose edge to a crate in scope only a non-default feature
    compiles (an optional dependency, or one a non-default feature names).
    Other dependents keep the default build. More than three such dependents
    send the PR to the heavy lane, whose `feature-matrix` covers them;
  - the `flui-app`/`flui` iOS runner's clippy, when either is in scope, in a
    separate macOS job (`fast-lane-ios`), because it needs xcrun;
  - rustdoc with `-D warnings` over the crates in scope, with their `testing`
    features (the `doc` job's flags). A moved item's broken intra-doc link is
    the typical casualty of a refactor;
  - the doctests of the library crates in scope (`cargo test --doc`, the
    `doc-test` job narrowed): nextest runs none.

  A workspace-wide input (clippy/nextest config, the lane's own scripts) or a
  file no crate owns widens the lane to the whole workspace. Nothing compiles
  for a documentation-only or tooling-only change. The scope comes from
  `scripts/affected-crates.sh`; `just check-changed` runs it with the same
  arguments before a PR, and also counts uncommitted work.
- **Heavy lane**: a push to main, the merge queue, the nightly schedule,
  `workflow_dispatch`, a pull request that changes an input of the heavy jobs,
  or a pull request labelled `full-ci`. The heavy-job inputs are `Cargo.lock`,
  the root `Cargo.toml`, `.cargo/`, the toolchain file (either spelling), a
  workflow, a WGSL shader (only a GPU job compiles one), and any script a
  heavy job runs (read from `ci.yml`, the justfile included). Adding the label
  re-runs the PR's own CI run through `full-ci.yml`, so the heavy result
  replaces the fast one in the same `ci` check; `plan` reads the label from
  the API. On a fork PR it fails, saying so (its token cannot re-run
  anything): re-run CI from the PR's Checks tab instead. Later pushes to a
  labelled PR take the heavy lane directly. Every job below runs. The path is
  proven on #1273 (2026-09-23): the label re-ran run 35815534194 as attempt
  2, `plan` logged `full-ci-label=true heavy=true`, and the PR's `ci` check
  was that attempt's job.

  A red heavy run on main or nightly opens (or comments on) the "CI is red on
  main" issue. The rule is fix forward within the hour, or revert.

**Only the heavy lane checks these**, so a pull request can merge green and
still turn main red:

- doc-tests and rustdoc of crates outside the change's scope (`doc-test`,
  `doc`);
- linking of examples and benches (`test`'s `build --all-targets`,
  `bench-compile`);
- the feature-gated suites (`test-features`);
- the per-feature matrix of dependents that reach the change under a
  default feature or not at all (`feature-matrix`);
- the facade in its default feature set;
- GPU readback (`gpu-test`), miri, msrv, `live-smoke`;
- macOS's `flui-cli` suite (`cli-macos`; its iOS runner clippy also runs in
  `fast-lane-ios` when `flui-app` or `flui` is in scope);
- linking and running the wasm32 tests (`wasm-check`).

Label a change that is likely to break one of these `full-ci`.

The `ci` aggregator recomputes which jobs this lane and change should skip
from `plan`'s outputs. It fails on any other skip, and on a job that ran where
it should have skipped.

Locally, `just check-changed` is the pre-PR check. `just ci` is the full local
gate and is optional: CI is the proof. `just ci-full` mirrors the heavy jobs
this host can run, and `just doctor full` names what it needs. One row per job
in `.github/workflows/ci.yml`:

| CI job | Local recipe | Difference, or why CI-only |
|---|---|---|
| `checks` | `just gate` (fmt, text-check, inventory, runtime-conformance, toolchain-consistency, panic-policy, port-check, wgsl-uniformity) + `just workflow-lint` | `workflow-lint` skips actionlint/zizmor with a message when they are not installed; CI always has them |
| `plan` | `scripts/affected-crates.sh` (`just check-changed` runs it) | decides the lane and the affected packages; CI passes the PR's base SHA, `check-changed` diffs against `origin/main` and adds uncommitted files |
| `fast-lane` | `just check-changed` | same packages and arguments; the cross-target and wasm32 clippy and the per-feature pass for changed manifests run only when their rustup target or cargo-hack is installed (`just doctor full`); the flui-platform leg needs `xvfb-run` (Linux) |
| `fast-lane-ios` | `just check-changed` (on a Mac with the iOS target) | the same iOS runner clippy as `cli-macos`, run on a PR when `flui-app` is in scope |
| `clippy` | `just clippy` (in `just gate`) | — |
| `test` (ubuntu, macos, windows) | `just test-ci` + `just build-all-targets` | one host OS, not three; `test-ci` does not link examples (`build-all-targets` does); the flui-platform leg needs `xvfb-run` (Linux) |
| `test-features` | `just test-features` | — |
| `live-smoke` | `just live-smoke`, `just live-smoke-wayland` | Linux only (Xvfb, weston); `ci-full` runs them on Linux and says it skipped them elsewhere |
| `gpu-test` | `just gpu-test` | CI renders on Windows' WARP software rasterizer; locally the host adapter renders, so a local-only mismatch is a host difference to look at, not a CI verdict |
| `platform-windows` | `just test-ci` (on Windows) | `test-ci` runs the all-features pass only; CI adds a default-features pass |
| `bench-compile` | `just bench-compile` | — |
| `doc` | `just doc-strict` (in `just gate`) | — |
| `deny` | `just deny` | its second step, re-reading ADR-0045's reopen condition when a PR touches `Cargo.lock`, reads the PR through the GitHub API: CI only (and advisory) |
| `doc-test` | `just test-doc` (in `just ci`) | — |
| `msrv` | `just msrv` | needs the `rust-version` toolchain (`just doctor full`) |
| `miri` | `just miri` | nightly + miri; advisory in CI too (`continue-on-error`) |
| `feature-matrix` | `just feature-matrix` | CI splits the packages into three parallel slices and first asserts the slices cover the workspace exactly once; locally it is one run over the workspace, which needs no such check |
| `wasm-check` | `just wasm-check`, `just wasm-link-check`, `just wasm-test` | `wasm-link-check` needs `wasm-tools`; `wasm-test` installs the locked `wasm-bindgen-cli` itself |
| `cli-macos` | `just test-ci` (flui-cli's tests) + `just cross-typecheck` (its iOS clippy line) | the same commands; they only mean "macOS" on a Mac |
| `cross-typecheck` | `just cross-typecheck` | needs the four targets (`just doctor full`) |
| `ci` | — | CI only: the single required check. It verifies that every gated job ran and passed, and that the jobs which skipped are exactly those the plan skips |
| `notify-main-red` | — | CI only: opens or updates the "CI is red on main" issue after a red heavy run on main or nightly |

The other workflows (`weekly.yml`, `release.yml`, `docs.yml`,
`coderabbit-trigger.yml`) are scheduled or event-driven, not per-PR gates, and
have no local mirror. `full-ci.yml` only turns the `full-ci` label into a
re-run of the PR's own `ci.yml` run.

The `gpu-test` job additionally runs the full `testing` readback
suite on a windows-latest runner (WARP software rasterizer) and is
merge-blocking. Failing snapshot/readback tests upload debuggable artifacts:
insta `.snap.new` candidates (`test` job) and readback PNG dumps
(`gpu-test` job, written when `FLUI_READBACK_DUMP_DIR` is set).

A scheduled `weekly.yml` workflow (Mondays, or manually via
`workflow_dispatch`) re-checks RustSec advisories against the committed
lockfile and builds/tests against a fresh `cargo update` — early warning for
upstream semver breakage. It is not a merge gate.

A change cannot be merged if any of these fail. If you encounter a flaky test, file a fix issue rather than retrying CI.

## See Also

- [Getting Started](getting-started.md) — toolchain setup and first build
- [Contributing](../CONTRIBUTING.md) — planning a change, git hygiene, bug reports
- [`AGENTS.md`](../AGENTS.md) — current performance and testing requirements
