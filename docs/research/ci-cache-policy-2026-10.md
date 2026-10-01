# CI cache policy and validation, 2026-10-01

## Decision and scope

Keep the existing lanes, commands, platform execution, assertions and independent feature graphs.
Stabilize the compiler inventory, put native workspace dependencies on a trusted-main write path,
and make the Linux nested job the sole writer of its existing workspace-test family. This targets
measured compilation on the critical path rather than the 99 seconds of ordinary Windows tests.
No timeout is tightened. No cache is deleted and no repository/account setting is changed.

The [baseline](ci-baseline-2026-10.md) has eleven run snapshots, step timestamps and Cargo log excerpts.
The [cache inventory](ci-cache-inventory-2026-10.json) records the API response before changes.
These are observational comparisons, not a controlled experiment. During the investigation main
advanced to e8909cfca (the merged final #1408 tree); this branch was updated to it before final
verification. Its full source tree matches #1408 head 33c22f6213fd93769313f1e07e65cce1b71bdb68.
Latest main run 36901416035 succeeded in 30.73 wall / 223.83 runner minutes; test-nested ended
last at 26.82 minutes and eight Rust restores missed. Main has two fewer jobs than extended PRs.

## Confirmed causes

- In run 36847748985, the GPU cache considered only Rust 1.98.1 and restored the
  `0eaa406c-b7be04c3` family. In 36864134553 and 36894848200, setup installed rolling stable
  1.99.0 while the workspace still compiled with 1.98.1. The environment prefix became
  `f9e5b087`; the dependency/config suffix remained `071a4281`. Linux changed
  `0aacd5b2` to `0b90b9aa` for the same reason.
- Pinned [rust-cache config](https://github.com/Swatinem/rust-cache/blob/6323deb102c322ba6fcbdcafc7e3dddab59af2b6/src/config.ts)
  hashes *every installed rustup compiler*, OS/architecture and the CARGO/CC/CFLAGS/CXX/CMAKE/RUST
  environment prefixes. The displayed short prefix is not a fallback:
  [restore](https://github.com/Swatinem/rust-cache/blob/6323deb102c322ba6fcbdcafc7e3dddab59af2b6/src/restore.ts)
  only falls back within the environment-inclusive prefix. Lock/config changes can reuse an older
  dependency generation; an unrelated installed compiler change cannot.
- Windows and macOS workspace families have unique keys, `save-if: false`, and no entries in the
  inventory. They cannot become warm through another job's writes.
- The Linux family was written by `test --fast`, which never executes the nested consumers.
  `test-nested` restored it but never wrote its additional dependency artifacts. The pinned
  [cleanup](https://github.com/Swatinem/rust-cache/blob/6323deb102c322ba6fcbdcafc7e3dddab59af2b6/src/cleanup.ts)
  does recursively visit nested target roots: the problem is which producer populates the archive,
  not an inability to cache nested directories.
- GPU has two intentional Cargo resolutions: engine `testing` library tests and the facade's
  no-default-feature readback consumer. Nested generated/consumer projects use their own target
  roots and default consumer profiles, unlike workspace dependencies optimized at level 3.
  Cache restore does not make these artifacts interchangeable.

Exact cache archive contents and historical eviction decisions are not exposed by the inventory
API. CPU/RAM pressure and lock wait durations were not instrumented in the old logs. The source
shows Cargo serialization inside each shared nested root, but not how many seconds it cost.
Compile and link durations are combined in Cargo's Finished messages. These remain measurement
limits rather than established causes.

## Alternatives and adversarial review

| Option | Benefit | Cost and failure modes | Decision |
|---|---|---|---|
| Isolated pinned Rust, existing dependency cache mechanism, native families and one nested writer | Eliminates confirmed compiler-inventory invalidation; enables reuse on the extended critical path; keeps Cargo freshness and upstream cleanup | One cold migration; new native sizes need main measurements; GitHub LRU can evict an infrequently used family | Selected |
| Custom active-compiler compatibility seeds, registry split from selected dependency artifacts | Can avoid unrelated manifest metadata generations and allocate explicit family slots | Immutable seeds age when dependencies change; needs refresh policy, custom pruning and key logic; restores add overhead | Rejected for this change: more maintenance before fixing the demonstrated key bug |
| Lower CI dependency optimization or parallel macOS gates | Could improve cold wall time | Lower optimization may slow GPU/shaping/raster tests; splitting repeats builds and increases runner minutes; requires controlled A/B runs | Deferred; opt-level 3 is not removed on the strength of build time alone |

Independent cache and build/coverage reviews found no command or lane coverage loss. Their
material objection is that native compressed archive sizes are not yet measured, so the projected
useful cohort is not a proven fit. The policy records that limit instead of claiming a hard
per-family retention guarantee.

Specific failure scenarios considered:

- Poisoning/isolation: only successful jobs with `refs/heads/main` write dependency entries. PRs
  and branch dispatches restore only. Remote actions remain SHA-pinned. Runner-local rustup is
  outside the checkout, so it cannot enter manifest globbing or target archives.
- Stale artifacts: keep rust-cache's compiler/environment/config/dependency keys and Cargo's
  freshness checks; workspace products, incremental data and tool binaries are not saved.
  Source-changing consumers are still created and compiled on every run.
- Feature unification: do not combine the two GPU graphs, consumer roots, platform targets or
  library-only/dev-target feature passes. A missing feature edge must still fail in isolation.
- Disk exhaustion: native caches explicitly select debug dependency folders for the workspace,
  CLI-generated and facade consumers. They exclude iOS/clippy-only target roots, rustdoc,
  examples and incidental outputs. New post-build metrics report uncompressed roots, registry/git
  and free disk; API archive bytes remain a separate measure.
- Cancellation/failure: cache publication is optional and successful-job-only. A failed/canceled
  sole writer leaves no new entry but cannot turn a test failure into success. Existing entries
  remain eligible for restore. Partial failed builds are not published.
- Flakiness: nextest groups, timeouts, GPU adapter requirement, snapshot/readback diagnostics and
  the existing leaky-test reporting remain unchanged. No failure is retried into a skip.
- Setup regression: the action validates a numeric repository pin, installs requested targets in
  that release, and fails if another compiler leaks into its isolated inventory or the active
  compiler differs. The old duplicated target installs are unnecessary with the exact release.

## Budget and eviction

The API snapshot contains 17 entries, all on main, totaling 10,699,227,445 bytes (9.965 GiB).
The initial latest dependency/config cohort plus two tool binaries costs 4,652,193,536 bytes; older
cohorts occupy approximately 6.05 GB. These are compressed archive sizes, not target disk sizes. A second
[inventory after the newer main run](ci-cache-inventory-after-main-2026-10.json) has 16 entries
and 10,291,622,361 bytes. Six entries for the rolling-stable-expanded environment were added and
seven older entries disappeared. That is consistent with quota eviction during new writes;
the API does not prove the deletion cause. No entry was deleted by this task.

Use a conservative 10,000,000,000-byte planning budget even though the observed quota permits
about 10 GiB. Do not add target caches for every short job. Priority follows measured critical-path
cost and avoided compilation, not a uniform allocation per job.

| Family | Latest observed compressed GB | Planning allocation GB | Reason |
|---|---:|---:|---|
| Linux xtask, GPU, feature tests, live smoke, wasm, tool archives | 3.728 | 4.0 | Existing useful isolated families; fast checks/plan and required feature/platform graphs |
| Linux workspace + nested consumers | 0.924 before nested writer | 1.7 | Ordinary PR critical path ends at test-nested; one writer adds generated/consumer dependencies |
| Native Windows workspace + consumers | Not yet available | 1.5 | Extended critical path: 25:17 first compile plus 8:35 nested execution/compilation |
| Native macOS workspace + consumers | Not yet available | 1.8 | Extended job 39:05, including 11:06 test compile and 12:15 nested work |
| Headroom/older dependencies | — | 1.0 | Lock/config transition and LRU churn |

Allocations are an acceptance/planning target, **not an implemented byte quota or measured native
size**. Github LRU is the storage bound. Without deleting entries, ordinary immutable Actions
caches cannot guarantee a strict generation count or keep every allocated family warm. This PR
limits avoidable accumulation by removing rolling-toolchain generations and duplicate binary
payloads, choosing one writer per family, and selecting native dependency folders explicitly.
Manifest/dependency changes can still create generations, and eviction can cause a cold run.
Every cache is optional: a miss rebuilds and runs the same checks, so eviction affects cost and
latency without changing correctness or suppressing a contract failure.

After the first successful trusted-main extended run, inspect `gh cache list --json key,sizeInBytes`
and compare the newest useful families with these allocations. If they exceed the planning
budget, reduce the native selection or undo native writes rather than deleting other families
without authorization. Main-only policy means a pre-merge PR cannot seed or measure these new
main-scoped entries. This is a known pre-merge validation limit, not a warm-cache result.

Registry/git stay alongside each dependency family under rust-cache's package cleanup. Splitting
sources into a separate shared cache would save duplication, but would add restore/save steps and
custom ownership without an archive-content measurement demonstrating the net benefit. No nested
root is shared with the outer Cargo target. No sccache, build artifact transfer, paid runner or
service is introduced: cache key drift and missing native producers are cheaper established fixes.

## Validation and before/after limits

Before: cold extended run 36864134553 succeeded in 49.57 wall minutes / 305.95 runner minutes;
36894848200 succeeded in 51.42 / 316.20. All twelve Rust restores missed in both. Main and ordinary
PR warm observations are in the baseline; lane and commit differences preclude a cache-only
speedup claim.

Local validation completed: `cargo xtask checks --strict` passed; `cargo test -p xtask --locked`
reported 31 passed, zero failures; actionlint 1.7.12 passed; zizmor reported no findings (22
self-repository syntax ignores explain the pinned actionlint compatibility; the 26 pre-existing
suppressions remain). A smoke test executes the composite action's actual pin/inventory scripts:
an isolated installed 1.98.1 exits zero; adding an unrelated toolchain makes setup exit one.
The pin script emits exactly `channel=1.98.1`. A command comparison against `origin/main` found
only five duplicate rustup-target setup commands removed and five size telemetry commands added;
existing run commands and lane conditions, needs and timeouts were unchanged. Existing negative
external-consumer tests `plugin_factory_requires_the_canonical_scene` and
`plugin_teardown_requires_unsafe` passed against their shared, populated consumer target; compile-fail
families likewise still exercise compiler rejection. This confirms real consumer compilation and
failure detection rather than merely checking generated files exist. `check-changed`
and Actions results follow below. The first full local gate passed 608 workspace tests (10
pre-existing skips) but could not find clang for the Android runner. The already-installed LLVM
was added only to the verification process PATH, with a worktree-local llvm-ar alias for its `ar`
command, and the gate was repeated after updating to current main. It passed: 608 workspace tests,
10 unchanged skips, strict docs/doctests, available Windows/macOS/Android/iOS backend and desktop
cross checks, Android runner and wasm checks. Local iOS-runner execution and the Linux platform
suite remain host-limited and are covered by the unchanged Actions jobs. No system setting changed. A PR run can prove
setup, targets, preserved checks and cold performance. A comparable new warm-main run requires
merging this cache policy; merging is outside the authorized task. Do not report a projected saving
as measured acceleration.

Rollback: restore the previous setup/cache configuration, or disable the two native writers first
if storage churn increases ordinary PR latency. Keeping the isolated pin setup while reverting
native writes preserves the confirmed fingerprint fix. No cache cleanup or repository setting
change is required for rollback.
