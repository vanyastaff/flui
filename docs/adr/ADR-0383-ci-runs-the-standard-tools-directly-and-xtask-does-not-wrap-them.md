# ADR-0383: CI runs the standard tools directly, and xtask does not wrap them

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  §1 only for the command lines it lists as xtask's orchestration (`test`, `clippy`,
  `cross-typecheck`, `miri`, `feature-matrix`, the `wasm-check` step, `wasm-link`, `wasm-test`)
  and for those of the same
  kind it does not list (`deps`, `doc-strict`, `gpu-test`, `platform-test`, `wayland-optional`,
  `native-features`, `build-all-targets`, `facade-combos`, `wasm-imports`, `wasm-test-crates`,
  `locked-version`, and the local mirrors `lint`, `gate`, `ci`, `ci-full`): a workflow step calls
  the tool. The same §1 for `perf`, which it lists among the tools for working in the repository:
  the scenarios hold their own baseline, and the other comparisons are steps (§7).
  [ADR-0371](ADR-0371-before-the-first-release-no-xtask-gate-holds-a-rule-about-the-sources.md)
  §3 only for the formatters, the linters and the link check it keeps in `cargo xtask checks`:
  `cargo fmt`, typos, taplo and lychee are steps of the `checks` job (§4).
- **Revised:** 2026-11-19 — §5 and the wasm names in Supersedes: `wasm-check` links the web demos,
  runs wasm-bindgen over them and reads their import modules with node, and runs the wasm32 tests
  itself; §6 and `cross-typecheck` in Supersedes: each native leg lints the whole workspace; §7 and
  `perf` in Supersedes: the perf scenarios hold their baseline, and `perf-gates`, `size` and `deps`
  compare the rest in steps.
- **Related:** [ADR-0371](ADR-0371-before-the-first-release-no-xtask-gate-holds-a-rule-about-the-sources.md),
  [ADR-0375](ADR-0375-before-the-first-release-a-pull-request-runs-a-fixed-gate-and-no-api-contract-gate.md)
  and [ADR-0376](ADR-0376-ci-decides-its-lane-in-the-workflow-and-a-pull-request-names-no-expectations.md)
  (the same reduction of `tools/xtask` before the first release).

## Context and problem statement

ADR-0303 §1 kept in `tools/xtask` the command lines that must match between a contributor and CI:
CI's `test` job ran `cargo xtask test --fast`, `deps` ran `cargo xtask deps`, `doc` ran
`cargo xtask doc-strict`, the GPU and Wayland steps and the nightly desktop legs ran their own
subcommands, and `cargo xtask ci` ran them all locally. Each subcommand is a few `cargo nextest`,
`cargo deny`, `cargo doc` or `cargo clippy` lines with fixed arguments, behind about 1,900 lines of
Rust and tests a reader has to open to learn what a job runs.

The owner wants `tools/xtask` gone before the first release and CI made of the tools every Rust
project runs. Where do these command lines live?

## Decision drivers

- A reader of a failed job sees the command that failed in the workflow.
- One source per fact: a command line written in two places drifts.
- Before the first release one owner runs CI, and a contributor runs the job they need.

## Considered options

1. Keep the subcommands, CI and a contributor calling the same wrapper.
2. Write each command in its workflow step; a contributor copies the job's lines; delete the
   subcommands.
3. Move the subcommands to a task runner (`mise`, `just`) that both call.

## Decision

Chosen option: 2, because the workflow is the one place that has to hold the command anyway, and
a task runner would be a second tool to install for what a step already says.

### §1. A workflow step calls the tool

CI's `clippy`, `test`, `test-nested`, `test-trybuild`, `doc`, `deps`, `feature-matrix`,
`wasm-check`, `cross-typecheck`, `perf-gates` and `size` jobs, `advisories.yml` and the `desktop`
legs of `nightly-platforms.yml` call `cargo nextest`, `cargo test`, `cargo build`, `cargo clippy`,
`cargo hack`, `cargo doc`, `cargo deny`, `cargo tree` and `valgrind` with their arguments in the
step. A value two steps share (the test features) is a
job-level `env`; a list the workspace already states (the packages with a `testing` feature, those
with `wasm = false`) is read from `cargo metadata` in the step. A step that repeats over a short
list (the facade's feature combinations, flui-native's features, its two feature sets on Windows
and macOS) is a shell loop that runs every iteration before it fails.

### §2. The subcommands are removed

