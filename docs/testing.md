[← Crates Map](crates.md) · [Foundations](FOUNDATIONS.md) · [Roadmap](ROADMAP.md) · [Back to README](../README.md) · [Contributing →](contributing.md)

# Testing

This page documents the test, lint, format, and benchmark commands enforced for FLUI. All gates listed here must pass before a change is merged.

## Map of the testing layer

FLUI's test support is a stack, not one harness. Each tier drives the machine at
one depth; pick the **shallowest tier that can fail for the reason you care
about** — a layout bug found by a `RenderTester` test names the render object,
the same bug found by a golden names a PNG.

| Tier | Drives | Entry point | Enabled by |
|------|--------|-------------|------------|
| Diagnostics | Structured self-description of any node | `flui_foundation::{DiagnosticsNode, DiagnosticsBuilder}` | always |
| Painting | A `DisplayList`, no canvas boilerplate | `flui_painting::testing::{record, command_count, bounds}` | `flui-painting/testing` |
| Layer | A `LayerTree` built declaratively | `flui_layer::testing::{LayerTester, layer, inspect}` | `flui-layer/testing` |
| Render object | A real `PipelineOwner` — layout, paint, hit-test, intrinsics | `flui_rendering::testing::{RenderTester, Probe}` | `flui-rendering/testing` |
| **Frame** | A **whole headless frame** on a virtual clock: build → layout → paint → composite, gestures, animation, async tasks | `flui_testing::HeadlessBinding` | dev-dependency |
| **Widget** | A mounted widget tree with geometry probes and synthetic input | `flui_widgets::testing::{lay_out, LaidOut}` | `flui-widgets/testing` |
| Accessibility | The assembled semantics tree, queried by role | `flui_testing::a11y::{A11yTree, A11yQuery}` | dev-dependency |
| Gesture replay | A scripted gesture replayed with its timing | `flui_testing::replay::PointerScript` | dev-dependency |
| Log capture | The `tracing` events a frame emitted | `flui_testing::log_capture::capture` | dev-dependency |
| GPU readback | Real pixels off a real device (WARP in CI) | `flui-engine`'s readback suite | `flui-engine/enable-wgpu-tests` |
| Visual regression | Whole-demo pixels vs. committed PNGs | `tests/golden_screenshots.rs` | `flui/golden` |
| Live E2E | A real window, real X11/Wayland input, real exit code | `tools/live-smoke` | `just live-smoke` |

Two structural rules hold across the stack:

- **Test-only APIs live in `flui-testing`**, not behind a `testing` feature on a
  shipped crate. Where layering forbids the move — `flui_widgets::testing`
  mounts `FocusRoot`/`VsyncScope`/`GestureArenaScope`, which are widgets, and
  `flui-testing` may never depend on the widget catalog — the harness stays put
  but is *built on* `flui-testing`, so the shared machinery is not forked. See
  [`crates/flui-testing/AGENTS.md`](../crates/flui-testing/AGENTS.md).
