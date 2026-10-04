# Rust 1.99 ecosystem modernization audit

Research date: 2026-10-03. Baseline: Rust 1.99.0 and main at `465b8e13d`.

## Coverage and limits

This report records a verified set of modernization changes, not an exhaustive
functional or architecture audit of every crate or every module. Workspace-wide
Clippy, tests, documentation and dependency checks do not establish that all
subsystems have been inspected for design problems.

The branch changes files in 18 of the 27 members directly under `crates/` and
`packages/`, counted with `git diff --name-only origin/main...HEAD` against their
manifests. The facade, examples and xtask are additional workspace members.

| Scope | Members and inspected areas |
|---|---|
| Substantive subsystem work | `flui-foundation`: notification ownership, read adapters and geometry; `flui-scheduler`: async polling, wake delivery, ticker resolution and frame completion; `flui-assets`: admission, loaders, cache ownership and runtime bridging; `flui-engine`: recording allocations, texture reuse, scale calculations and dashed contours; `flui-view`: reactive loan recovery. These are selected areas, not complete crate audits. |
| Targeted numerical work | `flui-animation`: friction simulation; `flui-rendering`: constraint diagonals. |
| Local lint/API-consumer adaptations | `flui-app`, `flui-cli`, `flui-hot-reload`, `flui-interaction`, `flui-objects`, `flui-platform`, `flui-runtime`, `flui-testing`, `flui-widgets`, `flui-material`, `flui-cupertino`. File changes here do not imply a full functional review. |
| No crate-specific modernization changes | `flui-layer`, `flui-log`, `flui-macros`, `flui-painting`, `flui-platform-api`, `flui-protocol`, `flui-sdk`, `flui-semantics`, `flui-devtools`. Global checks and downstream compilation cover them, but they have not received a complete independent architecture audit in this work. |

## Sources and applicability