`lint`, `gate`, `test`, `platform-test`, `cli-test`, `wayland-optional`, `build-all-targets`, `ci`,
`ci-full`, `deps`, `native-features`, `gpu-test`, `miri`, `doc-strict`, `feature-matrix`,
`facade-combos`, `wasm-check`, `docs-links`, `wasm-link`, `wasm-test`, `wasm-imports`,
`wasm-test-crates`, `locked-version`, `cross-typecheck` and `perf` leave `tools/xtask`.
There is no local command that mirrors CI: `docs/testing.md` names, for each job, the commands a
contributor runs, and the job holds them.

### §3. The trybuild shards are listed in the workflow

`test-trybuild`'s matrix lists each shard's test targets and binaries. Together they are the
`trybuild` group's filter in `.config/nextest.toml`, which a suite joins first; its comment says a
suite added there goes into one shard. The same comment says `test-nested` builds only the
packages the `nested-cargo` filter names, so a test of another package joins the job's `-p`
list. cargo-deny's advisories read the RustSec database
cargo-audit reads, so cargo-audit is not added beside them.

### §4. The `checks` job calls the formatters and the link check

`cargo fmt --all --check`, typos, `taplo fmt --check` and lychee are steps of CI's `checks` job,
each running even when one before it failed. `cargo xtask checks` keeps only the checks no standard
tool makes: the toolchain pin, unmounted test files, ADR numbers, the changelog fragments and the
generated files against their sources. lychee's command line in the job takes the options
`docs-links` passed it; the extra pass that caught a link climbing above the repository root goes
with `docs-links`, so such a link is the reader's to catch.

### §5. `wasm-check` links the web demos and runs the wasm32 tests itself

The job builds `flui-web-demo` and `flui-painting-demo` for wasm32, so the demos link; rustc
stopped passing `--allow-undefined` to the wasm linker in 1.96 (rust-lang/rust#149868), so an
undefined symbol fails that link where it used to become an `env` import. What still becomes an
import is an extern that names its module (`#[link(wasm_import_module = …)]`), and no page
provides one wasm-bindgen does not generate. The check below reads the code the two demos link
(the workspace crates they use and those crates' dependencies), as `wasm-imports` did; code no demo
links, such as `flui-app`'s web runner, is outside it. Four options were weighed for that class:

- *An allowlist read by `wasm-tools`* (what `wasm-imports` did with `wasmparser`): a tool version
  beside the toolchain, an allowlist file and a scrape of a printer's output.
- *Load the demos* in the job's headless Chrome: finds every load-time failure, but the demos start
  a renderer when instantiated, which needs a page, a canvas and a browser GPU path.
- *Rely on the linker*: nothing to keep, but an import module a dependency adds passes until a
  demo is loaded.
- *Read the imports with node, after wasm-bindgen* (chosen): the job runs `wasm-bindgen --target
  web` over each demo, which fails on a binding it cannot generate as a web build would, and node
  compiles the linked module and lists its import modules with `WebAssembly.Module.imports`,
  failing on any but wasm-bindgen's own two (`__wbindgen_placeholder__`,
  `__wbindgen_externref_xform__`), named in the step. It needs only the CLI the tests install and
  the node they run on, compares names rather than text, and instantiates nothing.

The job then runs `cargo test --target wasm32-unknown-unknown`, through the runner
`.cargo/config.toml` names, for each workspace package whose dev-dependencies on wasm32 include
`wasm-bindgen-test`, as cargo's resolver reports them (`cargo metadata --filter-platform
wasm32-unknown-unknown` keeps a dependency only when one of its target tables is active on wasm32,
though it then lists every kind of that edge): its lib test, and its
`wasm32` test target when it has one, each run and counted on its own, and flui-native's
`web_browser` target, which the loop names. A test runs on node, or in the job's headless Chrome
for Testing, at the exact release the step names, with its matching ChromeDriver, when its crate
configures `run_in_browser`. A target whose run passes no assertion fails the step, except a lib
target beside a `wasm32` target, which may hold none; an empty list fails it too. The
`wasm-bindgen-cli` the job installs is the one version of
`wasm-bindgen` that `cargo metadata --all-features` lists, which is every package Cargo.lock holds;
more than one fails the step, and `cargo xtask doctor full` fails its row on the same lock.

### §6. `cross-typecheck` lints the whole workspace for four native triples

Each leg of the job (`x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `aarch64-linux-android`,
`aarch64-apple-ios`, the triples `cross-typecheck` checked) runs `cargo clippy --workspace
--all-targets --locked --target <triple> -- -D warnings`, the `clippy` job's command, and then the
same with `--all-features`, which reaches the targets behind `required-features` that plain
`--all-targets` skips. The Windows leg runs on Linux through `cargo xwin clippy`, which the matrix
names as the leg's subcommand. The options for which packages a leg lints, judged by the drivers
(the command in the job, one source per fact) and by the runner time an owner's quota pays:

