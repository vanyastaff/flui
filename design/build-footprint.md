# Build-footprint study

- **Status:** Measured; recommendations Proposed. The build-footprint step of the
  [migration plan](../docs/plans/2026-09-25-architecture-migration-plan.md). The CI side of the
  recommendations is in [ci.md](ci.md#4-build-footprint-levers-adopted-in-ci).
- **Date:** 2026-09-26
- **Baseline:** `main` at `c2ba3ae51`, toolchain 1.98.1 (`rustc 1.98.1 (48a229cea 2026-09-01)`),
  cargo-nextest 0.9.146.
- **Host:** the Windows development host (Windows 11, 32 logical CPUs, 64 GB), MSVC `link.exe`,
  **shared with other agents building at the same time** (CPU load 82 % and 11 other `rustc`
  processes in a snapshot during the run). Every timing below is noisy; sizes and counts are not.

The owner's problem (open question 20) is test and build time, disk use and memory growth. The
short answer from the numbers: the largest cost is not any single artifact kind but **the number
of feature resolutions a target directory accumulates**. One workspace test build is 9.3 GB; the
same worktree reached 19.7 GB after six more test builds that each resolved features for a
different package set, and incremental caches are most of the growth.

---

## Method

Everything ran in a worktree of its own (`git worktree add ... origin/main`) with its own target
directory, never the shared checkout:

```powershell
$env:CARGO_BUILD_JOBS = "8"
$env:CARGO_TARGET_DIR = "<worktree>\target"      # a fresh directory per lever
cargo nextest run --workspace --no-run            # the cargo xtask test-equivalent build
```

The profile is the workspace's own (`Cargo.toml:788-831`): `[profile.dev]` with
`incremental = true` and `debug = "line-tables-only"`, dependencies at `opt-level = 3` and
`debug = false`; the `test` profile inherits it. No `RUSTFLAGS` and no user-level `rustflags`
were set.

- **Sizes** are the sum of file lengths under the directory, by extension and subdirectory
  (`Get-ChildItem -Recurse -File | Measure-Object Length -Sum`).
- **Peak memory** is the largest sum of `WorkingSetSize` over the cargo process and all its
  descendants (`Win32_Process`, walked by `ParentProcessId`), sampled once a second. Other agents'
  builds are not counted. It is the peak of the whole build at 8 jobs, not per `rustc`.
- **Wall-clock** is from process start to exit.
- **Test binaries** are counted with
  `cargo nextest list --workspace --list-type binaries-only --message-format json`.
- **A warm one-line edit** appends `#[allow(dead_code)] const FOOTPRINT_PROBE: u32 = <n>;` to
  `crates/flui-widgets/src/lib.rs`, rebuilds, and restores the file; each edit uses a new `<n>`.

---

## Baseline: one workspace test build

`cargo nextest run --workspace --no-run`, cold, in an empty target directory.

| Measure | Value |
|---|---:|
| Wall-clock (noisy) | 616.6 s (cargo: "Finished `test` profile ... in 8m 37s") |
| Units compiled | 442 |
| Peak working set, process tree, 8 jobs | 3.9 GB (14 processes) |
| **`target/` total** | **9.30 GB**, 30,059 files |
| `debug/incremental/` | **5.10 GB** (55 %), 24,876 files |
| `debug/deps/` | 3.03 GB, 1,763 files |
| ├ `.pdb` (debug info) | 1.04 GB, 136 files |
| ├ `.rlib` | 0.97 GB, 431 files |
| ├ `.rmeta` | 0.58 GB, 430 files |
| ├ `.exe` (test and bin executables) | 0.31 GB, 104 files |
| └ `.dll` (proc macros) | 0.12 GB, 32 files |
| `debug/examples/` (linked examples) | 0.92 GB, 153 files |
| `debug/build/` (build scripts: executables and `.pdb`; `OUT_DIR` contents under 0.01 GB) | 0.21 GB, 212 executables and PDBs |
| All `.pdb` in the tree | 1.83 GB, 296 files |
| Test binaries (nextest) | **101**: 33 library, 60 integration-test, 7 binary, 1 proc-macro; 308 MB of executables, 966 MB of PDBs |
| Workspace rlibs | 27 files for 26 crates (`libflui_foundation` twice) |
| Distinct hashes: `libflui_rendering-*.rlib`, `libflui_widgets-*.rlib` | 1 and 1 |

The largest test binaries: `flui-app` lib tests 23.3 MB (+49.4 MB PDB), `flui-engine` lib
16.6 MB, `flui-widgets::widgets_it` 13.7 MB (+54.8 MB PDB), `flui-widgets` lib 11.6 MB,
`flui-material::material_it` 11.1 MB.

### Warm one-line edit of `flui-widgets`

| Command, after the edit | Runs (s) | Peak working set |
|---|---|---:|
| `cargo nextest run -p flui-widgets --no-run` | 26.9, 21.3, 16.7 | 1.1-1.2 GB |
| `cargo nextest run --workspace --no-run` | 49.0, 42.0 | 3.3-3.8 GB |

The `.cargo/config.toml` comment records 6.7-7.1 s for the per-crate case on a quiet host; the
2.5-4x here is the shared host, not a regression. A no-op rebuild is 2.4 s (`-p flui-widgets`)
and 3.6 s (workspace).

---

## Duplicate builds

The plan asked whether per-crate `testing` features build the upper stack more than once. After
one workspace build they do not (one hash each). **Every build over a different package set
does**, and not only through `testing`: resolver 2 unifies features over the packages selected,
so `-p <x>` resolves third-party features too (`syn`, `windows`, `tokio`, `winit`, `tracing`
were rebuilt below).

Each row runs in the same target directory, in this order, after the baseline:

| Build | Wall-clock (s) | Units compiled | `target/` after | Growth |
|---|---:|---:|---:|---:|
| baseline, `--workspace --no-run` | 616.6 | 442 | 9.30 GB | — |
| `-p flui-widgets --no-run` (first time) | 136.0 | 65 | 11.12 GB | +1.82 GB (incremental +1.17) |
| `-p flui-rendering --no-run` (first time) | 65.1 | 38 | 14.32 GB | +3.20 GB (incremental now 9.25 GB) |
| `cargo xtask test`'s `TEST_SCOPE` build (`--workspace --exclude flui-platform --lib --bins --tests --features flui/cupertino,flui/localizations --no-run`) | 34.4 | — | 14.67 GB | +0.35 GB |
| two `flui-widgets` edits, `TEST_SCOPE` | 46.3, 31.7 | — | 14.82 GB | +0.15 GB |
| `check-changed`'s scope for a `flui-widgets` edit (`-p flui -p flui-app -p flui-cupertino -p flui-localizations -p flui-material -p flui-web-counter -p flui-widgets -p hot-reload-counter-host -p hot-reload-counter-logic --lib --bins --tests --features flui/cupertino,flui/localizations --no-run`), first time | 117.3 | 88 | 18.39 GB | +3.33 GB |
| two `flui-widgets` edits, that scope | 28.4, 30.6 | — | 19.74 GB | +1.35 GB |

After the `-p flui-rendering` row, 96 rlib names had more than one hash (216 files); the workspace
crates alone: `libflui_foundation` ×4, `libflui_rendering`, `libflui_objects`, `libflui_painting`,
`libflui_platform`, `libflui_interaction`, `libflui_layer`, `libflui_scheduler`,
`libflui_semantics`, `libflui_animation`, `libflui_tree` ×3, the rest ×2.

What it means:

- A warm edit costs about the same either way (28-46 s here), because only the changed crate and
  its dependents rebuild in both.
- A **new** package set costs a partial rebuild of the whole graph (65-136 s here) and 1.8-3.3 GB
  of disk, most of it incremental caches, and the directory keeps every variant. Each
  `cargo xtask check-changed` on a branch with a different set of changed crates is a new
  package set; so is each `-p <crate>` run by hand.
- CI has the same shape (see [ci.md](ci.md#other-facts-the-design-depends-on)): the fast lane's
  test build took 4 min 12 s on a warm cache because the cache was built with a different
  resolution.

---

## Levers

Each lever against the baseline, one at a time, in a fresh target directory where it changes
the fingerprint.

| Lever | How measured | `target/` | Incremental | PDBs | Cold (s, noisy) | Warm workspace edit (s, noisy) | Peak | Verdict |
|---|---|---:|---:|---:|---:|---:|---:|---|
| none (baseline) | above | 9.30 GB | 5.10 GB | 1.83 GB | 616.6 | 49.0, 42.0 | 3.9 GB | — |
| `CARGO_INCREMENTAL=0` | same build, env set | **4.06 GB** | 0 | 1.69 GB | 477.2 | 88.8, 119.0 | 3.7 GB | **kept for one-shot builds** (CI, reviews, agents that build once); rejected as the local default |
| `debug = false` for workspace crates (`CARGO_PROFILE_DEV_DEBUG=false CARGO_PROFILE_TEST_DEBUG=false`) | same build, env set | 7.32 GB | 4.13 GB | 0.88 GB | 509.1 | 36.4, 31.1 | 4.6 GB | rejected as a default: panics lose file and line; available per run |
| `split-debuginfo` = `off` / `packed` / `unpacked` | `cargo test --no-run` of a scratch library crate, one target directory each | — | — | identical | — | — | — | **rejected: no effect on MSVC**. Executable 740,864 B and PDB 3,207,168 B for all three values; MSVC always writes a PDB (`debug = false` still wrote 3,035,136 B) |
| One feature resolution for test builds (`TEST_SCOPE` everywhere, subsets by nextest filterset) | the duplicate-builds table | +0.35 GB once, then +0.15 GB per two edits | — | — | — | 46.3, 31.7 | 2.9-3.2 GB | **kept**: avoids +1.8-3.3 GB and 65-136 s per new package set |
| Consolidating per-file test targets | binary list and sizes | — | — | — | — | — | — | **mostly done already**; see below |
| nextest archive | `cargo nextest archive --workspace --archive-file ws.tar.zst` on the `CARGO_INCREMENTAL=0` target | archive 114.1 MB, 137 files, 3.7 s to write | | | | | | not a disk lever; rejected for CI in [ci.md](ci.md#4-build-footprint-levers-adopted-in-ci) |
| sccache | — | | | | | | | **not measured**: not installed on the host, and not installed for this study. `Cargo.toml:800-804` already records that sccache refuses incremental builds and crashed rustc on Windows with incremental on |
| cargo-sweep | — | | | | | | | **not measured**: not installed. Its effect on a used directory is bounded by what it deletes; deleting `debug/incremental` alone frees 55 % of the baseline and 65 % of the directory after the per-crate builds (9.25 of 14.32 GB) |
| One target directory shared by worktrees | not run | | | | | | | **rejected**: unsound, see below |

### Consolidating test targets

The workspace is already mostly on one integration binary per crate (the `*_it` targets). Of the
60 integration-test binaries, what stays separate:

- `flui-log`: 15 binaries, 30.2 MB of executables and 67.3 MB of PDBs, 15 links. Each file tests
  the process-global logger slot. Under nextest every test is its own process, so one binary would
  behave the same; under `cargo test` they would share a process. Gain about 90 MB and 14 links
  per build (1 % of the baseline). Low priority.
- Allocation-counting tests (`flui-scheduler`'s two, `flui-engine::raster_backpressure_allocation`,
  `flui-interaction::pointer_route_hot_path`): each installs a `#[global_allocator]`, so each
  needs its own binary. Keep.
- `compile_fail` (trybuild) and `wasm32` targets. Keep.
- `flui-widgets`' `image`, `image_async`, `image_network`: feature-gated (`required-features`), so
  they build only in the feature runs. Keep.

### A target directory per worktree against a shared one

Measured on this host, the other agents' worktree targets were 10.9, 14.4, 27.7 and 40.4 GB (and
0.8 GB). The 40.4 GB one breaks down as 21.3 GB `debug/` (13.3 GB of it incremental),
7.30 GB `cli-template-check/`, 7.39 GB `facade-consumer-check/`, 2.47 GB `tests/trybuild/`, and
cross-target and doc directories under 0.6 GB each.

Sharing one directory would save the duplicated dependency builds, but it is unsound, as
`docs/testing.md:244-257` records: the same unit built from two worktrees' sources gets the same
fingerprint, and a worktree can link another worktree's code (reproduced 2026-09-22).
`cargo xtask check-changed` refuses a target directory outside the checkout for that reason
(`tools/xtask/src/tasks/check_changed.rs:260-264`). The documents disagree, though:
`docs/testing.md:277-291` ("Local machine mode") and AGENTS.md's Gotchas still describe "a shared
`CARGO_TARGET_DIR`". This study keeps the per-checkout rule.

### The nested-cargo test builds

Not part of the `--no-run` baseline, but the largest single item seen on this host: the 24
nested-cargo tests build generated projects into three directories under the target
(`docs/testing.md:230-242`). In the 40.4 GB worktree they held 17.2 GB, and **7.6 GB of it is
incremental caches** (`cli-template-check/debug/incremental` 3.68 GB,
`facade-consumer-check/debug/incremental` 3.92 GB): the generated projects build with Cargo's
default dev profile, incremental and full debug info, not the workspace's. They are also started
with `max-threads = "num-cpus"` (`.config/nextest.toml`, group `nested-cargo`), each nested cargo
with the inherited job count, which is what can exhaust memory on a shared host.

Not re-run here (a cold run is 9-10 min on the M1 of `docs/testing.md:219-223`); the effect
below is the measured incremental share, not a before-and-after.

---

## Recommendations

| # | Recommendation | Files | Effect (measured unless marked) |
|---|---|---|---|
| R1 | **One feature resolution for test builds.** `cargo xtask check-changed` and CI's fast lane build `TEST_SCOPE` (`tools/xtask/src/tasks.rs:50-61`) and narrow the run with `-E 'package(a) \| package(b) ...'` over the scope `cargo xtask affected` computes | `tools/xtask/src/change_scope/lane_args.rs:327-345` (`test_args`), `tools/xtask/src/tasks/check_changed.rs:139-145` | avoids +1.8-3.3 GB and 65-136 s for each new package set; warm edits unchanged (28-46 s) |
| R2 | Keep `incremental = true` for local edit loops; set `CARGO_INCREMENTAL=0` for one-shot builds (a review, an agent's one `check-changed` before a PR) | `docs/testing.md` (guidance) | 9.30 → 4.06 GB; a warm edit is about 2x slower without it |
| R3 | `CARGO_INCREMENTAL=0` on the nested-cargo commands, and `max-threads = 4` for the `nested-cargo` group | `tests/facade_consumer.rs:95-102`, `crates/flui-cli/tests/cli_create.rs` (the four `Command`s at `:122`, `:186` and after), `.config/nextest.toml` | about −7.6 GB per worktree that runs the full suite (incremental share, not re-run); bounded memory |
| R4 | Make the three documents agree on one target per checkout | `docs/testing.md:277-291`, AGENTS.md Gotchas | no disk change; removes the instruction that leads to the unsound setup |
| R5 | Keep `debug = "line-tables-only"`; do not add `split-debuginfo` or `/DEBUG:NONE` | none | `split-debuginfo` has no effect on MSVC; `debug = false` saves 1.98 GB but drops file:line |
| R6 | Deleting `target/debug/incremental` (or `cargo sweep --maxsize`) is the one clean-up step worth scripting; a worktree's target goes with the worktree after merge (already the rule) | `docs/testing.md` | frees 55-65 % of a used directory |
| R7 | Merge `flui-log`'s 15 test files into one binary | `crates/flui-log/tests/` | about 90 MB and 14 links per build; low priority |

What R1 needs as tests (they belong to the CI implementation, together with [ci.md §5](ci.md#5-implementation-shape)):

- `lane_args::fast_lane_builds_the_test_scope_and_filters`: for
  `crates/flui-material/src/lib.rs`, `test_args` is the `TEST_SCOPE` string followed by
  `-E 'package(flui) | package(flui-material) | package(flui-web-counter)'`. Fails today: the
  value is `-p flui -p flui-material -p flui-web-counter --lib --bins --tests`
  (`lane_args.rs:327-345`).
- `check_changed::dry_run_builds_the_test_scope`: `cargo xtask check-changed --dry-run` prints
  one `cargo nextest run --workspace --exclude flui-platform ... -E` line and no `-p`. Fails today
  on the same value (the expected dry-run text is pinned at `check_changed.rs:348`).

After R1 lands, the CI implementation re-measures this table's duplicate-builds rows and the CI fast lane's test
build time.

## Out of scope

- Dynamic linking: measured separately ([dynamic-linking.md](dynamic-linking.md)) and deferred.
- A different linker: `rust-lld` was measured before (`.cargo/config.toml`) at 6.7-7.1 s against
  7.3-7.5 s for a warm edit, and not adopted.
- Codegen settings of the workspace crates (`opt-level`, `codegen-units`): they change runtime
  behaviour of the test suite, not only the footprint.
- Linux and macOS numbers: CI measures Linux (see [ci.md](ci.md#measured-job-durations)); this
  host is Windows.
