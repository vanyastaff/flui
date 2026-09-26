# CI design

- **Status:** Accepted 2026-09-26, with the owner's decisions in [§9](#9-open-points-for-the-owner).
  Implemented except the rows [§7](#7-migration-steps-each-pr-labelled-full-ci) lists as deferred
  (jobs whose commands do not exist yet, and the levers that need CI runs to measure).
- **Date:** 2026-09-26
- **Baseline:** `main` at `c2ba3ae51`; workflows as of that commit; CI runs from 2026-09-23 to
  2026-09-26.
- **Inputs:** every file in `.github/workflows/`, `tools/xtask/src/change_scope/`, issue #1279,
  every row of the plan's §9, and the numbers of the
  [build-footprint study](build-footprint.md). The hosted `windows-a11y` trial run has **not run**:
  this design schedules it (through `manual.yml`, migration step 5), and its result, PASS or
  CANNOT_VERIFY with the pre-check's reason, decides §3 row 5. Until then the claim that
  `windows-a11y` passes on hosted `windows-latest` stays unverified
  ([open question 22](open-questions.md#22-unverified-claims-that-adrs-must-not-state-as-fact)).

The owner's rule (open question 5, decided 2026-09-25): a fast pull-request lane over the changed
crates and their dependents; full runs only when needed (the `full-ci` label, `main`, nightly,
release); no platform-specific heavy runs while work on a branch is in progress.

The short version of what the measurements say: the machinery for that rule already exists
(`plan`, `fast-lane`, the `ci` aggregator), but in the measured window **75 of 79 successful pull
request runs took the heavy lane**, and the four that took the fast lane were whole-workspace runs
that were slower than the heavy lane. The design below keeps the machinery and changes what sends
a pull request where.

---

## 1. What runs today

### Workflows

| Workflow | Triggers | Jobs |
|---|---|---|
| `ci.yml` | `pull_request`, `push` to `main`, `merge_group`, nightly `17 3 * * *`, `workflow_dispatch` (`ci.yml:49-66`); PR runs cancel superseded ones (`ci.yml:75-77`) | `checks`, `plan`, `fast-lane`, `fast-lane-ios`, 14 heavy jobs (17 with matrix entries), `deps`, `ci`, `notify-main-red` |
| `full-ci.yml` | `pull_request: labeled` (`full-ci.yml:15`) | `rerun-ci`: cancels and re-runs the PR's `ci.yml` run so `plan` reads the label |
| `docs.yml` | `book/**` or itself, PR and `main` | mdBook build, offline lychee, Pages deploy on `main` |
| `release.yml` | tags `v*`, `workflow_dispatch` (`release.yml:20-21`) | `build` on five targets (`flui-cli` release binaries), `release` (draft GitHub release) |
| `weekly.yml` | Monday `17 5 * * 1`, `workflow_dispatch` (`weekly.yml:35-36`) | `latest-deps`, `bench`, `nightly-canary`, `release-lints`, `cli-live-build` (all advisory except `latest-deps`) |

### How a run picks its lane

`plan` (`ci.yml:281-359`) sets two outputs. `heavy` is true on every event but `pull_request`, and
on a pull request when the PR carries `full-ci` (read from the API, `ci.yml:342-345`) or
`cargo xtask affected` reports `heavy_required=true`. `mode` comes from
`classify` (`tools/xtask/src/change_scope/classify.rs:549`):

- `docs` when every file matches `DOCS_ONLY` (`classify.rs:32-44`);
- heavy and `full` when a file matches `HEAVY_TRIGGERS` (`classify.rs:49-62`: root `Cargo.toml`,
  `Cargo.lock`, `.cargo/**`, the toolchain file, `.github/workflows/**`, `**/*.wgsl`, `deny.toml`)
  or an xtask module a heavy job runs (`heavy_job_inputs`, `classify.rs:174`);
- `full` when a file matches `FULL_TRIGGERS` (`classify.rs:65-77`) or no package owns it
  (`classify.rs:613-619`);
- `none` when only `TOOLING` changed (`classify.rs:85-97`);
- `packages` otherwise, with the transitive dependents; `lane_args` then sets `heavy_required` when
  more than three dependents reach the change only under a feature
  (`lane_args.rs:33,295-304`).

The `ci` aggregator (`ci.yml:1647-1700`, `cargo xtask ci-verify`,
`tools/xtask/src/change_scope/aggregator.rs:166-182`) recomputes which jobs may skip from
`heavy`, `mode` and `cross_ios`, and fails on any other skip. `HEAVY_JOBS` (`ci.yml:1692-1695`)
lists the heavy jobs; a test pins it to the jobs whose `if:` is the heavy condition
(`aggregator.rs:375-384`).

### Measured job durations

Minutes from `gh run view <id> --json jobs` (start to completion of each job). Four heavy runs:
two pushes to `main`, one nightly, one PR with `full-ci`-equivalent heavy scope.

| Job | Runner | `main` a540312f8 (36203822844) | `main` cab06137d (36186367887) | nightly (36090648022) | PR (36174082423) |
|---|---|---:|---:|---:|---:|
| `checks` | ubuntu | 1.2 | 1.6 | 1.1 | 1.4 |
| `plan` | ubuntu | 0.6 | 1.3 | 0.6 | 1.3 |
| `deps` | ubuntu | 0.7 | 0.7 | 0.7 | 1.6 |
| `clippy` | ubuntu | 1.6 | 1.2 | 6.1 | 1.7 |
| `test (ubuntu-latest)` | ubuntu | 9.1 | 19.5 | 10.8 | 10.9 |
| `test-features` | ubuntu | **24.6** | 9.1 | 7.7 | 8.8 |
| `live-smoke` | ubuntu | 2.5 | 2.4 | 2.3 | 2.6 |
| `bench-compile (ubuntu-latest)` | ubuntu | 1.9 | 1.7 | 1.8 | 1.6 |
| `doc` | ubuntu | 1.9 | 1.4 | 6.2 | 1.7 |
| `doc-test` | ubuntu | 2.2 | 1.9 | 8.4 | 1.9 |
| `miri` (advisory) | ubuntu | 2.3 | 2.3 | 6.3 | 2.2 |
| `feature-matrix (1/3)` | ubuntu | 4.1 | 3.3 | 3.3 | 3.2 |
| `feature-matrix (2/3)` | ubuntu | 6.1 | 6.1 | 16.1 | 6.1 |
| `feature-matrix (3/3)` | ubuntu | **21.4** | 8.5 | 6.5 | 8.4 |
| `feature-matrix (combinations)` | ubuntu | 1.9 | 1.5 | 2.0 | 1.7 |
| `wasm-check` | ubuntu | 2.8 | 2.5 | 2.7 | 2.9 |
| `cross-typecheck` | ubuntu | 1.3 | 1.3 | 1.2 | 1.7 |
| `gpu-test` | windows | 7.1 | 9.4 | 8.4 | 11.5 |
| `platform-windows` | windows | 4.3 | 3.3 | 3.6 | 3.4 |
| `cli-macos` | macos | 3.2 | 3.3 | 2.7 | 2.6 |
| `ci` | ubuntu | 1.0 | 0.6 | 0.5 | 1.2 |
| **run wall-clock** (created to updated) | | 27.1 | 21 | 17 | 14 |

Where the long jobs spend it (steps over 20 s):

- `test-features` on 36203822844: `flui-assets full` + four `flui-widgets` image runs 10.8 min,
  the facade with `cupertino,localizations` 8.1 min, `signals` 4.9 min. Each `cargo nextest run`
  line resolves features for its own package set (`ci.yml:843-869`), so each one rebuilds the
  shared crates under another hash.
- `test` on 36186367887: `cargo build --workspace --all-targets` 9.2 min, nextest 6.0 min, the
  `flui-platform` leg 2.0 min; on 36203822844 the same steps took 4.9, 1.6 and under 0.4 min.
- `feature-matrix (3/3)`: its one step, 20.8 min on 36203822844 against 3.3 and 5.4 min for the
  other two shards: cargo-hack's `--partition` splits the command list into contiguous blocks, not
  equal work.
- `gpu-test`: the readback suite 3.7-5.5 min, the composited-layer step 1.2-1.8 min, cache
  restore and save 1.8-2.2 min.

Fast lane, the one recent run (36218618491, PR #1299, `mode=full` because it changed
`tools/xtask/src/change_scope/classify.rs`): `fast-lane` 15.4 min, run wall-clock 17.8 min.
Steps: cache restore 1.1, clippy 1.2, nextest 6.1 (of which the build, "Finished `test`
profile ... in 4m 12s", on a cache **hit**), rustdoc 0.9, doctests 0.2, four cross clippies 3.6,
wasm32 0.8, the `flui-platform` leg 0.5.

### Which lane pull requests actually took

The last 150 `ci.yml` runs, filtered to `conclusion == success` (100 runs, 2026-09-23 to
2026-09-26), classified by whether `fast-lane` or `clippy` ran:

| Event | Lane | Runs | Mean wall-clock (min) | Mean job-minutes |
|---|---|---:|---:|---:|
| `pull_request` | heavy | 75 | 16.5 | 89.7 |
| `pull_request` | fast (all `mode=full`) | 4 | 19.0 | 20.5 |
| `push` (`main`) | heavy | 18 | 24.5 | 104.6 |
| `schedule` | heavy | 3 | 18.9 | 90.7 |

Why the pull requests were heavy, from each run's `plan` log line
`event=pull_request full-ci-label=<l> heavy=<h>` (80 runs; 4 logs expired):

- 59 heavy **without** the label, because the change touched a heavy trigger. 46 of them are one
  branch, `tools/desktop-mcp` (PR #1287), which changed `.github/workflows/ci.yml`, `Cargo.lock`
  and `Cargo.toml`; one PR (#1288) changed `Cargo.lock`; five are dependabot workflow bumps.
- 13 heavy **with** `full-ci`.
- 3 fast; the fourth fast run's log had expired. All four fast runs were `mode=full`: two
  because `tools/text-spike/**` (a crate with its own `[workspace]`, not a member) is owned by no
  package (#1279), one for `classify.rs`, one for `ci.yml` before workflows were a heavy trigger.

No run in the window used the fast lane in `packages` mode, the case it was built for.

### Other facts the design depends on

- **Nothing requires `ci`.** `GET /repos/vanyastaff/flui/branches/main/protection` returns 404;
  the only ruleset (`Copilot`, id 9864977) requests Copilot review. The aggregator's verdict
  blocks no merge. `merge_group` is a trigger (`ci.yml:61`) with no merge queue configured.
- **Cache budget.** `gh cache list` shows 17 entries, all saved from `main`, 10.98 GB in total:
  `workspace-tests` 2.27 GB, `test-features` 1.59 GB, the four feature-matrix entries 2.32 GB, the
  rest 0.17-0.75 GB each. GitHub evicts least-recently-used entries once a repository passes its
  cache limit (10 GB by default), so a job can miss its own entry.
- **Cache contents do not match the fast lane's builds.** `test` saves `workspace-tests` after
  `cargo build --workspace --all-targets` with default features (`ci.yml:661-678`); `fast-lane`
  restores it (`ci.yml:397-405`) and then builds `-p <scope>` with
  `--features flui/cupertino,flui/localizations` (`ci.yml:431`, `lane_args.rs:346-349`). A
  different package set or feature set resolves features differently and rebuilds every shared
  crate: the 4 min 12 s test build above is on a warm cache. The
  [build-footprint study](build-footprint.md#duplicate-builds) measures the same effect locally.
- **The label path can fail.** `full-ci.yml:54` waits at most five minutes for a cancelled run;
  run 36194704724 ended "run 36194704156 cannot be rerun; This workflow is already running", and
  the PR had to be re-run by hand (attempt 2 of 36194704156).
- **Cost.** The repository is public, so hosted-runner minutes are not billed. The costs are
  wall-clock to a green check and concurrency: a heavy run starts 21 jobs, and GitHub's
  concurrent-job limit (20 on the Free plan, 5 of them macOS) makes a second heavy run queue.

---

## 2. Target lanes

`plan` replaces the two outputs `heavy` and `mode` with one output, **`lane`**, plus the
existing scope outputs. Each job's `if:` names the lanes it runs in, and the aggregator's expected
skips are a function of `lane` alone.

| Lane | When | Runs |
|---|---|---|
| `docs` | every changed file is documentation | `checks` |
| `tooling` | only files `checks` covers, or only a standalone crate outside the workspace (today `mode=none`) | `checks`, `plan`, `deps`, and `standalone` (`cargo check` of each changed standalone crate, C7) when one changed |
| `fast` | an ordinary PR push whose scope is a set of packages | `checks`, `plan`, `deps`, `fast-lane`, `fast-lane-ios` when `cross_ios` |
| `wide` | an ordinary PR push whose scope is the whole workspace or needs a heavy-only input (today `heavy_required`, or `mode=full`) | every Linux full job, in parallel; no Windows or macOS job |
| `full` | push to `main`, `merge_group` | every job in the `wide` set, plus the Windows and macOS jobs of today |
| `extended` | nightly `schedule`, `workflow_dispatch`, a PR labelled `full-ci` | `full`, plus the platform-heavy jobs §9 adds: `macos-ci`, `test-windows` (implemented); `windows-a11y`, `protocol-windows` (deferred, §7) |

A PR labelled `full-ci` gets exactly one lane: `extended` (C2 accepted). That choice is made in one
place, §5's `--event` mapping (`Lane::decide` in `tools/xtask/src/change_scope/lane_args.rs`),
and nowhere else. A `v*` tag
is not a `ci.yml` event (`ci.yml:49-66` has no tag trigger; tags start only `release.yml`), so no
lane is keyed on it: a release is checked by `release.yml`'s `release-check` job (§3).

What changes against today:

1. **`wide` replaces the whole-workspace fast lane** (#1279, gap 2). Today `mode=full` on a
   pull request runs everything serially in one job (15.1-16.2 min in #1279, 15.4 min in
   36218618491); the Linux jobs of the heavy lane run the same work in parallel and finished the
   heavy PR runs above in 14-16.5 min including the Windows jobs. `wide` also runs the jobs the
   serial fast lane leaves out (`test-features`, `feature-matrix`, `live-smoke`, `bench-compile`,
   the example links in `test`).
2. **Heavy triggers stop starting Windows and macOS jobs on a pull request.** A `Cargo.lock` bump
   or a workflow edit gets `wide`; the platform jobs run when the PR is labelled `full-ci`, and
   on `main`. That is the owner's "no platform-specific heavy runs during active work". It would
   have moved the 59 unlabelled heavy PR runs of the window off the Windows and macOS runners.
3. **A standalone crate is not "unowned"** (#1279, gap 1). A changed file under a directory whose
   `Cargo.toml` declares its own `[workspace]` and is not a member (today `tools/text-spike`) puts
   the run in `tooling`. No member can depend on it: a path dependency inside the workspace root
   becomes a member, so such a crate is outside every member's graph.
4. **The fast lane builds what the cache holds** (§4). It builds with the workspace's own feature
   resolution and narrows the tests with a nextest filterset, so a warm cache is warm.
5. **`extended` holds the jobs that exist for platform coverage, not for merge safety**: the new
   macOS `cargo xtask ci`, the Windows test matrix entry, the Windows UI Automation step and the
   protocol comparison. They run nightly and on demand, never on a PR push.

A PR's route in practice: pushes during work get `fast` (or `wide` when the change is
workspace-wide); adding `full-ci` when the PR is ready runs `extended` (or `full`, per C2) once,
on the commit that merges. AGENTS.md already asks for the label on risky PRs; C1 asks whether a
platform-sensitive scope should require it.

---

## 3. Where each job runs

`✓` runs and blocks; `a` runs advisory (`continue-on-error`); blank skips.

| Job | Runner | `fast` | `wide` | `full` | `extended` | Change against today |
|---|---|:-:|:-:|:-:|:-:|---|
| `checks` | ubuntu | ✓ | ✓ | ✓ | ✓ | also in `docs`, `tooling` |
| `plan` | ubuntu | ✓ | ✓ | ✓ | ✓ | outputs `lane` (§5) |
| `deps` policy | ubuntu | ✓ | ✓ | ✓ | ✓ | — |
| `deps` advisories | ubuntu | a | ✓ | ✓ | ✓ | blocking in `wide` (it already is for a `Cargo.lock` or `deny.toml` change) |
| `fast-lane` | ubuntu | ✓ | | | | workspace feature resolution, nextest filterset (§4) |
| `fast-lane-ios` | macos | ✓ if `cross_ios` | | | | — |
| `standalone` **(new)** | ubuntu | | | | | tooling lane only, when a standalone crate changed: `cargo check --locked --all-targets` of each (C7) |
| `clippy` | ubuntu | | ✓ | ✓ | ✓ | — |
| `test` | ubuntu | | ✓ | ✓ | ✓ | `cargo xtask test` after the all-targets build (§4) |
| `test-features` | ubuntu | | ✓ | ✓ | ✓ | one feature resolution per crate group (§4) |
| `live-smoke` | ubuntu | | ✓ | ✓ | ✓ | runs `cargo xtask live-smoke` (§9 row 1) |
| `bench-compile` | ubuntu | | ✓ | ✓ | ✓ | — |
| `doc`, `doc-test` | ubuntu | | ✓ | ✓ | ✓ | — |
| `miri` | ubuntu | | a | a | a | — |
| `feature-matrix` (shards) | ubuntu | | ✓ | ✓ | ✓ | shard count unchanged (§4.3 deferred) |
| `wasm-check` | ubuntu | | ✓ | ✓ | ✓ | — |
| `cross-typecheck` | ubuntu | | ✓ | ✓ | ✓ | — |
| `perf` **(deferred)** | ubuntu | | a | a | a | §9 row 4; blocking at the B1 exit; `cargo xtask perf` exists, the job is not in the workflows yet |
| `package-check` **(deferred)** | ubuntu | | ✓ | ✓ | ✓ | §9 row 6, if it does not fit in `checks`; lands with its command |
| `gpu-test` | windows | | | ✓ | ✓ | runs `cargo xtask gpu-test` (§9 row 2); not in `wide` (C3 declined) |
| `platform-windows` | windows | | | ✓ | ✓ | — |
| `cli-macos` | macos | | | ✓ | ✓ | stays in `extended` while `macos-ci` is advisory |
| `macos-ci` **(new)** | macos | | | | a | §9 row 3: `cargo xtask ci` + the iOS runner clippy; advisory until three green runs |
| `test-windows` **(new)** | windows | | | | a | §9 row 12: `cargo xtask test` on Windows; advisory until three green runs |
| `windows-a11y` **(deferred)** | windows | | | | a | §9 row 5, after the `manual.yml` trial; advisory until three green runs |
| `protocol-windows` **(deferred)** | windows | | | | a | §9 row 7, advisory until B3; lands with its command |
| `ci` | ubuntu | ✓ | ✓ | ✓ | ✓ | expected skips from `lane` |
| `notify-main-red` | ubuntu | | | on `main` | on schedule | — |

Outside `ci.yml`:

| Where | Job | Lane |
|---|---|---|
| `release.yml` | `release-check` **(deferred, with `cargo xtask release-check`)** before `build` | tags `v*` |
| `manual.yml` **(new)** | `trial`: `workflow_dispatch` with a `command` input choosing one of an allowlisted set of `cargo xtask device ...` commands and a `runner` input; not gated by `ci` | on demand (the hosted `windows-a11y` trial) |
| `weekly.yml` | unchanged; `latest-deps` gets the §4 test scope | weekly |
| `docs.yml`, `full-ci.yml` | `full-ci.yml` waits for the cancelled run without a five-minute cap and says so when it gives up (§1) | unchanged triggers |

### The plan's §9 rows

| # | Need | Where it runs | Notes |
|---|---|---|---|
| 1 | `sliver_demo` built with `--features material` (`ci.yml:924`) | `live-smoke` calls `cargo xtask live-smoke` and `cargo xtask live-smoke --wayland`, which build the example (`tools/xtask/src/tasks.rs:537`) | the change that gives the facade `default = []` then edits xtask, not the workflow; the workflow change lands with this design |
| 2 | `--no-default-features` on the Windows readback step (`ci.yml:1015-1024`) | `gpu-test` calls `cargo xtask gpu-test` (`tasks.rs:442-460`) | the flag goes with the facade's `default = []`, in xtask |
| 3 | `cargo xtask ci` on `macos-latest` (B0 exit [P]) | new `macos-ci`, `extended`; `cli-macos` stays in `full` | `cargo xtask ci` runs `flui-cli`'s tests, so `macos-ci` subsumes `cli-macos` where both run |
| 4 | `cargo xtask perf` | new `perf` job in `wide`, `full`, `extended`, advisory in B0; at the B1 exit blocking there and a `fast-lane` step when `flui-rendering`, `flui-view` or `flui-testing` is in scope | counters are counts, not timings, so a shared runner does not make them noisy |
| 5 | `cargo xtask device windows-a11y` with its pre-check | the hosted trial first, through `manual.yml`; then new `windows-a11y` in `extended`, advisory until three green runs, then blocking there | not in `platform-windows`: the release build of `a11y_probe` changes that job's profile |
| 6 | `cargo xtask package-check` | a step in `checks` if it runs under two minutes on a warm xtask cache; otherwise its own job in `wide`/`full`/`extended` | decided by the measurement in the change that adds the command |
| 7 | the protocol outline comparison on `windows-latest` | new `protocol-windows`, `extended`, advisory until B3 | — |
| 8 | one pinned nightly for rustdoc JSON | a toolchain step inside the jobs that need it (`api-closure` in `wide`/`full`, `release-check`) | only if the nightly requirement is confirmed (open question 22) |
| 9 | `cargo xtask release-check` | `release.yml`, job `release-check`, which `build` needs | **a deliberate change from the plan**, which puts it in `ci.yml` as a heavy job in `needs` and `HEAVY_JOBS`: the check guards a release, and a tag is the only event that makes one, so it runs on the tag and blocks the release `build`; on every `main` push it would re-run `cargo package` and semver-checks for no release. It is therefore outside the `ci` aggregator. Package dry-run and semver-checks there; evidence freshness stays local (row 10) |
| 10 | `fetch-depth: 0` in `release.yml` | not adopted | freshness runs locally before tagging, as the plan's default says |
| 11 | `cargo xtask dylib-exports` | `extended`, Windows, only if ADR-0096 is accepted | — |
| 12 | `windows-latest` in the test matrix (`ci.yml:638-643`) | new `test-windows`, `extended` | not a matrix entry of `test`: `test` runs in `wide` on every workspace-wide PR, and a matrix entry would put Windows there |
| 13 | the build-footprint levers | §4 | only the levers with a measured gain |

Every new job follows AGENTS.md: actions pinned to a full SHA, `--locked` on every cargo call,
caches saved only on `main`, the job's `name` equal to its key, listed in the aggregator's
`needs`, and in the lane list of §5.

---

## 4. Build-footprint levers adopted in CI

From the [build-footprint study](build-footprint.md). CI already builds with
`CARGO_INCREMENTAL=0` and `line-tables-only` debug info (`ci.yml:93-102`). The study measured
the first at 9.30 → 4.06 GB for one workspace test build and found nothing to add on debug info
that keeps file and line in a panic; both stay.

1. **One feature resolution per build, subsets by filterset.** The measured local cost of a
   subset build after a workspace build is in the study (§"Duplicate builds"). In CI:
   - `test`, `fast-lane` and `cargo xtask test` build the same `TEST_SCOPE`
     (`tools/xtask/src/tasks.rs:50-61`: `--workspace --exclude flui-platform --lib --bins --tests
     --features flui/cupertino,flui/localizations`). `test` keeps its separate
     `cargo build --workspace --all-targets` for the example links.
   - `fast-lane` narrows what it **runs**, not what it builds:
     `cargo nextest run <TEST_SCOPE> -E '<filter>'`, where `<filter>` is
     `package(a) | package(b) | ...` over the scope `cargo xtask affected` computes today. The
     scope keeps coming from the declared graph (optional and target-specific edges included),
     which nextest's `rdeps()` over the resolved graph would not reproduce.
   - `fast-lane`'s clippy runs `--workspace --all-targets`, the `clippy` job's command, so the
     dependencies' check-mode artifacts `test` warms on `main` (`ci.yml:753-755`) serve it.
   - The cost: a whole-workspace build graph on every fast-lane run instead of the scope's. What
     is saved is the rebuild of third-party dependencies under a second feature set. Measured on
     the Windows host for a `flui-widgets` edit: the scoped build's first run compiled 88 units in
     117 s and added 3.3 GB; the `TEST_SCOPE` build then took 34 s and added 0.35 GB; warm edits
     were 28-46 s either way ([study](build-footprint.md#duplicate-builds)).
   - **Those numbers do not transfer to CI.** They were taken after a workspace build existed in
     the same target directory, workspace crates included. CI's restored cache holds only the
     dependencies: `Swatinem/rust-cache` (v2.9.2) leaves `cache-workspace-crates` at its default
     `false` and prunes the workspace crates' artifacts before saving, and a fresh checkout gives
     every path package new mtimes, so cargo would treat them as dirty anyway. On CI every
     fast-lane run therefore compiles every workspace crate twice over: clippy in check mode over
     the workspace, then the whole `TEST_SCOPE` test build (the 4 min 12 s `mode=full` build of
     §5's table), where the scoped build compiled only the scope and its dependencies. For a leaf
     crate that is likely a loss; for a crate most of the workspace depends on it is about even.
     The shape is kept per C6, as a lever under C8: the first fast-lane runs are compared against
     the scoped runs before it (job duration, per step), and the scoped build comes back if they
     are slower. A cold local measurement says nothing either way until it is repeated with the
     dependencies warm and the workspace crates cold, which is CI's actual state.
     `cargo xtask check-changed` keeps its scoped build (C6 and the study's R1).
2. **`test-features` in one resolution per crate group.** The five `flui-assets`/`flui-widgets`
   invocations (`ci.yml:845-849`) become one:
   `cargo nextest run -p flui-assets -p flui-widgets --features flui-assets/full,flui-widgets/images,flui-widgets/asset-images,flui-widgets/network-images`.
   The features are additive (the feature policy's rule 1), so the tests that each run selected
   still compile. The unified run holds every test ID of the five old runs (`network-images`
   implies `asset-images`, which implies `images`, and no test in the old runs' targets is gated
   on one of those features being off), but a test ID is not the configuration it runs under:
   the old `--features images --test image` run compiled `Image` as the
   `cfg(not(feature = "asset-images"))` `StatelessView` impl, which is what a consumer of
   `images` alone builds and which the unified run never compiles. That run therefore stays as a
   second, small step (`cargo nextest run -p flui-widgets --features images --test image`); the
   other three old runs fold into the unified one. The `cargo nextest list` comparison was not
   run locally, so the first wide run's log is the measured check. The facade step is covered by `test` now that `test` runs `cargo xtask test`
   (`TEST_SCOPE`); the `signals` step goes with the first reactive step (ADR-0085).
3. **Rebalanced feature-matrix shards (deferred).** `--partition k/3` gave 3.3, 6.1 and 21.4 min on
   36203822844. The implementation measures `k/5` (five shards) and keeps it if the slowest shard drops under
   10 min; `tools/xtask/src/tasks.rs`'s `feature_matrix_stage` owns the shard count. Not adopted
   yet: the measurement needs CI runs (C8).
4. **Cache budget.** One cache per feature resolution, not per job: `clippy`, `fast-lane` and
   `test` share `workspace-tests`; `test-features`, the feature-matrix shards and `doc` keep their
   own. Implemented in part: `fast-lane` and `test` share `workspace-tests-v2` (renamed because
   rust-cache never overwrites an existing key, and the old entry holds the default-feature
   build); `clippy` keeps its own cache until the size effect below is measured (C8).
   - **Precondition: the warm-clippy step on `main`** (`ci.yml:753-755`, "Warm clippy artifacts
     for fast-lane's cache"). The earlier shared-key attempt failed for a reason the feature
     resolution does not touch (`ci.yml:598-608`): only one job can write a key, and with `test`
     as the writer clippy restored build-mode artifacts that its check mode cannot use (3.3 min
     against 1.2 min with its own cache). Sharing works only because `test` also runs clippy
     before it saves the entry, so the entry holds check-mode artifacts; removing `clippy`'s own
     cache without that step brings the 3.3 min back. The implementing PR keeps the step and
     says so in its comment.
   - **Size: not derived, to be measured.** At the baseline (`gh cache list`, 17 entries,
     10.98 GB), `clippy`'s own entry is 0.39 GB (421,549,070 bytes on 2026-09-26), which is the only saving sharing is known to
     give. The other effects are unmeasured and pull both ways: one `test-features` resolution
     should shrink its entry (about 1.6 GB), and five feature-matrix shards (lever 3) add two entries to
     today's four. The implementation reports the total from `gh cache list` before and after;
     staying under the 10 GB eviction limit is the goal, not a result this design shows.
   - **What the implementation adds, and holds back.** `workspace-tests-v2` sits beside the old
     `workspace-tests` (2.27 GB) until that entry goes unused for seven days or the owner deletes
     it (`gh cache delete`), and it holds two feature resolutions: `test`'s default-feature
     `cargo build --workspace --all-targets` for the example links, then `TEST_SCOPE`
     (`flui/cupertino,flui/localizations`, flui-platform excluded). Giving the link build
     `TEST_SCOPE`'s features would merge them but stop linking the examples under the facade's
     default features, so it is left for the first wide run to measure how much the second
     resolution costs. `macos-ci` and `test-windows` restore their caches but do not save them
     (`save-if: false`) while they are advisory: at 10.98 GB, two new multi-GB entries from the
     nightly run would evict the `test-features`, feature-matrix and `doc` entries, which only
     `main` and wide runs restore. They start saving on `main` once `gh cache list` shows room.

Rejected for CI, with the reason in the study: nextest archives (the test run is 1.6-6 min of a
job whose build is 4.9-9.2 min, so splitting the run saves little and the archive must be
uploaded and downloaded), sccache (not measured: not installed on the host, and a GitHub cache
backend competes for the same 10 GB), `split-debuginfo` (no effect on MSVC), cargo-sweep (CI
targets are fresh per run).

---

## 5. Implementation shape

### As implemented

The shape below is the proposal; the implementation follows it with these differences, each for
the reason given:

- **`HEAVY_JOBS` keeps its name** and lists the `wide` lane's jobs; its pinning test
  `heavy_jobs_list_matches_the_jobs_gated_on_heavy` stays. `FULL_JOBS` and `EXTENDED_JOBS` join
  it; no `WIDE_JOBS` exists.
- **The fast lane's filterset is a new output, `ci_test_args`**, not a new value of `test_args`,
  which stays scoped because `check-changed` reads it (C6). It is written without spaces,
  `-E package(a)|package(b)`: the workflow word-splits the value, so the quoted form
  `-E 'package(a) | package(b)'` would reach nextest as separate words.
- **The whole-workspace rebuild happens on CI's path only.** `affected` (CI's `plan`) calls
  `plan_args`, which rebuilds the arguments over the whole workspace for `wide`, `full` and
  `extended` and keeps the classification's reason; `check-changed` calls `lane_args` and keeps
  its scoped build even when the lane is `wide`.
- **A standalone crate is standalone only if no member reaches it.** Besides its own
  `[workspace]` table, no workspace package may declare a path dependency into its directory
  (read from `cargo metadata`'s `Dependency.path`), so "outside every member's graph" is
  checked, not assumed. A deleted standalone crate leaves no manifest to find: its files fall
  back to unowned, `Mode::Full`, `wide`.
- **The aggregator's rule is the complement of the jobs a lane runs**: every lane runs
  `checks` and `plan`; `tooling` adds `deps` (and `standalone` when plan names a crate), `fast`
  adds `deps`, `fast-lane` (and `fast-lane-ios` under `cross_ios`), `wide` adds `deps` and
  `HEAVY_JOBS`, `full` adds `FULL_JOBS`, `extended` adds `EXTENDED_JOBS`. Every other declared
  job must skip, so a job in no list and with no matching `if:` fails every run instead of
  passing unnoticed. Besides the three literal forms below, the conditions are the fast forms,
  `needs.plan.outputs.lane == 'tooling' && needs.plan.outputs.standalone != ''` (`standalone`)
  and `needs.plan.outputs.lane != 'docs'` (`deps`); a test fails on any other.

### `plan` and `cargo xtask affected`

- `LaneArgs` (`tools/xtask/src/change_scope/lane_args.rs:38-57`) gets `lane: Lane` with
  `enum Lane { Docs, Tooling, Fast, Wide, Full, Extended }`; `mode` stays as an internal detail of
  `Scope`. `affected --format github` prints `lane=<name>`.
- The event-dependent part moves out of the shell in `ci.yml:333-359` into xtask:
  `cargo xtask affected --event <name> --full-ci-label <bool> [--base <sha>]` decides:
  `schedule`/`workflow_dispatch` → `extended`; `push`/`merge_group` → `full`; a PR with the
  label → `extended` (C2; `full` if C2 is declined, and this is the only place the label's lane
  is decided); a PR with `heavy_required` or `Mode::Full` → `wide`; `Mode::Packages`
  → `fast`; `Mode::None` → `tooling`; `Mode::Docs` → `docs`. The shell keeps only the label read.
- `classify` (`classify.rs:599-619`): before a file counts as unknown, walk up its directories to
  the repo root; if one holds a `Cargo.toml` with a `[workspace]` table and no workspace package
  owns that directory, the file is standalone and joins `TOOLING` for that run.
- `heavy_job_inputs` (`classify.rs:174-216`) reads the jobs whose `if:` names `wide`, not the
  string `needs.plan.outputs.heavy == 'true'` (`classify.rs:189`).
- `fast-lane`'s arguments: its `test_args` output becomes the `TEST_SCOPE` string plus
  `-E '<package filter>'`. `check-changed` builds from the same `LaneArgs` field
  (`check_changed.rs:139-145`) and stays scoped (§4.1), so the filterset form is a separate value
  that only `affected --format github` prints, not a change to what `check-changed` runs. The filter never names `flui-platform` (`TEST_SCOPE` excludes it; its
  own leg tests it), so when the scope holds no other package `test_args` stays empty and the
  nextest step is skipped, as `lane_args.rs:329-337` does today; an empty `-E ''` would either
  fail to parse or select every test. `pkg_args` for clippy becomes `--workspace`; `doc_args`,
  `doctest_args`, `wasm_args` and `hack_args` stay scoped (rustdoc and doctests are per package
  anyway, and each already uses its own resolution).

### Job conditions

Each job's `if:` uses one of three literal forms, so the parser in `aggregator.rs:66` and the
pinning test can read them:

- `if: needs.plan.outputs.lane == 'fast'` (`fast-lane`, `fast-lane-ios` adds `&& needs.plan.outputs.cross_ios == 'true'`);
- `if: contains(fromJSON('["wide","full","extended"]'), needs.plan.outputs.lane)` (Linux full jobs);
- `if: contains(fromJSON('["full","extended"]'), needs.plan.outputs.lane)` and
  `if: needs.plan.outputs.lane == 'extended'` (platform jobs).

### Aggregator

- `ci-verify` reads `LANE` and three lists: `WIDE_JOBS`, `FULL_JOBS` (the platform jobs of
  `full`), `EXTENDED_JOBS`, replacing `HEAVY`, `MODE` and `HEAVY_JOBS`
  (`tools/xtask/src/change_scope.rs:170-212`).
- `expected_skips` (`aggregator.rs:166-182`) becomes a table from `lane` to the jobs that skip:
  `docs` → all but `checks`, `plan`, `ci`; `tooling` → all but those and `deps`; `fast` → the three
  lists, plus `fast-lane-ios` unless `cross_ios`; `wide` → `fast-lane`, `fast-lane-ios`,
  `FULL_JOBS`, `EXTENDED_JOBS`; `full` → `fast-lane`, `fast-lane-ios`, `EXTENDED_JOBS`;
  `extended` → `fast-lane`, `fast-lane-ios`, and the `full`-only `cli-macos`.

### Tests (in `cargo test -p xtask`, which `checks` runs)

| Test | Asserts | Fails today because |
|---|---|---|
| `classify::a_standalone_crate_is_tooling` | `tools/text-spike/Cargo.toml` and `tools/text-spike/src/main.rs` classify as `Mode::None` with a reason naming the standalone manifest | they are "no package owns" → `Mode::Full` (`classify.rs:613-619`) |
| `lane_args::whole_workspace_pr_takes_the_wide_lane` | a PR changing `tools/xtask/src/change_scope/classify.rs` gets `lane=wide` | today `heavy=false mode=full` |
| `lane_args::heavy_triggers_on_a_pr_take_the_wide_lane_not_full` | `Cargo.lock`, `.github/workflows/ci.yml`, `deny.toml` on a PR → `wide`; the same with `--full-ci-label true` → `extended` | there is no `wide`: the result is `heavy=true` |
| `lane_args::events_pick_their_lane` | `push` → `full`, `schedule` → `extended`, `workflow_dispatch` → `extended`, `merge_group` → `full`, `pull_request` with `--full-ci-label true` → `extended` | `--event` does not exist |
| `lane_args::fast_lane_builds_the_test_scope_and_filters` | `crates/flui-material/src/lib.rs` → `test_args` equals `TEST_SCOPE` plus `-E 'package(flui) \| package(flui-material) \| package(flui-web-counter)'` | `test_args` is `-p flui -p flui-material ...` |
| `lane_args::platform_only_scope_has_no_test_args` | `crates/flui-platform/src/lib.rs` with a scope of only `flui-platform` (dependents stubbed out of the graph) → `test_args` is empty, with no `-E` | passes today; pins the empty case so the filterset change cannot emit `-E ''` |
| `aggregator::wide_lane_skips_platform_jobs` | lane `wide` with `gpu-test` skipped is green; with `clippy` skipped is red | no `wide` lane |
| `aggregator::extended_jobs_skipped_on_main_is_green_and_on_schedule_is_red` | lane `full` may skip `macos-ci`; lane `extended` may not | no `extended` lane |
| `aggregator::lane_lists_match_the_job_conditions` | `WIDE_JOBS`, `FULL_JOBS`, `EXTENDED_JOBS` equal the jobs whose `if:` has each literal form | replaces `heavy_jobs_list_matches_the_jobs_gated_on_heavy` (`aggregator.rs:375`) |
| `aggregator::a_narrow_lane_on_main_or_nightly_is_red` | `push`/`merge_group` on `fast` or `wide`, and `schedule`/`workflow_dispatch` on `full`, are red even when every job the lane skips did skip: `ci-verify` checks the lane against the event outside `Lane::decide` | the old shell set `heavy` from the event itself |
| `aggregator::jobs_outside_the_lane_lists_have_their_own_condition` | `deps`, `fast-lane`, `fast-lane-ios` and `standalone` each carry the exact `if:` the aggregator assumes for it, so swapping two passes no test | only membership in a known set was checked |

The existing tests that pin `heavy` (`classify.rs:733-760`, `aggregator.rs:394-547`) are
rewritten against `lane` in the same PR; none is deleted without a replacement asserting the same
case.

### Docs to update in the implementing series

- `docs/testing.md` "CI jobs and their local commands" (the lane list and the job table, lines
  859-960 at the baseline) and "Only the heavy lane checks these".
- The header comments of `ci.yml` (lines 3-39) and `full-ci.yml`.
- AGENTS.md: the "Before a PR" row stays; "Risky PRs get the `full-ci` label" gains what the label
  now runs (the lane C2 settles). The review guideline "Manifests and workflows" says a new job is
  listed in the aggregator's `needs` "(a heavy one also in `HEAVY_JOBS`)"; it names `WIDE_JOBS`,
  `FULL_JOBS` and `EXTENDED_JOBS` instead once the aggregator reads them.
- `docs/testing.md:277-291` ("Local machine mode") contradicts `:244-264` (one target per
  checkout) and AGENTS.md's "a shared `CARGO_TARGET_DIR`"; the study recommends the per-checkout
  rule, and the three places are made to agree (not a CI file, but the same series).

---

## 6. Cost estimates

Estimates, from the measured job durations in §1; the implementation remeasures them.

| Case | Today | Target |
|---|---|---|
| PR touching one leaf crate (e.g. `flui-material`) | fast lane: never observed in `packages` mode; the test build alone was 4 min 12 s on a warm cache in `mode=full` | `fast`: cache restore 1.1 + workspace clippy + the whole `TEST_SCOPE` test build (every workspace crate: the restored cache holds only dependencies, §4.1) + run of the filtered tests + scoped rustdoc, doctests and cross clippies; unmeasured, the first fast-lane runs are the measurement against the scoped build (C8) |
| PR changing `Cargo.lock`, a workflow or `tools/xtask/src/change_scope/**` | heavy: 14-16.5 min wall-clock, about 90 job-minutes, 21 jobs, 3 of them Windows or macOS | `wide`: the Linux jobs only; long pole `test` or `test-features` (8.8-10.9 min on PR runs) plus `checks`; about 70 job-minutes, 18 jobs, no Windows or macOS runner |
| Push to `main` | 21-27 min, about 105 job-minutes; long poles `test-features` (up to 24.6) and `feature-matrix (3/3)` (up to 21.4) | `full`: the same jobs; with §4's two long-pole fixes the long pole becomes `test` (9-20 min) |
| Nightly | 17-19 min | `extended`: adds `macos-ci`, `test-windows`, `windows-a11y`, `protocol-windows`; not on any PR's critical path |
| The window's 75 heavy PR runs | 75 × ~90 = ~6,700 job-minutes, including ~75 × 15 on Windows and macOS | the 59 unlabelled ones as `wide` (~4,100 job-minutes, no platform runners) and the 13 labelled ones as `extended` |

---

## 7. Migration steps (each PR labelled `full-ci`)

Each step is one PR with `cargo xtask check-changed` and `cargo test -p xtask` green; each keeps
the old path working until the next.

1. **xtask first.** `Lane`, `--event`, the standalone-crate rule and the filterset arguments in
   `tools/xtask/src/change_scope/`, with the tests of §5; `affected --format github` prints both
   the old keys and `lane`. No workflow changes, so the PR runs today's lanes.
2. **Workflow switch.** `plan` passes `--event`; every job's `if:` uses `lane`; `ci` passes `LANE`
   and the three lists; the aggregator reads them. The old `heavy`/`mode` outputs are removed in
   the same PR. Proof in the PR: one run per lane, forced with `workflow_dispatch` (`extended`), a
   docs-only commit (`docs`), a one-crate commit (`fast`), a `Cargo.lock` touch (`wide`) and the
   label (`extended`).
3. **Job bodies call xtask.** `live-smoke`, `gpu-test` and `test` call `cargo xtask live-smoke`,
   `cargo xtask gpu-test` and `cargo xtask test` (§9 rows 1-2 then need no workflow edit).
4. **Footprint levers.** The unified `test-features` invocation, the shard count, the cache keys
   (§4), each with before and after job durations from the PR's own runs and `gh cache list`.
5. **New jobs.** `macos-ci`, `test-windows` in `extended`; `perf` advisory; `manual.yml` for
   the hosted `windows-a11y` trial. `windows-a11y`, `protocol-windows`, `package-check` and `release-check` land with the
   changes that create their commands (the Windows automated line, `flui-protocol`, `package-check`, `release-check`), each only adding its job in its
   lane, which this design has already placed.
6. **Required check.** After the owner's sign-off on C4: a ruleset on `main` requiring `ci`.

Rollback: revert the series; the old workflows are in git. The main risk is a job lost from the
merge path, which the aggregator's completeness rule (`aggregator.rs:6-9`) and
`lane_lists_match_the_job_conditions` catch.

### What landed, and what was deferred

Steps 1-5 landed as one series of commits on one branch, not five PRs, one commit per step;
each commit passes `cargo test -p xtask` on its own. The per-step PR runs above were not made:
this change could not trigger CI runs.

- **Landed:** the lane (`Lane::decide`, `--event`, `--full-ci-label`); the standalone-crate rule
  with its path-dependency guard and the `standalone` job (C7); `ci_test_args`; every job's `if:`
  and the aggregator on `lane` with `HEAVY_JOBS`, `FULL_JOBS` and `EXTENDED_JOBS`; `test`,
  `live-smoke` and `gpu-test` calling xtask; `fast-lane` on the `TEST_SCOPE` build with the
  filterset and a workspace-wide clippy; `workspace-tests-v2`; the unified `test-features` run;
  `weekly.yml`'s latest-deps on the test scope; `macos-ci` and `test-windows` (advisory);
  `manual.yml`; `full-ci.yml`'s 25-minute wait and `gh run rerun` retries.
- **Deferred, the commands do not exist yet:** `package-check`, `release-check`,
  `protocol-windows`. `cargo xtask perf` exists now; its advisory job is not added yet, and its
  baseline was blessed on Windows, so the first ubuntu run is also the first check that the
  counts match there. `windows-a11y` waits for the `manual.yml` trial (§9 row 5).
- **Deferred, they need CI runs to measure (C8):** the `k/5` feature-matrix shards (§4.3) and
  sharing `clippy`'s cache (§4.4).
- **Not run:** the per-lane proof runs of step 2. The first runs of the PR itself (`wide`, since
  it changes workflows and the lane code) and of the label (`extended`) are the first evidence.
- **Not in this change:** step 6 (C4) is a repository setting the owner changes.

---

## 8. Out of scope

- Self-hosted runners, larger hosted runners, or paid concurrency.
- A merge queue (the `merge_group` trigger stays, mapped to `full`; C4 asks whether to enable one).
- Changing what any job checks, beyond the lane it runs in and the §4 levers.
- `weekly.yml`'s set of jobs, and `docs.yml`.

---

## 9. Open points for the owner

Decided 2026-09-26:

| # | Decision | Reason |
|---|---|---|
| C1 | No | Platform-crate PRs merge on `fast` or `wide`; platform coverage runs on `main`, nightly and the label, and a red `main` is fixed forward |
| C2 | Yes | The label runs `extended` |
| C3 | No | `gpu-test` runs on `main`, nightly and the label only |
| C4 | Not in this change | Requiring `ci` is a repository setting the owner changes |
| C5 | No | `macos-ci` runs nightly and on the label |
| C6 | Yes, CI's `fast-lane` only | `check-changed` keeps its scoped build |
| C7 | Standalone crates compile only in the tooling lane, when their files change (the `standalone` job) | — |
| C8 | Unmeasured levers stay unadopted | Includes the `k/5` shards and sharing the clippy cache |

The proposals as they were put:

- **C1. Platform-sensitive PRs.** Should a PR whose scope contains `flui-platform`, `flui-app`,
  `flui-engine` or `flui-desktop-mcp` be unable to merge green without `full-ci`? Proposed: yes,
  enforced by `ci-verify` failing a `fast` or `wide` run with a message naming the label when
  `plan` reports `platform_sensitive=true`. Without it, a Windows-only break is first seen on
  `main` (then fixed forward or reverted within the hour).
- **C2. What the label runs.** Proposed: `extended` (everything, including the nightly-only
  platform jobs), so a labelled PR proves what nightly would. The alternative is `full`, which
  skips `macos-ci`, `test-windows`, `windows-a11y` and `protocol-windows` (roughly 30-60 min of
  macOS and Windows time, not yet measured).
- **C3. Shader changes.** Every lane already parses shaders: `crates/flui-engine/build.rs` runs
  `wgsl_bindgen` (naga) over six shaders on every build, Linux included, and `cargo xtask wgsl`
  in `checks` runs a uniformity check over all 31. What only `gpu-test` on Windows does is
  create the shader modules on a device and draw with them, which is where a shader that parses
  but is rejected by the backend or renders wrong shows up. Proposed:
  `gpu-test` runs in `wide` when a shader or `flui-engine` changed, as the one exception to "no
  platform-heavy runs during active work". Otherwise a shader break reaches `main`.
- **C4. Make `ci` required.** Today it blocks nothing (§1). Proposed: a ruleset on `main` that
  requires the `ci` check; optionally a merge queue so `full` runs before, not after, a merge.
- **C5. `main` pushes run `full`, not `extended`.** Proposed: yes; `extended` nightly. If the
  owner wants `macos-ci` (the B0 exit's "`cargo xtask ci` is green on `macos-latest`") on every
  `main` push, it moves to `full` at about 25-35 min of macOS per push (estimate).
- **C6. The fast lane's build graph.** Proposed: CI's `fast-lane` builds the whole `TEST_SCOPE`
  and filters the run (§4.1), because its cache always holds a workspace build. (That premise
  holds for the dependencies only: rust-cache does not keep workspace crates, §4.1, so the
  decision stands as an unmeasured lever until the first fast-lane runs are compared.) The alternative
  keeps `-p <scope>` and accepts the second feature resolution; the study's numbers are the
  trade-off. `cargo xtask check-changed` is not part of this proposal: a fresh worktree starts
  cold, the study measured only the warm case, and the cold comparison comes first.
- **C8. Levers and measures the study did not run.** The plan asks for each lever to be kept or
  rejected on its measured effect, and for peak memory per compiling job. The study falls short
  there, and the owner either accepts these gaps or asks for the runs:
  sccache and cargo-sweep (not installed, not measured); one shared target directory (rejected on
  soundness, not run); test-target consolidation (about 90 MB, from binary sizes, not a rebuild);
  the nested-cargo saving of R3 (the incremental share, not a before-and-after); peak memory
  (measured for the whole process tree at 8 jobs, not per `rustc`); and the cold scoped build
  against a cold `TEST_SCOPE` build (C6).
- **C7. Standalone crates.** Proposed: `tooling`, with nothing compiled for them. The alternative
  adds a `cargo check --manifest-path <crate>/Cargo.toml` step to `checks`.
