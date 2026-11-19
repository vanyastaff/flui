# ADR-0383: CI runs the standard tools directly, and xtask does not wrap them

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  §1 only for the command lines it lists as xtask's orchestration (`test`, `clippy`, `miri`,
  `feature-matrix`, the `wasm-check` step, `wasm-link`, `wasm-test`) and for those of the same
  kind it does not list (`deps`, `doc-strict`, `gpu-test`, `platform-test`, `wayland-optional`,
  `native-features`, `build-all-targets`, `facade-combos`, `wasm-imports`, `wasm-test-crates`,
  `locked-version`, and the local mirrors `lint`, `gate`, `ci`, `ci-full`): a workflow step calls
  the tool.
  [ADR-0371](ADR-0371-before-the-first-release-no-xtask-gate-holds-a-rule-about-the-sources.md)
  §3 only for the formatters, the linters and the link check it keeps in `cargo xtask checks`:
  `cargo fmt`, typos, taplo and lychee are steps of the `checks` job (§4).
- **Revised:** 2026-11-19 — §5 and the wasm names in Supersedes: `wasm-check` links the web demos
  and runs the wasm32 tests itself, and the import allowlist goes, since the linker now refuses an
  undefined symbol.
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

CI's `clippy`, `test`, `test-nested`, `test-trybuild`, `doc`, `deps`, `feature-matrix` and
`wasm-check` jobs, `advisories.yml` and the `desktop` legs of `nightly-platforms.yml` call
`cargo nextest`, `cargo test`, `cargo build`, `cargo clippy`, `cargo hack`, `cargo doc` and
`cargo deny` with their arguments in the step. A value two steps share (the test features) is a
job-level `env`; a list the workspace already states (the packages with a `testing` feature, those
with `wasm = false`) is read from `cargo metadata` in the step. A step that repeats over a short
list (the facade's feature combinations, flui-native's features, its two feature sets on Windows
and macOS) is a shell loop that runs every iteration before it fails.

### §2. The subcommands are removed

`lint`, `gate`, `test`, `platform-test`, `cli-test`, `wayland-optional`, `build-all-targets`, `ci`,
`ci-full`, `deps`, `native-features`, `gpu-test`, `miri`, `doc-strict`, `feature-matrix`,
`facade-combos`, `wasm-check`, `docs-links`, `wasm-link`, `wasm-test`, `wasm-imports`,
`wasm-test-crates` and `locked-version` leave `tools/xtask`.
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

The job builds `flui-web-demo` and `flui-painting-demo` for wasm32, so the demos link. Their
imports are not checked against a list. `wasm-imports` compared them with a committed allowlist
because an undefined symbol used to link as an import (`env` the catch-all for an accidental
native extern); rustc stopped passing `--allow-undefined` to the wasm linker in 1.96
(rust-lang/rust#149868), so on the pinned toolchain that symbol fails the link. What still becomes
an import is an extern that names its module (`#[link(wasm_import_module = …)]`), in the
workspace or in a dependency. Three options were weighed for that class:

1. Keep the list: install `wasm-tools` at a version the pinned `install-action` serves with a
   checksum, read each demo's imports from the text `wasm-tools print` writes, and fail on a module
   the allowlist lacks. Good, because a dependency bump that adds an import module is refused.
   Bad, because the job keeps a tool version, an allowlist and a scrape of a printer's output,
   checked only on main, for a class whose only members today are wasm-bindgen's own two modules
   (`__wbindgen_placeholder__`, `__wbindgen_externref_xform__`), which `wasm-bindgen-cli` resolves.
2. Load the demos: run `wasm-bindgen` over each and instantiate it in the job's headless Chrome.
   Good, because it finds every load-time failure, an unknown import module among them. Bad,
   because the demos start a renderer when instantiated, so it needs a page, a canvas and a
   browser GPU path the job does not have, which is a harness of its own.
3. Rely on the linker (chosen). Good, because the accidental class fails the link with nothing the
   job keeps for it. Bad, because an import module a dependency adds passes until the demo is
   loaded in a browser (Consequences).

The job then runs `cargo test --target wasm32-unknown-unknown`, through the runner
`.cargo/config.toml` names, for each workspace package whose dev-dependencies on wasm32 include
`wasm-bindgen-test`, as cargo's resolver reports them (`cargo metadata --filter-platform
wasm32-unknown-unknown`, which evaluates each target table's `cfg`): its lib test, and its
`wasm32` test target when it has one. A test runs on node, or in the job's headless Chrome for
Testing, at the exact release the step names, with its matching ChromeDriver, when its crate
configures `run_in_browser`. A package whose run passes no assertion fails the step,
and so does an empty list. The `wasm-bindgen-cli` the job installs is the one version of
`wasm-bindgen` that `cargo metadata --all-features` lists, which is every package Cargo.lock holds;
more than one fails the step, and `cargo xtask doctor full` fails its row on the same lock.

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
  the `wasm-check` job holds them: the discovery (a normal dependency, or a dev-dependency only a
  non-wasm32 target table declares, does not opt a package in) and the count of passed assertions
  (which lines of the runner's output count). The job runs only on main, so such a change is run
  on its branch at the head that merges (Confirmation).
- Bad, because an import from a module the extern names is no longer refused by a list: in the
  workspace it is a rule a reader holds (AGENTS.md), and one a dependency bump brings in is found
  when the demo is loaded, since nothing in CI instantiates the demos.
- Bad, because the Chrome release is one more version to bump by hand.

### Confirmation

- `actionlint` (in `checks`) parses the workflows and runs shellcheck over their steps.
- Each job runs its commands on every run of its lane;
  `git grep -E 'cargo xtask (test|deps|doc-strict|gpu-test|feature-matrix|facade-combos|wasm-[a-z-]+|locked-version)\b'`
  finds none in `.github/`.
- A pull request that changes the steps of a job only main runs (`wasm-check` among them) runs
  that job on its branch at the head that merges: `gh workflow run CI --ref <branch>` takes the
  full lane, and the pull request's body links the run. AGENTS.md "Before each push" item 14 asks
  for it, so the readers of "Before a merge" check it.
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
