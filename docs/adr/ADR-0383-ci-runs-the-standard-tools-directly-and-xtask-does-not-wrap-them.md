# ADR-0383: CI runs the standard tools directly, and xtask does not wrap them

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Supersedes:** [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  §1 only for the command lines it lists as xtask's orchestration (`test`, `clippy`, `miri`,
  `feature-matrix`, the `wasm-check` step) and for those of the same kind it does not list
  (`deps`, `doc-strict`, `gpu-test`, `platform-test`, `wayland-optional`, `native-features`,
  `build-all-targets`, `facade-combos`, and the local mirrors `lint`, `gate`, `ci`, `ci-full`): a
  workflow step calls the tool.
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

CI's `clippy`, `test`, `test-nested`, `test-trybuild`, `doc`, `deps` and `feature-matrix` jobs,
the check and clippy step of `wasm-check`, `advisories.yml` and the `desktop` legs of
`nightly-platforms.yml` call `cargo nextest`, `cargo clippy`, `cargo hack`, `cargo doc` and
`cargo deny` with their arguments in the step. A value two steps share (the test features) is a
job-level `env`; a list the workspace already states (the packages with a `testing` feature, those
with `wasm = false`) is read from `cargo metadata` in the step. A step that repeats over a short
list (the facade's feature combinations, flui-native's features, its two feature sets on Windows
and macOS) is a shell loop that runs every iteration before it fails.

### §2. The subcommands are removed

`lint`, `gate`, `test`, `platform-test`, `cli-test`, `wayland-optional`, `build-all-targets`, `ci`,
`ci-full`, `deps`, `native-features`, `gpu-test`, `miri`, `doc-strict`, `feature-matrix`,
`facade-combos` and `wasm-check` leave `tools/xtask`.
There is no local command that mirrors CI: `docs/testing.md` names, for each job, the commands a
contributor runs, and the job holds them.

### §3. The trybuild shards are listed in the workflow

`test-trybuild`'s matrix lists each shard's test targets and binaries. Together they are the
`trybuild` group's filter in `.config/nextest.toml`, which a suite joins first; its comment says a
suite added there goes into one shard. The same comment says `test-nested` builds only the
packages the `nested-cargo` filter names, so a test of another package joins the job's `-p`
list. cargo-deny's advisories read the RustSec database
cargo-audit reads, so cargo-audit is not added beside them.

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

### Confirmation

- `actionlint` (in `checks`) parses the workflows and runs shellcheck over their steps.
- Each job runs its commands on every run of its lane;
  `git grep -E 'cargo xtask (test|deps|doc-strict|gpu-test|feature-matrix|facade-combos|wasm-check)\b'`
  finds none in `.github/`.

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
