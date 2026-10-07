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
| Realm | A `UiRealm`'s own frame transaction, multi-presentation routing and failure containment, submitting to a scripted sink | `flui_runtime::ui_realm::UiRealm::for_test` with `flui_runtime::testing::{ScriptedSink, TestWindow}` (the realm tests live in `crates/flui-runtime/src/ui_realm/`) | `flui-runtime/test-support` |
| **Widget** | A mounted widget tree with geometry probes and synthetic input, every frame the realm's own `UiRealm::pump` on a manual clock | `flui_testing::widgets::{lay_out, LaidOut}`, `flui_testing::HeadlessHost` | dev-dependency |
| Accessibility | The assembled semantics tree, queried by role | `flui_testing::a11y::{A11yTree, A11yQuery}` | dev-dependency |
| Gesture replay | A scripted gesture replayed with its timing | `flui_testing::replay::PointerScript` | dev-dependency |
| Log capture | The `tracing` events a frame emitted | `flui_testing::log_capture::capture` | dev-dependency |
| GPU readback | Real pixels off a local device (adapter required) | `flui-engine`'s readback suite | `flui-engine/testing` |
| Demo composition | A whole demo tree's committed `LayerTree`, as structured text | `tests/demo_layer_snapshots.rs` | `flui/material` + `flui/cupertino` |
| Live E2E | A real window, real X11/Wayland input, real exit code | `tools/live-smoke` | `cargo xtask live-smoke` |

Two structural rules hold across the stack:

- **Test-only APIs live in `flui-testing`**, not behind a `testing` feature on a
  shipped crate. It sits above the frame runtime and the widget catalog, so
  the widget harness lives there too and drives the product frame
  transaction: `lay_out` mounts its tree in a `HeadlessHost`, whose frames are
  `UiRealm::pump` (ADR-0083 §4).
- **On the substrate driver, mount through `HeadlessBinding::mount_root`.** It owns the eight-step
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
`cargo xtask wasm-link` proves rust-lld resolves the cdylibs (with a committed import allowlist,
because on wasm32 an undefined symbol becomes an *import* rather than a link error). None of them
execute a single instruction.

It is not an academic gap. Swap `web_time::Instant` for `std::time::Instant` in
`crates/flui-foundation/src/clock.rs` — the substitution that module's own comment justifies as
*"the std one panics there"* — and every compile-only step stays green while the code panics
`time not implemented on this platform` the moment it runs. That was the state of the workspace
until issue #985.

`cargo xtask wasm-test` (CI: the last steps of the `wasm-check` job) discovers the participating
crates — those declaring a wasm32 `wasm-bindgen-test` dev-dependency, resolved by
`cargo xtask wasm-test-crates` parsing the manifests rather than grepping them — and hosts their
tests on node through `wasm-bindgen-test-runner`. It runs **both** target kinds, because which one
is possible depends on visibility: an integration test (`tests/wasm32.rs`) sees only the public
API, while a `pub(crate)` seam is reachable *only* from a lib test. Two things about it are deliberate:

- **What belongs in that file** is behaviour that *differs* on wasm32, or a native-target
  substitution whose whole purpose is keeping wasm32 working. A test that would pass identically on
  native costs a wasm build and proves nothing extra.
- **The assertion count is checked, not trusted.** A runner that finds no tests exits 0 and prints
  `0 passed`, which is indistinguishable from a passing suite, so both the xtask command and the CI
  step fail when the count is zero.

Three versions must agree or the runner refuses to start: the locked `wasm-bindgen`,
`wasm-bindgen-test` (pinned `=0.3.77` in flui-foundation — the unpinned `"0.3"` resolves to 0.3.78
and would bump the workspace lock as a side effect of adding a dev-dependency), and
`wasm-bindgen-cli`, whose version xtask and the CI step both read out of `Cargo.lock`
(`cargo xtask locked-version wasm-bindgen`).

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
`spawn_local` — is now executed, from `crates/flui-runtime/src/execution.rs`'s
`wasm_sequential_backend_tests`. It stays a **lib** test: it pins the private `Backend::Sequential`
next to its definition, the `default_pools_started` probe it reads exists only under `cfg(test)` or
the `test-support` feature, and the one execution API an embedder reaches through `flui-app`
(`DeterministicExecutors`) is a target-independent FIFO that would pass identically on native.

Still compile-only: `flui-platform`'s web backend, which is wasm32-only and has no executing
coverage at all. See #985.

## Quality Gates

The local pre-review gate is:

```bash
cargo xtask ci
```

It runs, in order (`tools/xtask/src/tasks.rs` is the authority):

```bash
cargo xtask checks                        # fmt, typos, taplo, markdown links (docs-links: lychee, offline), the paths, packages and llms.txt links the docs name (docs-paths), workspace (tiers, layers, manifests, test reachability, ADR numbers), reach (what each crate's resolved graph may contain, and the hot-reload facts), module-dag (import direction between a crate's modules), toolchain, wgsl, globals (process-global state, ADR-0097), the docs-only allowlist, font assets, file-length, markers and changelog fragments (each with its self-test); builds only xtask
cargo xtask lint                          # clippy -D warnings, as the CI clippy job runs it: the workspace, then flui-engine's `testing` code
cargo xtask doc-strict                    # cargo doc --workspace --no-deps --locked --document-private-items with every workspace `testing` feature on
cargo xtask test                          # nextest over the local scope, flui-platform headless, then the nested-cargo group (see "What `cargo xtask test` runs")
cargo test --workspace --locked --doc     # doc-tests (flui-platform included — its doctests need no display server)
```

The workspace test-reachability check takes test target source paths from Cargo
metadata, including targets with an implicit path. It parses direct external
module declarations with `syn`; a commented declaration or an example inside
a string cannot mount a test file. This checks direct mounts, not arbitrary
macro expansion or conditional module graphs.