- **Mount through `HeadlessBinding::mount_root`.** It owns the eight-step
  bootstrap whose ordering is load-bearing, and its contract is that the
  bootstrap frame is the same frame `pump_frame` runs (same layout↔build
  fixpoint, same lazy-sliver service pass). Hand-rolled copies of that sequence
  have already drifted once, silently: of the eight that existed, one
  bootstrapped with a bare `PipelineOwner::run_frame` and captured every
  `SliverAppBar` delegate child unbuilt, and none ran the lazy-sliver service
  pass. All eight now go through `mount_root`.

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
bash scripts/port-check.sh                                 # port-check: architecture refusal triggers
cargo clippy --workspace --all-targets -- -D warnings      # clippy: lint gate — zero warnings
cargo nextest run --workspace --exclude flui-platform --locked --no-fail-fast  # test-ci (flui-platform gets its own invocation below — see CI Expectations)
FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast  # test-ci: flui-platform, headless (Linux only — apt install xvfb; skipped with a message on other hosts, see justfile)
cargo test --workspace --locked --doc                      # test-doc: doc-tests (flui-platform included — its doctests need neither device above)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --document-private-items  # doc-strict
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
- **Property-based tests** use [`proptest`](https://docs.rs/proptest) for layout algorithms and geometric operations.
- **Visual regression tests** live in `tests/golden_screenshots.rs`: each demo renders headless and is compared against a committed PNG in `tests/goldens/`. See [Visual regression](#visual-regression-goldens) below for the run/regenerate workflow and what counts as a pass.
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
| `flui-testing` | [crates/flui-testing/AGENTS.md](../crates/flui-testing/AGENTS.md) | `HeadlessBinding` (`pump_frame`, `mount_root`, `replay`), `a11y::A11yQuery` — a **dev-dependency**, not a `testing` feature |
| `flui-widgets` | [crates/flui-widgets/AGENTS.md](../crates/flui-widgets/AGENTS.md) | `testing::{lay_out, LaidOut, settle_lazy}` — the canonical widget harness, shared verbatim by `flui-material` / `flui-cupertino` |

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
hand-roll the bootstrap (see the map above).

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
contract; `flui-foundation` is emission-only and may not construct a subscriber
(`crates/flui-foundation/AGENTS.md`), which is also why the primitive lives in
`flui-testing` rather than at the bottom of the DAG where every crate could
reach it without an edge.

## Visual regression (goldens)

```bash
just golden          # compare against tests/goldens/ (needs a GPU)
just golden-update   # regenerate after an intended visual change, then review the diff
```

Off by default (`--features golden` on the `flui` package) because the goldens
are machine-specific: GPU and driver differences move anti-aliased edges, so
they must be regenerated on one reference device.

**What counts as a pass is only a pixel comparison against a committed golden.**
Two paths that used to go green having compared nothing are now failures:

- a **missing golden fails** — writing the absent PNG and returning would let a
  deleted or never-committed golden heal itself into a pass, so a golden is only
  ever written under an explicit `UPDATE_GOLDENS=1` regeneration;
- a **missing GPU fails** — set `FLUI_GOLDEN_ALLOW_NO_GPU=1` to skip explicitly
  on a device-less machine, which reports each skip on stderr instead of
  pretending to have run.

No CI job runs this suite; it is a local gate on the reference device. The
committed PNGs predate the bootstrap consolidation and were captured without
the layout↔build fixpoint — regenerate them before trusting a comparison.

To *look* at what a tree renders without a window, capture it instead:

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
cargo check -p flui-platform --locked --all-targets --target x86_64-pc-windows-msvc            # cross-typecheck job — just cross-typecheck
cargo check -p flui-platform --locked --all-targets --target aarch64-apple-darwin              # (type-check only: no link, no tests)
cargo deny check                                              # advisories / bans / licenses / sources
cargo bench -p flui-rendering --no-run                        # bench-compile job
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items  # doc job
cargo nextest run --workspace --exclude flui-platform --locked --no-fail-fast
FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --locked --all-features --no-fail-fast  # test job's dedicated flui-platform step
cargo test --workspace --locked --doc
cargo check --workspace --all-targets --locked                # repeated on Rust 1.97 (MSRV job)
cargo +nightly miri test -p flui-rendering --lib pipeline::owner  # advisory (continue-on-error); NARROW — every
                                                              # unit test under that module, including PipelineCell
                                                              # checkout, an owner-local run_frame traversal, a
                                                              # reentrant-layout walk, and two real-NodePtr walks
                                                              # driving layout_dirty_root through every reborrow phase
                                                              # of layout_subtree_borrowed_impl. Deeper sliver walks
                                                              # and intrinsics queries are not interpreted.
```

The `gpu-test` job additionally runs the full `enable-wgpu-tests` readback
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
- [Contributing](contributing.md) — workflow, commits, speckit
- [`AGENTS.md`](../AGENTS.md) — current performance and testing requirements (`.specify/memory/constitution.md` does not exist in this checkout)