The [Rust 1.99 changelog](https://releases.rs/docs/1.99.0/) and
[This Week in Rust 671](https://this-week-in-rust.org/blog/2026/09/30/this-week-in-rust-671/)
were read with Firecrawl. Keenable supplied source discovery and targeted primary-document
reads. A stabilization PR mentioned in TWIR is not evidence that its API is available in
1.99: Allocator, Box::take, SyncView and vec_try_remove announcements are not used here.

| Question | Evidence | Decision for FLUI |
|---|---|---|
| Which newer standard APIs remove unnecessary work? | [String::from_utf8_lossy_owned](https://doc.rust-lang.org/stable/std/string/struct.String.html#method.from_utf8_lossy_owned), stable since 1.99 | Consume owned process-output/decoded Vec buffers. Keep borrowed conversion for slices. Allocation reuse is an implementation opportunity, not a public guarantee. |
| Can task ownership avoid frame-path allocations? | [Waker::from(Arc)](https://doc.rust-lang.org/stable/std/task/struct.Waker.html#impl-From%3CArc%3CW%3E%3E-for-Waker) and [will_wake](https://doc.rust-lang.org/stable/std/task/struct.Waker.html#method.will_wake); FLUI's existing counting allocator | Retain one waker per task. It contains a Weak driver, so a retained external waker cannot retain the realm. Require zero allocations in the existing warm ready-task oracle. |
| What causes resource growth and repeated GPU allocations? | [GPUI memory investigation](https://sot.dev/cutting-a-rust-gpui-launcher-idle-memory.html), independently checked against FLUI's pool admission code; [engawa texture ownership](https://docs.rs/engawa-wgpu/latest/engawa_wgpu/) | Correct FLUI's own full-pool admission: evict the oldest returned idle texture, permitting the resized working set to become reusable. No environment-wide Vulkan driver filtering or allocator tuning is inferred from another application's measurements. |
| Are newer libraries preferable to existing implementations? | [SIMD survey](https://shnatsel.github.io/state-of-simd-rust-2026/) and [glam feature documentation](https://docs.rs/glam/0.33.7/glam/#feature-gates) | Retain glam and its SIMD/std behavior, compile only used floating families, and enable bytemuck only in the engine. Do not add another SIMD crate without a measured kernel bottleneck. |
| Do Cargo/Clippy settings reflect the current compiler? | [Cargo changelog](https://doc.rust-lang.org/nightly/cargo/CHANGELOG.html), root profiles and workspace lint table | Existing resolver 3, edition 2024, selective graphics backends, line-table development debug info, dependency optimization and thin release LTO are already modern. Audit additional lints before making them merge requirements. |
| Can invariants replace repeated validation? | [Rusty thoughts on Parse, don't validate](https://eli.thegreenplace.net/2026/rusty-thoughts-on-parse-dont-validate/) and FLUI's foundation/runtime architecture | Existing NonZero IDs, geometry types, lifecycle capability acquisition and realm ownership already follow this principle. A wholesale API rewrite needs a concrete invalid state to eliminate. |

## Changes selected from code evidence

- Remove unconditional smallvec serde and glam bytemuck features from the foundational graph;
  enable them at consumers that require serialization or GPU POD conversions.
- Consume owned byte buffers with the new standard conversion API.
- Retain a stable task waker and remove the allocation paid on every async poll.
  The waker allocation now lives with each active task, including dormant tasks; this trades
  retained per-task storage for avoiding per-poll allocation. No idle-memory reduction is claimed.
- Preserve unpaid frame-delivery demand through hook failures and replacement. Successful
  older deliveries cannot acknowledge newer demand. Bound same-thread compensation and
  preserve the first panic without dropping opaque secondary panic payloads during recovery.
- Enable selected Clippy nursery checks for redundant clones, needless collection and unread
  collections, and review futures larger than 8 KiB. Keep the nursery group selective because
  its checks have different applicability and false-positive profiles.
- Adapt the bounded offscreen texture pool to changed descriptors after resize.
- Stream cached path vertices through transformation into the destination segment, eliminating
  the temporary Vec previously created for each cache hit. Preserve ordered SSAA and advanced
  blend isolation; this does not make those entire recording paths allocation-free.

The initial changes above are crate-local implementation and feature-selection changes. They add no unwired public
surface and change no cross-crate ownership contract, so they do not need a new ADR.

## Additional Clippy policy and numerical behavior

The audit uses the exact [Clippy 1.99 lint catalog](https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html),
rather than enabling the entire nursery or restriction groups. Two further workspace checks
are selected: `imprecise_flops` exposes avoidable cancellation and squared-length range loss;
`debug_assert_with_mut_call` rejects mutations whose execution depends on debug assertions.
These are improvements adopted with the current compiler, not claims that the lints or the
standard numerical functions first appeared in Rust 1.99.

`FrictionSimulation` uses `exp_m1` for displacement and `ln_1p` for inverse arrival time.
Independent counterfactual runs restore each old expression separately: a valid drag one
representable step below one produces zero frame displacement or zero arrival time instead
of the constant-velocity limit. Column norms and maximum layout diagonals use `hypot`.
Decomposition retains a representable direct determinant before division; normalized products
are a fallback at overflow/underflow, because early normalization can erase the smaller
component of an anisotropic shear. Separate mutations fail the large/reflected-scale cases
and the signed anisotropic cases. The fixed foundation, animation, rendering and scheduler
suite passed 95 tests (`target/modernization-numerics-scheduler-fixed.log`).

Other audited candidates remain selective. `suboptimal_flops` proposes fused operations that
change rounding and requires a separate numerical contract. `mutex_atomic` cannot infer the
critical sections needed around shared state. `iter_with_drain` can discard the retained
capacity of frame queues. `future_not_send` conflicts with deliberately local UI futures,
and `non_send_fields_in_send_ty` conflicts with audited platform wrapper guarantees. A lint
finding is a reason to inspect the ownership or numerical behavior, not to rewrite it blindly.

## Asset loading contracts

The asset audit found concrete mismatches between the public API and production
behavior. `Asset::validate` advertised early rejection but registry loads never
called it. Validation now precedes key computation and cache access, including
cache hits; rejected descriptors leave previously accepted cached data intact.
This makes admission deliberate without making `get` invent a descriptor to validate.
[ADR-0105](../adr/ADR-0105-asset-validation-and-bridge-progress.md) records the contract.

The bridge formerly accepted an ambient current-thread Tokio handle even when only
entered: an external executor could poll the completion future forever while Tokio's
spawned task remained undriven. Automatic resolution now selects ambient multi-thread
runtimes or a registry-owned worker. Explicit injection remains a host promise to
keep the runtime alive and driven. Registry-local fallible initialization returns
`AssetError::Io`, leaves the runtime slot empty on failure, and permits retry;
non-blocking shutdown and registry ownership remain intact.

The generic `AssetLoader` abstraction had no production caller. Generic file loading
read bytes and then always failed, generic network loading always failed, and
`MemoryLoader` duplicated an unused storage boundary. These surfaces were removed.
Concrete `Asset::load` implementations select their source and decoder directly:
file assets now use `BytesFileLoader`, embedded assets own bytes through `from_bytes`,
and the network bridge fetches with `reqwest` before decoding with `image`. No new
decoder trait or storage layer was added. Optional image/network modules and types
require the corresponding features rather than exposing unconditional-error stubs.
[ADR-0107](../adr/ADR-0107-asset-byte-sources-and-decoding.md) records this API change.
The consumer audit used `rg -n 'ImageAsset|NetworkLoader' --glob '*.rs' --glob 'Cargo.toml'`;
widget `asset-images` already enables `flui-assets/images`, and `network-images`
additionally enables `flui-assets/network`.

Registry APIs also imposed seven unnecessary decoded-data `Clone` bounds, although
caches and handles already share `Arc` ownership. Those bounds were removed; explicit
`clone_data` keeps its copying requirement. The former extension `ptr_eq` compared
keys despite promising allocation identity. The inherent `AssetHandle::ptr_eq` now
uses `Arc::ptr_eq`, distinguishing an old retained value from a same-key reload.

Public consumer regressions cover validation rejection before loading and on cache
hits, file/embedded equivalence, invalid and missing sources followed by recovery,
non-Clone data ownership/release/reload, and actual image decoding while an ambient
current-thread runtime is entered but undriven. Named bridge/font cases continue
after ordinary panics. A private seam covers deterministic runtime-construction
failure followed by genuinely executed work after retry.

## Additional ownership and rendering audit

The subsequent AsyncDriver ownership audit reproduced eight failing subprocess cases,
including lazy/eager poll-plus-destructor aborts and failed-spawn orphans. Polling now borrows
the future into its catch boundary and retains that opaque future before resuming the original
panic. Unwind-time token destruction similarly detaches without running user destruction;
spawn establishes rollback ownership before calling its hook. All twelve subprocess cases
pass, including ordinary retirement failures and continued sibling progress. Exceptional
retention keeps captured resources and nested tokens alive. It cannot contain competing panics
inside user poll locals or multiple fields of an ordinary destructor.

Distinct atlas image IDs also currently force
batch changes; sharing an atlas page alone does not prove they can share all replay bindings.

## Initial validation

Commands and their full output are retained under the worktree's gitignored `target/`.

- `cargo xtask --help`: repository commands inspected before work.
- `cargo xtask deps`: bans, licenses, sources, advisories and cargo-shear passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed with
  `redundant_clone`, `needless_collect`, `collection_is_never_read` enabled and an
  8 KiB `large_futures` threshold. No new lint suppression was required.
- `cargo tree -p flui-foundation --no-default-features --edges normal --locked`: foundation's
  normal graph no longer contains serde or bytemuck; engine and animation opt-ins were checked.
- Isolated Clippy runs of foundation with only `serde`, and animation with `serde` over all
  targets, passed with `-D warnings`. Workspace-wide feature unification is not required for
  their serialization features to build.
- `cargo xtask checks`: passed, including documentation links, workspace/reach/module
  contracts, globals, file-length, shader validation and changelog validation.
- `cargo xtask check-changed`: passed. The initial pass ran all 612 selected tests (10
  suite skips), strict rustdoc, doctests, engine testing-feature linting, platform a11y
  typechecks on Windows/macOS, Windows CLI and desktop MCP on Windows/macOS. Android,
  iOS and wasm targets were unavailable; the Linux platform runtime suite requires Xvfb
  and was not run on this Windows host. CI remains the proof for those lanes.
- `cargo clippy -p flui-platform --all-targets --target aarch64-apple-darwin --locked -- -D warnings`:
  passed. This is cross-target typechecking and linting, not a macOS runtime run.
- Restoring the baseline async driver makes `poll_ready_costs_zero_extra_allocations_once_warm`
  fail with 256 allocations across four pumps of 64 ready tasks, against the new requirement
  of zero. Dormant cases still report zero. The source was restored after this negative run.
- `cargo test -p flui-scheduler --tests --lib --locked`: all 17 tests passed. The warm
  allocation oracle reports zero for both dormant sizes and 64 ready tasks over four pumps.
- Replacing receipt identity checking with unconditional successful acknowledgement makes
  four wake-delivery rows fail: both older-success/newer-failure races and both reentrant
  demand scenarios. Restoring the implementation makes the family pass again.
- Reentrant compensation rereads the installed hook. Restoring the earlier retained-hook
  implementation fails the replacement row on both scheduler and task wakes, and the
  competing capture-retirement row. The corrected implementation passes the whole scheduler
  suite again; retirement closes active bookkeeping before preserving its first failure.
- `FLUI_REQUIRE_GPU=1 cargo nextest run -p flui-engine --features testing --lib --test raster_backpressure_allocation --locked --no-fail-fast --test-threads 1`:
  59 tests passed, zero skipped. This is local GPU execution; current CI typechecks the
  feature-gated GPU suites and does not run a dedicated GPU job.
- Restoring baseline batching and texture-pool sources makes the warm path test fail with
  130 allocations for 128 draws and the resized pool identity row fail. Restored streaming
  records those draws with two arena-growth allocations (2,899,968 bytes versus baseline
  3,956,736); this is an allocation observation, not a throughput claim.

## Continued verification

- Asset tests passed without optional features (4), with images (7), network (5), and full (8). Reverting validation or allocation identity fails the relevant consumer row; accepting an entered but undriven ambient runtime reaches the bridge test's ten-second failure bound. Reintroducing decoded-data `Clone` bounds fails compilation of the non-Clone consumer (`target/modernization-assets-*-fixed.log` and corresponding baseline logs).
- Ticker tests passed 19/19. Its 18-child recovery matrix distinguishes the prior notifier, consuming callbacks, payload competition, waker ownership and telemetry behavior (`target/modernization-scheduler-ticker-fixed.log`, `target/modernization-ticker-baseline.log`).
- Frame completion tests passed 19/19, including 15 child scenarios for delivery, abort, teardown, retirement and reporting. The prior implementation aborts hostile executor/teardown/cancellation cases and loses chronological priority under telemetry failures (`target/modernization-frame-completion-*.log`).
- View tests passed 40/40. Four released-loan rows fail under the old graph implementation; retaining the new graph but restoring the old typed read adapter still aborts the two read scenarios. The adapter must expose failure before the graph finalizes its loan, while keeping the original payload outside that boundary (`target/modernization-view-loan-baseline.log`, `target/modernization-read-bridge-*.log`).
- Independent agents reviewed functionality, failure scenarios and misleading claims during verification. They caught premature anisotropic normalization, an invalid large-circle test oracle, a misleading cache-lifetime test name and stale signal-release documentation. The corrected GPU circle row uses tiny scale and a large local radius to produce a representable ten-pixel device radius; restoring squared CPU column norms loses the figure. Removing dash advancement refusal retains an invalid primitive's visible prefix. These are actual pixel differences, not implementation predicates (`target/modernization-circle-norm-baseline.log`, `target/modernization-dash-progress-baseline.log`).
- Selected workspace Clippy checks passed over all targets with `-D warnings` (`target/modernization-clippy-final-components.log`). The numerical required-GPU suite passed 59/59, zero skipped (`target/modernization-gpu-final-fixed.log`).
- Final peer review found that notifier diagnostics could interrupt healthy listeners after a caught failure. Reporting now borrows the original payload inside its own catch boundary and retains subscriber failures too. Restoring only the old reporting fails the public `notifier_ownership_and_recovery` family; the corrected foundation suite passed 22/22 (`target/modernization-notifier-telemetry-baseline.log`, `target/modernization-notifier-telemetry-fixed.log`).
- Dependency bans, licenses, sources, advisories and cargo-shear passed again (`target/modernization-deps-final.log`). macOS platform Clippy passed over all targets with the final lint policy (`target/modernization-platform-macos-final.log`); this remains typechecking, not a macOS execution result.
- Closed-contour review also exposed two caps at a seam covered by a dash. The engine closes one uninterrupted fragment or merges covered first/last fragments; actual gaps retain their caps. Restoring the prior tessellator fails all three covered-seam pixel rows while the gap row passes (`target/modernization-dash-seam-baseline.log`). Four independent named rows compare the miter corner with a solid stroke and verify an on-dash edge. The final required-GPU suite after this correction passed 59/59, zero skipped (`target/modernization-gpu-seam-final.log`). A separate agent reviewed the restored seam implementation and found no concrete blocker.
- After the final reporting and seam corrections, `cargo xtask check-changed` completed with exit 0: 620 tests passed, 10 configured suite skips; workspace and engine-testing Clippy, strict rustdoc including private items, doctests, platform a11y typechecks on Windows/macOS, Windows CLI and desktop MCP on Windows/macOS all passed (`target/modernization-check-changed-final.log`). Android/iOS/wasm targets and Linux platform execution remain unavailable locally. `cargo xtask checks` also passed (`target/modernization-checks-final.log`). The available macOS lanes are compile checks; no macOS runtime execution is claimed.