The first three are `cargo xtask gate`, the non-test half. Locally, `checks`
skips typos, taplo or lychee with a message when they are not installed; CI
passes `--strict`, which makes a missing one a failure. The flui-platform step
of `test` needs `xvfb-run` on Linux (`apt install xvfb`), runs without it on
Windows, and is skipped with a message on macOS.

The xtask commands that build or test the workspace (`check-changed`, `test`,
`ci`, `gate`, `lint`, `gpu-test` and the rest; `Command::is_heavy` in
`tools/xtask/src/main.rs` is the list) take one lock for the user on this
machine: `%LOCALAPPDATA%\flui\xtask-heavy.lock` on Windows,
`$HOME/.cache/flui/xtask-heavy.lock` elsewhere (`XDG_RUNTIME_DIR` is not
consulted), a per-user directory in the temporary directory when that
variable is unset or relative, or an absolute `FLUI_XTASK_LOCK_FILE` (a
relative one is refused with a warning). `clean-nested` and `worktree prune`
take it too, since they delete what a running build uses. Runs from different
checkouts queue instead of oversubscribing the machine, and a
waiting run names the one it waits for (best effort). The OS releases the lock
when its holder exits, crashed or not. One composite command running another,
in-process or as a child process, does not wait for itself.
`FLUI_XTASK_NO_LOCK=1` skips the lock; `--dry-run` never takes it.

**Adding a new gate** means two changes together, not one: a `cargo xtask`
command (so a contributor can run it standalone) *and* a step in
`.github/workflows/ci.yml`'s `checks` job (so CI actually runs it — `gate` and
`ci` are not themselves invoked from CI). A check folded into `cargo xtask
checks` gets both at once, since that job runs it. A command with no CI step
only runs when someone remembers to run it by hand.

### What `cargo xtask test` runs

One scope for the whole local suite:
`--workspace --exclude flui-platform --lib --bins --tests
--features flui/material,flui/cupertino,flui/persist,flui-devtools/agent`, run as the two
stages below. Text-size tests measure on Parley because the default build does
(ADR-0092 §10 step 4a); no feature selects another measurement.
Two choices in it differ from CI on purpose:

- **One feature slice.** The facade turns no catalog on by default, so both
  (`material`, `cupertino`) are named and join the workspace run through
  feature unification. The
  alternative, a second `cargo nextest run -p flui --features ...`, resolves
  features for `flui`'s own graph, without the dev-dependency features other
  members switch on (`testing` and friends), so every crate the two runs share
  was built twice under different hashes. No test is lost: the root crate has
  no `cfg(not(feature = ...))` code, so the no-feature facade's tests are a
  subset of these, and `cargo xtask facade-combos` lints that build.
- **Examples are not linked.** `cargo nextest run` with no target flags builds
  every example of every package it tests: about 60 binaries, each linking the
  whole render stack, on every run. `--lib --bins --tests` selects exactly the
  targets that have tests. Examples still **compile** in `cargo xtask lint`
  (`--all-targets`, part of `cargo xtask gate`), so a type error in one still
  fails the local gate. What goes unchecked locally is only a *link* failure
  specific to an example; CI's `test` job and a local
  `cargo xtask build-all-targets` catch it.

Measured against the previous two-slice scope (2026-09-22, M1/8 GB,
`CARGO_BUILD_JOBS=6`, shared target, after an edit to the bottom value-types
crate of that workspace so every crate above it rebuilds): `--no-run` 437.1 s + 107.7 s = 544.8 s before,
326.1 s after; `debug/examples` 1.7 GB with 125 linked example binaries
before, empty after. Test names: 9754 + 58 runs = 9769 distinct tests before
(43 ran twice), 9769 after, none lost. This was measured on a target
directory shared between worktrees, before the per-checkout rule below; the
timings are indicative, and CI on the change is the authority for the test
set.

### Nested-cargo tests

