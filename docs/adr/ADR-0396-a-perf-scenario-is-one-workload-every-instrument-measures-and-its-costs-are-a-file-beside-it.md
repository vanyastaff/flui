# ADR-0396: A perf scenario is one workload every instrument measures, and its costs are a file beside it

- **Status:** Accepted (2026-11-23)
- **Date:** 2026-11-23
- **Supersedes:** [ADR-0383](ADR-0383-ci-runs-the-standard-tools-directly-and-xtask-does-not-wrap-them.md)
  §7 only for the perf scenarios' counts (the holder in `crates/flui-widgets/tests/perf/baseline.rs`,
  checked by `perf-gates` on `-p flui-widgets` alone, beside the catalogs' own holder) and for the
  `perf_scaling_` instructions counted by a shell loop in that job; its `size` and `deps`
  comparisons stay as they are.
  [ADR-0375](ADR-0375-before-the-first-release-a-pull-request-runs-a-fixed-gate-and-no-api-contract-gate.md)
  only for its consequence that a perf count a pull request moves fails `main`: it fails the pull
  request's `test` job, inside the fixed gate.
- **Related:** [ADR-0322](ADR-0322-a-control-mounts-one-element-for-its-behaviours.md) (the control
  costs the catalogs' scenarios hold); [ADR-0331](ADR-0331-a-pull-request-names-every-expectation-it-edits.md)
  (a moved baseline is an expectation the pull request names);
  [ADR-0375](ADR-0375-before-the-first-release-a-pull-request-runs-a-fixed-gate-and-no-api-contract-gate.md)
  (the fixed pull-request gate the counts join, inside `test`);
  [ADR-0243](ADR-0243-one-testing-kit-per-layer.md) (`flui-testing` is the kit that holds the
  harness)
- **Sources:** `flutter/flutter@3.50.1` `packages/integration_test/lib/integration_test.dart:350`
  (`traceAction`), `packages/flutter_driver/lib/src/driver/timeline_summary.dart:51`
  (`TimelineSummary`), `dev/benchmarks/macrobenchmarks/`, `dev/benchmarks/microbenchmarks/README.md`
  (benchmark names are kept for their history); Chromium
  `docs/testing/rendering_representative_perf_tests.md` at tag `85.0.4178.4` (frame times against
  the 95th percentile of the last 30 runs); the rust-lang internals thread "What is
  perf.rust-lang.org measuring and why is instructions:u the default?" (instructions as the
  primary metric, wall time as a check)

## Context and problem statement

FLUI measures performance in several shapes that grew one at a time:

| What | Where | Held how |
|------|-------|----------|
| Frame work and heap per scenario | `crates/flui-widgets/tests/perf.rs`, files in `perf/baseline/` | its own holder, checked on `main` only, by `perf-gates` on `-p flui-widgets` |
| Control costs | the `perf` targets of `flui-material` and `flui-cupertino` | `flui_testing::control_cost::held_to_baseline`, a ceiling checked on every run |
| Allocations of one hot path | 13 tests below `flui-widgets`, each with its own `GlobalAlloc` | an assertion in the test |
| Instructions | a shell loop in `perf-gates` over `perf_scaling_` | the large tree within 5% of the small one; no absolute count |
| Wall time | 36 criterion targets in 14 crates | nothing: CI builds them and no run compares them |
| A whole application | `packages/flui-material/benches/app_scenarios.rs`, written for one audit | nothing |

Each new question has brought a new harness, and a hotspot audit brought a bench that is the
seventh. The question is how FLUI measures performance across the project, so that a cost
regression fails where it is made and an audit adds nothing that outlives it.

## Decision drivers

- A regression fails on the pull request that makes it, under the fixed gate (ADR-0375), with
  no new job.
- A gate holds a deterministic metric. Shared runners are noisy, so wall time cannot be a gate
  there.
- One workload, written once. A second description of the same steps for another tool drifts
  from the first.