- *Discover them from the sources* (what `cross-typecheck` did): xtask parsed every crate's module
  tree with `syn` and kept a package whose code or manifest names a native `cfg`. The selection is
  in Rust, not in the job; about 900 lines and their tests for it; and a `cfg` it did not parse (a
  macro's output, an `include!`) dropped its package silently.
- *Name the packages in the matrix*: the cheapest run, and the command is in the job, but the list
  is a second source beside the crates' `cfg`s, and a crate that gains a native `cfg` is
  unchecked until someone adds it, with nothing to say it is missing.
- *The whole workspace minus a list read from metadata*, as `wasm-check` reads `wasm = false`: a
  package marked as never built for a triple is left out, and nothing else needs keeping. No
  package is so marked for a native triple today, so the list would be empty.
- *Lint the whole workspace* (chosen): the command is the job's, there is no list to keep, and a
  crate whose code has no native `cfg` is still built against its dependencies' native forms. Each
  leg compiles the dozen packages the discovery left out (the design-system packages, the demos,
  the protocol and layer crates). When a package that never ships on a triple cannot build for
  it, the metadata list above is the next step, not a list in the job.

`cargo check` in place of clippy would save the lint pass, but a lint behind a native `cfg` is seen
only on that native target, which no other job builds. Neither run is an application's build:
`--all-targets` compiles the tests in the same invocation, so the features a crate's own
dev-dependency on itself names (flui-native's `testing`, `http` and `single-instance`) are on in
its library in both. The required-features run the discovery built had the same property, so
dropping it loses only the configuration with `testing` and required features on and the other
optional features off.

### §7. The perf scenarios hold their baseline, and the size and callgrind checks are steps

`cargo xtask perf` compared four things with checked-in numbers: the perf scenarios' counts with
`crates/flui-widgets/perf/baseline/`, the `perf_scaling_` scenarios' instructions on a small and
a large tree, the facade's default graph and the release sizes of generated projects with
`perf/size.toml`. The options for where each comparison lives, judged by the drivers and by what
a rule loses when it leaves the Rust that tested it:

- *Keep `perf`*: one command for a contributor and CI, but about 2,000 lines a reader opens to
  learn what three jobs check, which is what this record removes elsewhere.
- *Every comparison as a step*: the command is in the job, but the counts' rules (a work counter
  matches, a cost counter only stays at or under, a counter on one side only fails, a file no
  scenario holds fails) would be shell over twelve files with no test, and a bless would be lines
  a contributor copies.
- *The counts in the scenarios, the rest as steps* (chosen): the counts' rules live beside the
  scenarios that produce them, in the perf target's `baseline` module, with its own tests
  (`the_comparison_passes_only_an_equal_counter_or_a_cost_that_fell`,
  `every_baseline_file_is_a_scenario`). `FLUI_PERF_BASELINE=check` holds each scenario's counts
  to its file before the scenario asserts its budget, and `bless` rewrites the file; unset, the
  scenarios run as before on every host and feature set. `perf-gates` sets `check` and runs
  `-p flui-widgets` alone, with its default features, where the files are blessed. Each of the
  other three is one tool's output compared with one number: `valgrind --tool=callgrind` on each
  size, `cargo tree` piped through `sort -u`, and the bytes of the binary `cargo build` reports,
  each in its job's step.
- *A callgrind harness crate* (gungraun, formerly iai-callgrind) for the instructions: it compares
  a benchmark with its own earlier run, not one tree size with another, and adds a dependency and
  a bench harness for what one step's loop does.

The `size` job also compared each `main` push's binaries with the commit before it, from a cache
entry each push saved, so that growth inside the 2% tolerance was answered at the merge that
brought it. Kept, it needs a measurement file, a cache restore and save, a check that the file is
the parent's and a rule for a baseline restated in between, in shell with no test. It is dropped:
a binary fails past 2% over its line, as before, and the job summary lists each size beside its
line for the pull request that raises it.

### Consequences

- Good, because what a job runs is in the job, and `tools/xtask` loses the subcommands and their
  tests.
- Bad, because a contributor who wants CI's run locally copies several lines instead of one
  command, and the copy in `docs/testing.md` can fall behind the workflow.
- Bad, because a trybuild suite added to the group's filter but to no shard runs nowhere, which
  `trybuild_shards_partition_the_group` used to catch: the comment beside the filter is the rule,
  and the reader of the change holds it.
- Bad, because the test that pinned the `nested-cargo` group's packages to the job's `-p` list is
  gone with the code it read.
- Bad, because §5's rules lose their tests with the code they read, and the reader of a change to
  the `wasm-check` job holds them (AGENTS.md "Rules a reader holds"): the discovery (a normal
  dependency, or a dev-dependency only a non-wasm32 target table declares, does not opt a package
  in) and the count of passed assertions (which lines of the runner's output count). The job runs
  only on main, so such a change is run on its branch at the head that merges (Confirmation).
- Bad, because the import check's own tests go with `wasm-imports` (a malformed module, compact
  import groups, the committed allowlist refusing `env`): node's WebAssembly compiler parses the
  module, and the two allowed names live in the step, which a wasm-bindgen that renames them fails.
- Bad, because the Chrome release is one more version to bump by hand, and the browser tests run
  against it, not against the release users have, until someone does.
- Bad, because each `cross-typecheck` leg compiles the packages §6 adds, on runners an owner's
  quota pays for, the two macOS-hosted legs among them.
- Bad, because a native branch behind `testing` that only builds with some other optional
  feature off fails no leg, where the required-features run caught it (§6).
- Bad, because the tests of the discovery go with it (`native_typecheck_selects_target_gated_workspace_targets`
  and the module-tree tests), and the job's matrix and its two lines are the whole statement of
  the scope: the reader of a change to them holds it (Confirmation).
- Bad, as before this record, because no job compiles a native arm that exists only off these four
  triples (x86_64 macOS, 32-bit Android) or only with a self-dev-dependency feature off; `docs/gaps.md`
  lists them.
- Bad, because growth inside `size`'s 2% passes until the merge that crosses it, whose author then
  answers for the growth earlier merges brought (§7).
- Bad, because the callgrind, package-count and binary-size comparisons lose their tests with
  `perf` (the tolerance arithmetic, the parsers of callgrind's and libtest's output, the distinct
  package count): the reader of a change to those steps holds them, and the steps run only on
  main, except the package count, which `deps` runs on every pull request.
- Bad, because there is no local command for a size check: `perf/size.toml` is edited by hand from
  the job's log, and its counts are Linux x86_64's.

### Confirmation

- `actionlint` (in `checks`) parses the workflows and runs shellcheck over their steps.
- Each job runs its commands on every run of its lane;
  `git grep -E 'cargo xtask (test|deps|doc-strict|gpu-test|feature-matrix|facade-combos|wasm-[a-z-]+|locked-version|cross-typecheck|perf)\b'`
  finds none in `.github/`.
- flui-widgets' perf target tests the counts' rules on every pull request
  (`the_comparison_passes_only_an_equal_counter_or_a_cost_that_fell`,
  `a_blessed_file_parses_back_and_a_malformed_one_does_not`, `every_baseline_file_is_a_scenario`),
  and `perf-gates` holds the files on every main run.
- `cross-typecheck` runs its four legs on every main run. Its scope (`--workspace`, `--all-targets`,
  the two feature sets, the four triples) is read in the diff of a change to the job, against §6.
- A pull request that changes the steps of a job only main runs (`wasm-check`,
  `cross-typecheck`, `perf-gates` and `size` among them) runs
  that job on its branch at the head that merges: `gh workflow run CI --ref <branch>` takes the
  full lane, and the pull request's body links the run. AGENTS.md "Before each push" item 14 asks
  the author for it; the readers of "Before a merge" read git, not the body, so the author holds
  it.
- `cargo test -p xtask` holds doctor's wasm-bindgen row to one locked version
  (`the_wasm_bindgen_row_needs_one_locked_version_installed`, `this_lock_pins_one_wasm_bindgen`).

## Pros and cons of the options

### 1. Keep the wrappers

- Good, because one command reproduces a job locally.
- Bad, because a job's log shows `cargo xtask test --fast`, and the command behind it is in Rust.

### 2. Commands in the workflow (chosen)

- Good, because the workflow is the one source, readable without building anything.
- Bad, because of the Consequences above.

### 3. A task runner

- Good, because one command again reproduces a job locally.
- Bad, because a contributor installs another tool, and the workflow either calls it (a second
  wrapper) or repeats it.