The nested tests dominate the suite's wall-clock: tests that run a `cargo` or
`rustc` of their own. Two nextest test groups hold them: `trybuild`, the
`compile_fail` suites (`flui-engine`, `flui-rendering`, `flui-painting`,
`flui-view`'s `trybuild_ui`, `flui-widgets`' `routable_ui`), whose compiler
output does not depend on the host; and `nested-cargo`, the `flui-cli`
template tests (`cli_create::generated_*`) and every `flui::facade_consumer`
test, which build a project with its own feature graph on the host. Locally,
with their build caches cold, most take one to five minutes; the rest are
quick. `.config/nextest.toml` names each group with one filter;
`cargo nextest show-config test-groups` lists the members. Selecting by group needs nextest 0.9.133 or newer (the
config's `nextest-version` enforces it).

The configuration also recommends nextest 0.9.145 or newer. That release fixes
[capture-pipe inheritance on Unix, notably Apple platforms](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.145),
which could falsely report leaked handles when tests spawn concurrently. A runner
below the recommended version warns and still runs; below the required version
it refuses. Check the installed runner with `cargo nextest show-config version`
before diagnosing an intermittent leak. This command returns advisory exit code
10 below the recommendation, whereas ordinary test runs only warn.

- `cargo xtask test` (and so `cargo xtask ci`) runs
  `-E 'not (group(nested-cargo) | group(trybuild))'` first, then
  `-E 'group(nested-cargo) | group(trybuild)'` as its last stage. Nothing is
  dropped: the two filtersets are complements, so together they are the
  whole suite.
- `cargo xtask test --fast` is the quick local loop: the same scope without
  the nested tests, ending with a line that names what it skipped.
- `cargo xtask test --nested` runs only the nested tests (the union of `test-nested`
  and `test-trybuild`); `cargo xtask test --no-trybuild` everything but the `trybuild` group
  (useful for local native Windows runs).

Platform compiler diagnostics need no display or event loop. Exact diagnostics
require the pinned toolchain's `rust-src` component
(`rustup component add rust-src`), because exact snapshots include standard-library
declarations. CI installs it in the diagnostic test job.
The nested and trybuild stages run `flui-platform`'s `compiler_guards` target separately on
every host; the native platform leg excludes that group. `check-changed` also
runs the portable target whenever platform code is in scope, including hosts
where it skips the native display suite.

To narrow either stage, combine with `&` inside the single `-E`:
`-E 'not group(trybuild) & package(flui-view)'`. A second `-E` is ORed
with the first, not intersected: `-E 'not group(trybuild)' -E
'package(flui-view)'` runs everything outside the group *and* flui-view's
`trybuild_ui` test inside it. `group()` works only on the command line (a
profile's `default-filter` rejects it), which is why the stages are filtersets
rather than profiles.

Measured on the workspace invocation (2026-09-22, M1/8 GB, `CARGO_BUILD_JOBS=6`,
wall-clock including the build):

| Nested-build caches | One invocation (before) | Two stages (`cargo xtask test`) |
|---|---|---|
| warm, nothing changed | 191.2 s | 101.6 + 13.7 = 115.3 s |
| after an edit to the bottom value-types crate (registry deps warm) | 576.9 s | 406.5 + 87.2 = 493.7 s |
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
set built through them (1.5-3 GB each is normal). `cargo xtask clean-nested`
deletes them, safe whenever no test run is using them; `cargo sweep --installed .`
then `cargo sweep --maxsize 12GB .` bounds the main target directory (oldest
artifacts first).

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

### Bounding disk use of agent worktrees

A worktree's `target/` reaches 30-60 GB after a workspace build and the full
suite. A Claude Code subagent started with worktree isolation gets a checkout
of its own under `.claude/worktrees/`, so a session that fans out agents
multiplies that. To keep it bounded:

- **An agent that only reads needs no worktree.** Searching, answering a
  question or reviewing a PR (`gh pr diff`) runs in an existing checkout;
  isolation is for agents that edit.
- **A writing agent builds only what it touched:** `cargo check -p <crate>`
  and `cargo nextest run -p <crate>` while iterating, then
  `cargo xtask check-changed` once before the PR. A `--workspace` build or
  `cargo xtask test` is most of a short-lived worktree's disk.
- **`CARGO_INCREMENTAL=0` in a short-lived worktree.** Incremental caches are
  over half of a used debug directory (9.30 GB against 4.06 GB without them,
  [build-footprint.md](../design/build-footprint.md) R2); they pay off only
  across many edits to one crate, which a one-PR agent rarely makes.
- **Delete a worktree's `target/` once its branch is pushed.** From then on CI
  is the proof; a review fix rebuilds in minutes, while keeping the directory
  costs tens of GB until the merge.
- **After the merge, `cargo xtask worktree prune`.** It surveys every worktree
  git knows, `.claude/worktrees/` included, not only `.worktrees/`, and removes
  a merged one with its `target/` and branch. It keeps a worktree that is
  locked (Claude Code locks an agent's worktree while the agent runs, and a
  lock left by a process that died stays until `git worktree unlock`),
  detached, unmerged, or holding modified, untracked or ignored files other
  than `TASKS.md` and `target/`. `cargo xtask worktree prune --dry-run` names
  each kept one and why; deleting a kept one's `target/` by hand still frees
  the space.
- **The main checkout:** `cargo xtask clean-nested` drops the nested-test
  caches, then `cargo sweep --installed .` and `cargo sweep --maxsize 12GB .`
  bound the rest ([Nested-cargo tests](#nested-cargo-tests)).

## Build

```bash
cargo build --workspace              # full workspace build
cargo build --release --workspace    # optimized build (LTO enabled in release profile)
cargo check -p <crate>               # incremental type check for a single crate
cargo clean                          # wipe target/ before a fresh build
```

A bare `cargo build` at the root builds only the `flui` facade; pass `--workspace` for everything. Use `cargo ndk` for Android targets (see [Getting Started](getting-started.md)).

### Local machine mode (shared, memory-limited)

On a shared, memory-constrained dev machine — several agent worktrees off the same checkout —
run one compiling worker at a time (the heavy xtask commands queue behind one host-wide lock for
this, see AGENTS.md's Commands section) and size `CARGO_BUILD_JOBS` to available RAM rather than
core count. Each worktree still builds into its own `target/`
("One target directory per checkout", under [Nested-cargo tests](#nested-cargo-tests)); memory is bounded by serializing
builds, never by sharing a target directory, and disk by the
[agent-worktree rules](#bounding-disk-use-of-agent-worktrees). A docs-only change never needs a
workspace build: `cargo xtask checks`, which builds only xtask, is the full local gate for it,
so a docs worktree stays green without a multi-GB `target/` or a wait for the workspace builds.

## Test Commands

### Workspace-wide

```bash
cargo test --workspace                            # all tests, all crates
cargo test --workspace --no-fail-fast             # keep going after failures
cargo test --workspace --release                  # run tests against the release profile
```

### Per crate

```bash
cargo test -p flui-foundation
cargo test -p flui-painting
cargo test -p flui-platform
```

### A single test or filter

```bash
cargo test -p flui-foundation indexed_slot                 # filter by name
cargo test -p flui-foundation indexed_slot -- --nocapture  # surface stdout/println from tests
cargo test -p flui-foundation -- --test-threads=1          # serialize tests (debugging)
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
| Core | 80 % | `flui-foundation`, `flui-painting`, `flui-rendering`, `flui-view` |
| Platform | 70 % | `flui-platform` |
| Widget | 85 % | (future widget crates) |

Generate a coverage report with [`cargo-llvm-cov`](https://crates.io/crates/cargo-llvm-cov),
the only coverage tool this workspace uses:

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --html    # report: target/llvm-cov/html/index.html
```

These thresholds are a target, not a gate: no CI job enforces them today.

## Mutation Testing

Coverage says a line ran; a mutation run says whether a test would notice it
being wrong. [`cargo-gamma`](https://crates.io/crates/cargo-gamma) rewrites
operators, conditions, constants and function bodies, and reports each mutant
a test kills, one that survives, and one no test reaches. It compiles every
mutant into one instrumented build and switches them on at run time, so a run
over a whole crate takes minutes rather than a rebuild per mutant.
`gamma.toml` at the workspace root holds the shared settings (the timeout
floor, and skipping the nested-cargo tests).

```bash
cargo binstall cargo-gamma      # or: cargo install cargo-gamma --locked
cargo gamma run -p flui-painting --file crates/flui-painting/src/styling/color.rs
cargo gamma run -p flui-foundation # a whole crate
# Also let another crate's tests judge the mutants (a crate that depends on it):
cargo gamma run -p flui-foundation --test-package flui-painting
```

The report is in `target/cargo-gamma/` (`gamma-report.html` to browse,
`gamma-report.json` to diff two runs). Use it when a change rewrites or
deletes tests: run it before and after on the files the tests cover, and
every mutant the old tests killed should still be killed. A survivor is
either a missing assertion or an equivalent mutant (one no input can tell
apart, like `<` vs `<=` at a boundary both branches agree on); say which in
the change. `const fn` bodies are not mutated.

These runs are not a CI gate.

## Benchmarks

`criterion` is used for regression detection. Per-crate benchmark commands:

```bash
cargo bench -p flui-foundation
cargo bench -p flui-rendering
cargo bench -p flui-engine
```

Benchmark results are written under `target/criterion/` as HTML reports.

`cargo bench -p flui-widgets --bench signals_rebuilds -- --noplot`
(`crates/flui-widgets/benches/signals_rebuilds.rs`) runs ADR-0074's go/no-go: `setState` against realm-scoped signals on
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
  `cargo xtask bench-collect before` on the baseline commit, apply the change,
  then `cargo xtask bench-collect after` and `critcmp before after` (needs
  `critcmp`, e.g. `cargo binstall critcmp`). Criterion also prints its own
  change estimate against the last run of the same bench when run directly.
  `bench-collect` uses fresh isolated output for each target, so compare its
  published named baselines with `critcmp` instead.
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

### Phase counters and the perf baseline

Timings are noisy; counts are not. `PipelineOwner::counters()` reports what the
pipeline did per phase — layout passes and roots, nodes laid out and painted,
layers produced and grafted from retained boundaries, semantics nodes
published, frames produced — as monotonic totals (a frame's work is the
difference across it; see `crates/flui-rendering/ARCHITECTURE.md`, "Phase
counters"). `HeadlessBinding::last_frame_report()` pairs that difference with
`BuildOwner::last_frame_build_report()` (distinct elements rebuilt and builds
run) for the last pump.

`crates/flui-widgets/tests/perf.rs` drives a fixed app — a label over a lazy
10 000-row list — through an idle 10 s, a one-screen scroll, a one-label change
and a full reassemble, and asserts budgets on those reports. `cargo xtask perf`
runs that target with `FLUI_PERF_OUT` set and compares every count with
`crates/flui-widgets/perf/baseline.toml`:

```bash
cargo nextest run -p flui-widgets --test perf   # the budget assertions alone
cargo xtask perf                                # advisory: prints differences, exits 0
cargo xtask perf --check                        # any difference fails
cargo xtask perf --bless                        # rewrite the baseline from this run
cargo xtask perf --self-test                    # the comparison on planted fixtures (part of `checks`)
```

The comparison is exact: a count that rose is a regression, and one that fell
is a finding too, until `--bless` locks the improvement in — otherwise a later
regression back up to the old value would pass. Commit a re-blessed baseline
with the change that moved it and say why in the PR. The run writes the merged
counts to `<target>/perf/current.toml`. The checked-in baseline was blessed on
Windows. The CI redesign (`design/ci.md`) places an advisory `perf` job in the
`wide`, `full` and `extended` lanes, blocking at the B1 exit; that job is not in
the workflows yet, so only `perf --self-test` and the budget assertions are on
the merge path.

## Linting

`cargo clippy` is the canonical lint command. The constitution requires `clippy::all` and `clippy::pedantic` at warn level workspace-wide.

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask deps                                   # cargo-deny + cargo-shear over every member
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
  `flui-widgets` unit tests must not use the headless harness: `flui-testing` depends on
  `flui-widgets`, so a unit test under `src/` links a second copy of the library. It still
  compiles, but the realm's inherited scopes (`MediaQuery`, `FocusRoot`, `VsyncScope`) are the
  other copy's types, so the test's own lookups of them read `None`. Nothing but review
  catches it; a test that mounts a tree lives in `crates/flui-widgets/tests/` (ADR-0083 §4).
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
- **A family of scenarios is one table test.** `flui-view`, `flui-runtime`, `flui-app`,
  `flui-scheduler` and `flui-testing` run related scenarios (a failure-recovery matrix, a
  realm-isolation family) as rows of a single `#[test]` through a small `run_table`
  helper: each row is a named `fn()`, every row runs even after one fails, and the panic
  lists the failing row names. The rows keep their own assertions; the table keeps the
  set small enough that a refactor touches a table, not a hundred tests.
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
| `flui-painting` | [crates/flui-painting/src/testing/mod.rs](../crates/flui-painting/src/testing/mod.rs) | `record`, `command_count`, `bounds`, `diagnostics` |
| `flui-foundation` | [crates/flui-foundation/docs/TESTING.md](../crates/flui-foundation/docs/TESTING.md) | `DiagnosticsNode` / `DiagnosticsBuilder` for structured assertions (no `testing` module) |
| `flui-testing` | [crates/flui-testing/README.md](../crates/flui-testing/README.md) | `HeadlessBinding` (`pump_frame`, `mount_root`, `replay`), `a11y::A11yQuery` — a **dev-dependency**, not a `testing` feature |
| `flui-testing` (widgets) | [crates/flui-testing/ARCHITECTURE.md](../crates/flui-testing/ARCHITECTURE.md) | `widgets::{lay_out, LaidOut, settle_lazy}`, `widgets::harness::{mount, mount_with_ime}` — the canonical widget harness on the realm, shared verbatim by `flui-widgets` / `flui-material` / `flui-cupertino` |

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
in harness trees via [`ParentDataSeed`](../crates/flui-rendering/src/testing/parent_data.rs)
on [`TreeNode::with_parent_data_seed`](../crates/flui-rendering/src/testing/tree.rs).
The pipeline clones each seed into the per-walk child slots before
`perform_layout` runs.

### Multi-frame and animation testing

After `.run_frame()`, [`FrameRun`](../crates/flui-rendering/src/testing/harness.rs)
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

`flui_testing::widgets::lay_out` is the widget tier: it mounts the tree in a
`HeadlessHost`, under the realm's own root scopes, and adds geometry probes;
every frame, the mount included, is `UiRealm::pump`. It is one harness, shared verbatim by
`flui-widgets`, `flui-material`, and `flui-cupertino` — the per-crate
`tests/common/mod.rs` files are thin re-export shims, so mount ordering,
pointer-contact identity, and virtual-clock policy cannot drift apart between
crates again.

Two behaviours worth knowing before you write an assertion:

- **Lazy children build after paint**, not during layout, so a
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

### Text fields and IME

`flui_testing::text_store_kit` is the conformance kit for any widget that
implements `flui_platform_api::TextStore` (ADR-0090): write a
`TextStoreFixture` for it and call `text_store_kit::assert_conforms(&mut fixture,
KIT_VERSION)`. The kit holds frame transactions itself, through the
`CommitGate` it installs with `TextStore::set_commit_gate`; the fixture only
supplies the commit anchor (`pump`). `InMemoryFixture` is the worked example, and
`crates/flui-widgets/tests/text_store_kit.rs` runs the built-in `EditableText`
through it.

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
`flui-view`, `flui-interaction` and `flui-app` do, through a
**dev-dependency cycle**: `flui-testing` depends on them normally, and cargo
permits the reverse edge for dev-dependencies precisely so a lower crate can
use the test support built on it. `flui-devtools` does too, through a plain
dev-dependency, since it sits above `flui-testing`.

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

The tree under test is a Material counter (`Center` → `Column` → prompt
`Text` / count `Text` / `ElevatedButton`, a `StateCell` bound in
`init_state`), the shape the CLI `counter` template
(`crates/flui-cli/src/templates/counter.rs`) has with Material in place of
its `RawButton` and `Signal` — see `crates/flui-view/src/state_cell.rs` for
the state primitive. Five steps, each backed by a documented, facade-reachable API:

| Step | What it does | API |
|------|--------------|-----|
| 1. Mount | Bootstrap the tree headlessly | `flui::testing::HeadlessBinding::mount_root` |
| 2. Inspect structure | Dump the render tree, check a known node is there | `flui::testing::rendering::render_diagnostics` over `HeadlessBinding::pipeline_owner().with(...)` + `flui::foundation::DiagnosticsNode::to_string_deep` |
| 3. Inspect semantics | Find the button by its accessible label | `flui::testing::HeadlessBinding::{enable_semantics, a11y_tree}` + `flui::testing::a11y::A11yTree::find_by_label` |
| 4. Drive | Tap the label's own bounds | `flui::testing::HeadlessBinding::replay` with `flui::testing::replay::PointerScript::tap` |
| 5. Assert | Confirm the rendered count advanced | The diagnostics dump again, checked for the `RenderParagraph` `text` property |

Step 4 is deliberately **not** `flui_testing::widgets::lay_out`'s
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
The test mounts that Material tree and queries `"Increment"` from the
framework's own node; the template itself now uses `RawButton`, which
publishes the same `Semantics(container, button, enabled)` node.

The acceptance criterion's "actionable command failures" half is the error
`A11yTree::find_by_label` already produces, needing no new helper: a label that
is not in the tree yields `flui::testing::a11y::A11yQueryError::NotFound`, which
names the search and lists the labels that *were* reachable. No test asserts it.

Writing the test also found that the facade's `flui::testing::rendering`
module did not re-export `render_diagnostics` (`flui-rendering`'s render-tree
dump); it does now, and step 2 uses it.

## Demo composition snapshots

```bash
cargo xtask demo-snapshots   # run
cargo insta review           # review and accept intended changes (cargo-insta)
```

Each demo tree mounts headless at 900x760 through `flui-testing`'s canonical
bootstrap, and the `LayerTree` its bootstrap frame commits is serialized to
structured text — the layer structure plus every draw command, with geometry,
colors, transforms, and text — and compared against a committed `insta`
snapshot in `tests/snapshots/`. A widget that moves or resizes, a shadow that
stops being emitted, a clip that disappears, a subtree that stops being built:
each changes those lines and fails the matching test, naming the layer and the
command.

No GPU, no device-specific baseline: CI's `test` job (its scope turns
`flui/material` and `flui/cupertino` on) runs the suite
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
by flui-engine's local readback/oracle suite (`cargo xtask gpu-test`), and
that a real window presents at all by `tools/live-smoke`. What is genuinely
lost — the anti-aliased pixels of a whole demo — was guarded by nothing before,
since the suite ran on no job.

### Determinism

Text is measured and painted on the realm's `FontCollection`. A test bootstrap
builds it with `FontCollection::new()`, which holds only the bundled faces and
those registered on it, so measured geometry and painted glyphs do not depend
on the host; carets and selection read the same layout. Only the app's
collection is fed from the host's fonts (`HostFonts::scan`), and no test
bootstrap builds that one, so there is no font state to pin. A test that
wants a host's faces builds its own collection with
`FontCollection::with_host_fonts(&HostFonts::scan())` and accepts that its
geometry depends on the machine.

To *look* at what a tree renders without a window, capture it:

```bash
cargo run -p flui --example screenshot --features "material cupertino" -- material 900 760 out.png
```

## Live E2E smoke

```bash
cargo xtask live-smoke             # X11 under Xvfb: real window, real XTEST input, real pixels
cargo xtask live-smoke --wayland   # headless weston: the close-path teardown ordering
```

`tools/live-smoke` is the only executing coverage of the band **above**
synthetic event dispatch — platform translation, the event-loop wake chain,
window-close teardown — each of which has shipped broken while every synthetic
gesture test stayed green. It also verifies hidden-surface gating against a real
occlusion signal. Both variants run in CI.

For driving a real app by hand, [`tools/desktop-mcp`](../tools/desktop-mcp/README.md) is an MCP
server an agent uses to list and capture windows, read the Windows UI Automation tree, invoke
element actions and send real input, on any application. Its ignored
`tests/live_windows.rs` drives `a11y_probe` through it; the scripted Windows gates are
`cargo xtask device windows-a11y` and `windows-input`.

`windows-a11y` invokes the counter and exercises native ExpandCollapse and
RangeValue patterns on the probe's generic semantics controls. It checks
disclosure state, repeated-action refusal, fractional and endpoint values,
unchanged published state after an out-of-range request, and a later valid
action. Independent visible state labels and an intervening counter action
observe the resulting frames. This does not establish native acceptance of the
catalog's `Disclosure` or `Slider`, IME, or screen-reader announcements.
`windows-input` separately sends pointer input followed by Tab and Enter.
Both checks require an interactive Windows desktop. Before the probe launches,
the shared UIA session asks Windows (`UOI_IO`) whether the desktop the check
runs on, which the probe inherits, is the one receiving input: a locked
workstation, a disconnected or service session, input going to another
desktop, a failed query, or an unavailable UI Automation exits 2 (CANNOT VERIFY) rather than reporting a FAIL or a passing run.

## CI Expectations

CI runs the same local gates plus repository-wide source checks. Every job is
gated on the fast `checks` job and aggregates into the single required `ci`
check; all cargo commands run `--locked`; actions are SHA-pinned and the
workflow files themselves are linted:

```bash
cargo xtask checks --strict                                   # fmt, taplo, typos, markdown links, workspace layers, module-dag, toolchain, wgsl, globals, changelog fragments, ...; a missing tool fails
cargo test -p xtask --locked                                  # xtask's own tests, lane classification included
actionlint                                                    # workflow semantics
zizmor .                                                      # workflow security audit
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo xtask feature-matrix --slice 1/3                        # feature-matrix job: cargo-hack per-feature clippy, shards 1/3..3/3 (--partition)
cargo xtask feature-matrix --slice combinations               # same job: each facade feature combination built in isolation
cargo check --workspace --locked --target wasm32-unknown-unknown --exclude ...                   # wasm-capable set — cargo xtask wasm-check
cargo clippy --workspace --lib --bins --locked --target wasm32-unknown-unknown --exclude ... -- -D warnings  # the only lint pass over the wasm32-only web backend — cargo xtask wasm-check
cargo test -p flui-foundation --locked --target wasm32-unknown-unknown --test wasm32              # the only step that EXECUTES wasm — cargo xtask wasm-test
cargo check -p flui-platform --locked --all-targets --target x86_64-pc-windows-msvc            # cross-typecheck job — cargo xtask cross-typecheck
cargo check -p flui-platform --locked --all-targets --target aarch64-apple-darwin              # (type-check only: no link, no tests)
cargo xtask deps --strict --only policy                       # deps job: cargo-deny bans / licenses / sources + cargo-shear
cargo xtask deps --strict --only advisories                   # same job: RustSec advisories, blocking in the wide, full and extended lanes
cargo bench -p flui-rendering --no-run                        # bench-compile job
cargo xtask doc-strict                                        # doc job
cargo xtask test --fast                                       # test job: nextest over the test scope, then flui-platform under Xvfb (FLUI_HEADLESS=1)
cargo xtask build-all-targets                                 # same job: links the examples and benches the test scope's features reach
cargo xtask test --nested --nested-group native               # test-nested: generated projects and facade consumers
cargo xtask test --nested --nested-group trybuild             # test-trybuild: compile-fail suites
cargo xtask platform-test                                    # native platform suites, default and all features
cargo xtask cli-test                                         # native CLI suite
cargo test --workspace --locked --doc
cargo +nightly miri test -p flui-rendering --lib pipeline::owner  # miri job; NARROW — every
                                                              # unit test under that module, including PipelineCell
                                                              # checkout, an owner-local run_frame traversal, a
                                                              # reentrant-layout walk, and two real-NodePtr walks
                                                              # driving layout_dirty_root through every reborrow phase
                                                              # of layout_subtree_borrowed_impl. Deeper sliver walks
                                                              # and intrinsics queries are not interpreted.
```

### CI jobs and their local commands

Each CI run takes one of five lanes. The `plan` job picks it
(`cargo xtask affected --event <event>`), and every lane
runs `checks`, `plan` and the `ci` aggregator:

| Lane | When | Adds |
|---|---|---|
| `docs` | a pull request that changes only documentation | nothing |
| `tooling` | a pull request that changes only repository tooling, or a standalone crate | `deps`; `standalone` for the crate |
| `wide` | a pull request that compiles anything | `deps` and the pull-request gate (`HEAVY_JOBS`: `clippy`, `cross-typecheck`, `test`, `test-nested`, `test-trybuild`, `doc`, `wasm-check`, `test-features`, `live-smoke`) over the whole workspace |
| `full` | a push to `main`, the merge queue | `wide` plus `FULL_JOBS`: `feature-matrix`, `miri`, `bench-compile`, `doc-test` (all Linux) |
| `extended` | the nightly schedule, `workflow_dispatch`, manual dispatch | `full` plus the same full Linux coverage (`EXTENDED_JOBS` is empty) |

- **Wide lane**: every pull request that compiles anything runs every Linux
  job, in parallel, over the whole workspace. One serial job over the
  changed crates and their dependents was slower (35 min median, against 18
  for these parallel jobs, over the runs of 2026-09-30), and hosted runners
  do not queue for this repository, so scoping bought nothing in CI. The
  scope still decides whether `deps` advisories block: they block when the change is
  workspace-wide (`mode` full: `Cargo.lock`, the root `Cargo.toml`,
  `.cargo/`, the toolchain file, a workflow, the lane's own code in
  `tools/xtask/src/change_scope/`, a file no crate owns) or touches an
  input of the wide lane's jobs (`deny.toml`, a WGSL shader); otherwise a new advisory is reported without failing
  unrelated work, and main turns red for it.

  `cargo xtask check-changed` keeps the scope before a PR: clippy and
  nextest over the changed crates and every workspace crate that declares a
  dependency on them (optional, dev and target-specific dependencies
  included; normal and build edges transitively, a dev edge as the last
  hop), and the checks a Linux build never compiles: the other targets'
  clippy for `flui-platform`'s backends, the Android runner and `flui-cli`
  on Windows, wasm32 clippy, a per-feature `cargo hack clippy` for the
  changed crates with features, changed manifests and dependents whose edge
  only a feature compiles, rustdoc with `-D warnings`, and the doctests. It
  counts uncommitted work.
- **Tooling lane**: nothing in the workspace compiles. A standalone crate is
  a directory under the repository whose `Cargo.toml` declares its own
  `[workspace]` and that no workspace crate reaches by a path dependency
  (the repository has none today); the `standalone` job runs
  `cargo check --locked --all-targets` on each one the change touches.
- **Full and extended lanes**: main, merge queue, nightly and manual runs
  add the remaining Linux checks. Labels do not change the lane.

  A red run on main or nightly opens (or comments on) the "CI is red on main"
  issue. The rule is fix forward within the hour, or revert.

**Only `full` and `extended` check these**, so a pull request can merge green
and still turn main red: the per-feature matrix (`feature-matrix`), miri,
bench linking (`bench-compile`) and the doctests (`doc-test`) — over 171 runs
of 2026-09-26..30 none of them failed where `clippy` and `test` passed, so
they run after the merge and a break is fixed forward. Native Windows/macOS and GPU runtime checks are local commands;
there are no jobs or labels enabling them in CI.

The `ci` aggregator recomputes which jobs the lane runs from `plan`'s `lane`
and its lists `HEAVY_JOBS`, `FULL_JOBS` and `EXTENDED_JOBS`. It fails on any
other skip, and on a job that ran where it should have skipped.

Locally, `cargo xtask check-changed` is the pre-PR check. `cargo xtask ci` is
the full local gate and is optional: CI is the proof. `cargo xtask ci-full`
mirrors the wide and full lanes' jobs this host can run, and `cargo xtask doctor full` names
what it needs. One row per job in `.github/workflows/ci.yml`:

| CI job | Local command | Difference, or why CI-only |
|---|---|---|
| `checks` | `cargo xtask checks` + `cargo test -p xtask`, then `actionlint` and `zizmor .` (`ci-full` runs both, skipping one that is not installed) | CI passes `--strict`: a missing typos, taplo or lychee fails there instead of being skipped with a message |
| `plan` | `cargo xtask affected` (`check-changed` runs the same classification) | decides the lane and the affected packages; CI passes `--event` and the PR's base SHA, `check-changed` diffs against `origin/main` (or `--base`) and adds uncommitted files |
| `standalone` | `cargo check --locked --all-targets --manifest-path <crate>/Cargo.toml` | tooling lane only, for each standalone crate the change touches; warnings are not denied (the crate is outside the workspace lints) |
| `clippy` | `cargo xtask lint` (in `gate`) | — |
| `test` | `cargo xtask test --fast`, then `cargo xtask build-all-targets` | the same commands; the all-targets build uses the test scope's features, so it links the examples and benches without rebuilding the rest (a target whose `required-features` it leaves off is compiled by `feature-matrix`, not linked) (the facade's default feature set is linted by `feature-matrix`, not built here); the flui-platform leg needs `xvfb-run` (Linux) |
| `test-nested` | `cargo xtask test --nested --nested-group native` | generated projects and facade consumers, beside `test` and `test-trybuild` |
| `test-trybuild` | `cargo xtask test --nested --nested-group trybuild` | compile-fail suites; plain `--nested` still runs both groups |
| `test-features` | the job's `cargo nextest run` lines (`ci-full` runs them) | — |
| `live-smoke` | `cargo xtask live-smoke`, `cargo xtask live-smoke --wayland` | the job runs these two commands; Linux only (Xvfb, lavapipe, weston); `ci-full` runs them on Linux and says it skipped them elsewhere |
| `bench-compile` | `cargo xtask bench-compile` | — |
| `doc` | `cargo xtask doc-strict` (in `gate`) | — |
| `deps` | `cargo xtask deps` | CI runs `--only policy` and `--only advisories` as two steps, with `--strict` (a missing cargo-deny or cargo-shear fails instead of being skipped); every lane but `docs` runs it; the advisories step blocks on main, nightly and a pull request whose change is workspace-wide or touches `deny.toml`, and only reports on other pull requests, because a new RustSec entry can fail a commit that passed the day before |
| `doc-test` | `cargo test --workspace --locked --doc` (in `cargo xtask ci`) | — |
| `miri` | `cargo xtask miri` | nightly + miri |
| `feature-matrix` | `cargo xtask feature-matrix` (runs `facade-combos` too) | CI runs `--slice 1/3`, `2/3`, `3/3` and `combinations` in parallel; locally it is one run over the workspace |
| `wasm-check` | `cargo xtask wasm-check`, `cargo xtask wasm-link`, `cargo xtask wasm-test` | `wasm-test` needs the `wasm-bindgen-cli` version `Cargo.lock` pins (`cargo xtask doctor full` names it) |
| `cross-typecheck` | `cargo xtask cross-typecheck --target <triple>`, then the same command with `--all-features` (omit the filter for every target) | needs Rust target standard libraries and native C tools; see below |
| `ci` | — | CI only: the single check a ruleset would require. `cargo xtask ci-verify` verifies that every gated job ran and passed, and that the jobs which skipped are exactly those the lane skips |
| `notify-main-red` | — | CI only: opens or updates the "CI is red on main" issue after a red run on main or nightly |

The other workflows (`weekly.yml`, `release.yml`, `docs.yml`) are scheduled or
event-driven, not per-PR gates. Release workflows build platform artifacts;
they do not run native validation jobs. The native runtime and GPU suites
remain available locally through `cargo xtask platform-test`, `cli-test`,
`gpu-test` and `device`. No `full-ci` label or hosted device-check workflow
can enable them.

A scheduled `weekly.yml` workflow (Mondays, or manually via
`workflow_dispatch`) builds and tests against a fresh `cargo update` — early
warning for upstream semver breakage. It is not a merge gate. New RustSec
advisories need no weekly run: the nightly run's `deps` job checks them
every day and blocks on them.

A change cannot be merged if any of these fail. If you encounter a flaky test, file a fix issue rather than retrying CI.

### Runtime and native compilation coverage

CI runtime validation runs on Linux, including nightly and manual dispatch.
The Windows/macOS runtime and hosted GPU jobs, the native device-check workflow and
the full-ci label workflow are removed. `cargo xtask affected` accepts no
full-ci label flag, and plan does not read PR labels. Native runtime checks
remain local commands. `cargo xtask gpu-test` sets `FLUI_REQUIRE_GPU=1` for
both the engine and facade readback suites; a missing adapter or device fails
the gate instead of producing successful skips. Native compilation uses Linux
cross tools for Windows and Android, and macOS runners with Xcode's genuine
SDKs for macOS and iOS. These legs run clippy, not native tests or applications;
OS runtime regressions need local platform evidence.

The aggregator requires each lane's planned jobs to pass and every other job
to skip. The two nested jobs restore the existing test cache without writing
additional entries.

`cross-typecheck` discovers native cfg predicates in the module trees of every
workspace Cargo target and in target-specific dependency declarations. It
selects packages directly with `--all-targets`, including their tests,
examples and benches; indirect dependency compilation does not cover those
targets. Required target features and declared `testing`/`a11y` features are
enabled together. `--all-features` additionally compiles every feature of the
selected packages. CI runs both configurations: all-features alone can hide
disabled-feature branches. This is not an exhaustive optional-feature matrix or a
macro expansion: generated `include!` output needs an explicit target-specific
manifest dependency to participate in discovery.

On a Windows host, Windows checks use the installed MSVC tools. Other hosts
use `cargo-xwin` (CI pins 0.23.1) with clang/LLVM and its MSVC SDK/CRT
provisioning. On a macOS host, Apple checks use Xcode; macOS checks elsewhere
use `cargo-zigbuild clippy` (validated locally with 0.23.4) and Zig (0.17.0)
for the required-target configuration. iOS needs a
genuine Apple SDK: generic Darwin libc headers are insufficient for native
logging dependencies. Android needs the NDK's target compiler and archiver,
for example `CC_aarch64_linux_android` set to its API-21 clang wrapper and
`AR_aarch64_linux_android` to `llvm-ar`. CI uses its bundled NDK. Explicit
target, `TARGET_CC`/`TARGET_AR`, and global compiler settings are preserved.
`doctor full` checks that the selected Android compiler can parse the NDK's
API-level header for Android API 21 or later and that the archiver runs. A
host compiler's successful version output alone does not establish NDK support.

The macOS `flui-log/apple-unified-logging` configuration also needs genuine
Apple SDK logging headers. All-features Apple checks therefore use Xcode SDK
hosts in CI; generic Zig libc headers do not validate this configuration.
Local all-features macOS checks are explicitly skipped on non-macOS hosts.

`check-changed` runs the required-target configuration for installed native
targets. Optional native coverage requires the explicit `--all-features`
command or CI; a local default check is not evidence for those paths.

## See Also

- [Getting Started](getting-started.md) — toolchain setup and first build
- [Contributing](../CONTRIBUTING.md) — planning a change, git hygiene, bug reports
- [`AGENTS.md`](../AGENTS.md) — current performance and testing requirements