- A change of cost is reviewed like a change of behaviour: it is a line of the diff, with the
  pull request's reason (ADR-0331).
- No external service in the gate's path, and no dependency or bench harness for what a test of
  the kit can do (ADR-0383).
- Testing and measuring tools are part of the platform: what measures FLUI is what an
  application author can use on a screen of their own.

## Considered options

1. Keep the per-target holders and add a harness for each new question.
2. A hosted benchmark service (CodSpeed) over criterion benches.
3. A callgrind harness crate (Gungraun) for instructions, beside the counts.
4. A history store and dashboard (a results branch, as Bevy's metrics), with a trend gate.
5. One workload every instrument measures, its costs in a file beside it, and a thin harness in
   `flui-testing`.

## Decision

Chosen option: 5, because it is the only one where a scenario is written once and every metric
that gates is a deterministic number in the diff.

### §1. A scenario is a test, and every instrument measures it

A perf scenario is a `#[test]` named `perf_*` in a crate's `perf` target. It mounts a tree on
the kit's host, settles it, and runs fixed steps inside one measured window. The instruments read
that window and nothing else:

- the work of each phase (`flui_testing::FrameReport`) and what the steps asked of the heap
  (`flui_testing::heap::HeapCounts`), on every run;
- instructions, by the kit running the scenario's own binary under callgrind with collection on
  inside the window only (§3);
- a profile: the allocation sites (`flui-testing`'s `dhat` feature) or the frame's spans, when
  a developer asks for one;
- frame times, from the scheduler's `FrameSnapshot`s, on the nightly platform runs (§4).

The window is `flui_testing::perf::measure`, which returns what the steps cost; a scenario never
reads a counter around its own code. Until `measure` exists, each target's `perf_measured`
function is the window.

### §2. Every gate metric is a checked-in file beside the scenario

`flui_testing::perf::Baselines` is the one holder: a target names its directory and its sorted
scenarios once, and a scenario holds its counts with `Baselines::hold` before it asserts its own
budget. The rules are the holder's, with its tests:

- a **work** counter (elements rebuilt, nodes laid out, layers produced) equals its file;
- a **cost** counter (a name with `alloc` or `bytes`) stays at or under its file, so a fall
  passes and a bless on `main` lowers the file;
- a counter on one side only fails, and a file no scenario holds fails
  (`Baselines::assert_files_are_scenarios`).

`FLUI_PERF_BASELINE=check` compares, `bless` rewrites a file when its scenario's body ends
without a panic, and unset compares nothing. CI's `test` job runs the workspace with
`FLUI_PERF_BASELINE=check` and its own features, which is where every file is blessed
(`flui_testing::perf::BLESS_COMMAND`), then fails on a file no scenario recorded into
`FLUI_PERF_OUT`. A baseline file a pull request edits is an expectation it names (ADR-0331).

### §3. Instructions are a file too, on `main`

The kit counts a scenario's instructions inside its window and holds them in the scenario's file
as `instructions`, a cost counter that may rise by at most 3% before it fails. The tolerance
absorbs a toolchain or dependency update; a file blessed on `main` follows each one, so a series
of small rises cannot pass unnoticed. The count needs valgrind, so `perf-gates` on `main` runs it.
A `perf_scaling_` scenario also holds its large tree within 5% of its small one. The 3% is
measured before it is set: five CI runs of one commit give the spread, and a spread over 1%
reopens this section.

### §4. Frame time is a trend, never a gate

The nightly platform runs record each app-level scenario's frame-time percentiles (50th, 90th,
99th) and warn when the 90th passes the 95th percentile of that platform's last 30 runs. Nothing
fails on a frame time.

### §5. Workloads are permanent and named at two levels

- **A layer.** Each crate with a hot path (`flui-rendering`, `flui-view`, `flui-widgets`, the
  catalogs) has its `perf_*` scenarios in its own `perf` target.
- **An application.** App-level scenarios drive the acceptance application
  (`tests/fixtures/acceptance_app/`), which every platform already builds and runs, with the
  pages a scenario needs (a long list, wrapped text, a form).

A scenario keeps its name, since its file's history is its name's. An audit runs these scenarios
under the profile instruments of §1. A one-off workload an audit needs stays on its branch and
does not reach `main`.

### §6. Wall time is for local comparison

A criterion bench answers a question about wall time on a quiet machine
(`--save-baseline` and `critcmp`). Each one names that question in its header; a bench whose
question a `perf_*` scenario answers becomes that scenario.

### §7. The harness is public in shape

`flui_testing::perf` is written as the API an application author measures a screen with. Its
names are stabilised once the framework's own scenarios have run on it.

### Consequences

- Good, because a regression in work or heap fails on the pull request that makes it, inside
  `test`, with the counter, the old and the new value.
- Good, because a scenario is written once, and an audit adds nothing to `main`.
- Good, because the history of a cost is `git log` of its file.
- Bad, because every pull request that moves a count edits a file and names it, which is the
  price of reviewing cost.
- Bad, because the files are blessed with one feature set on Linux: a count on another platform
  or feature set is not held.
- Bad, because a toolchain update can move instructions past 3% and needs a bless on `main`.
- Neutral, because frame time on devices is watched, not gated, as on every shared runner.

### Confirmation

- The holder's rules: the tests of `crates/flui-testing/src/perf/baseline.rs`
  (`the_comparison_passes_only_an_equal_counter_or_a_cost_that_fell`,
  `a_check_fails_on_a_difference_that_fails_and_on_a_missing_file`,
  `a_bless_writes_the_file_only_when_the_test_ends_without_a_panic`,
  `only_a_run_with_a_mode_records_its_counts`).
- Each target's `every_baseline_file_is_a_scenario`.
- CI's `test` job: `FLUI_PERF_BASELINE=check` on its nextest step, and the step after it that fails
  on a baseline file with no record.
- `let_underscore_drop` denied in each `perf` target, so a `Held` cannot be dropped before the
  scenario's assertions.

## Pros and cons of the options

### 1. Keep the per-target holders

- Good, because nothing changes.
- Bad, because two holders disagree on what a work counter is, the widgets' counts are checked on
  `main` only, and each new question brings a harness, as `app_scenarios` did.

### 2. A hosted benchmark service

- Good, because it reports on every pull request with a history and a dashboard, and counts
  instructions under valgrind, so it is not noisy.
- Bad, because the gate depends on an outside service, and its benchmarks are criterion
  functions that know nothing of a frame's work counters, so every scenario is written a second
  time.

### 3. A callgrind harness crate

- Good, because it counts instructions, cache events and heap per benchmark.
- Bad, because a scenario is registered again in its macros, CI installs its runner at the
  library's version, and it compares a benchmark with its own earlier run kept in `target/`, not
  with a checked-in file. What FLUI needs from it is running a binary under valgrind and reading
  the total, which the kit does.

### 4. A history store and a trend gate

- Good, because it suits a noisy metric, and Bevy and Flutter use it for device frame times.
- Bad, because every gate metric here is deterministic, so a store and a dashboard would hold what
  a file in the repository already holds, and a trend gate passes a slow drift the files fail.

### 5. One workload, many instruments, costs in a file

- Good, because of §1 and §2: one description, deterministic gates, the cost in the diff.
- Bad, because the kit owns a small harness (the window, the holder, the valgrind runner) that
  a service would have provided.

## More information

Flutter's counterpart is a test that wraps its steps in `traceAction` and reads a
`TimelineSummary`. FLUI differs in two ways, both because Rust has no garbage collector and its
counts reproduce: the primary metrics are counts and instructions, with time second, and the
baseline is a file in the repository, not a dashboard. Revisit §3 if the spread of instructions
over CI runs of one commit exceeds 1%, and §4 if the nightly runners become dedicated hardware.
