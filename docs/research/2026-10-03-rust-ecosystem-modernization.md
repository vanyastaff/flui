# Rust 1.99 ecosystem modernization audit

Research date: 2026-10-03. Baseline: Rust 1.99.0 and main at `465b8e13d`.

## Coverage and limits

This report records a verified set of modernization changes, not an exhaustive
functional or architecture audit of every crate or every module. Workspace-wide
Clippy, tests, documentation and dependency checks do not establish that all
subsystems have been inspected for design problems.

The initial pass changed files in 18 of the 27 members directly under `crates/` and
`packages/`, counted with `git diff --name-only origin/main...HEAD` against their
manifests. The following first-pass table is historical; renewed source coverage
appears below. The facade, examples and xtask are additional workspace members.

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
| Which retained UTF-8 loops can std replace? | [str boundary APIs](https://doc.rust-lang.org/stable/std/primitive.str.html#method.floor_char_boundary), stable since 1.91, and [Rust 1.91 notes](https://releases.rs/docs/1.91.0/) | Use floor/ceil_char_boundary for byte clamping, preserving direction. Keep grapheme segmentation and exact UTF-16 errors. |
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

## Renewed source coverage

The first modernization pass did not review every crate. The renewed pass
inventoried manifests and source modules across the 27 framework crates and
official packages, then read selected implementation seams and their production
callers. This table records those reads and their limits; it is not a claim that
every line, feature combination or platform behavior was audited.

Discovery used `rg --files` on each source root, manifest and architecture reads,
and scoped searches for old std substitutes, UTF-8 loops, erased ownership,
unchecked counters, collection copies, numerical cancellation and debug-only
validation. Search hits were inspected with callers. The detailed working
inventories are `target/audit-values-contracts.md`,
`target/audit-render-catalog.md`, `target/audit-runtime-tooling.md` and
`target/audit-facade-tooling.md`; these are local evidence rather than published
artifacts.

| Crate/package | Implementation seams read; resulting choice | Not fully examined in this pass |
|---|---|---|
| flui-foundation | ClaimSlot registration, delivery, abandonment and owner Drop; executor cloning leaves locks and failures retain opaque ownership. Preserve request/reclaim state machine. | Complete geometry, diagnostics, deep platform teardown |
| flui-macros | Runtime crate-path resolution and Diagnosticable field bounds; correct SDK-first resolver comments. Keep syn/quote. | Every derive expansion and routing order |
| flui-platform-api | IME projection, exact UTF-16 conversion, transfer limits and offer identity. Use std ceil_char_boundary only for UTF-8 clamping. | LockArbiter callback competition and native consumers |
| flui-protocol | Version canonical parsing, handle vocabulary/schema and action request shape. Keep dependency-free default error implementation and canonical wire spellings. | Full unknown-name/schema evolution |
| flui-log | Filter resolution, subscriber ownership and selected backend/redaction boundaries. Keep tracing EnvFilter and composition-root ownership. | Full privacy corpus, native shutdown/callsite lifecycle |
| flui-semantics | Parent/child mutation, dirty publishing and AccessKit projection. Reject wrong/missing parents before detaching. | Arbitrarily corrupted graphs and complete deep traversal |
| flui-sdk | Complete re-export source and evolving manifest contract. No duplicate wrappers added. | Every downstream feature combination |
| flui-layer | Immutable graph construction, rectangle damage union and selected differ/LIS logic. Preserve typed content identity and append-only topology. | Full effect damage/follower propagation |
| flui-painting | UTF-8 caret/word boundaries, Parley font ownership and glyph registry. Use std boundary APIs; copy plugin font bytes into concrete host ownership while ordinary registries retain shared sources. | Complete typography/style cache keys and font-feed failures |
| flui-engine | Uniform/buffer/texture ownership and plugin font-source transition; prior stream/dash/numerical work retained. Keep wgpu/lyon/etagere. | Every shader, replay/filter branch and raster mailbox path |
| flui-rendering | Tagged erased protocol values, generational tree resolution and dirty membership/drain. Keep domain enums and safe dynamic subtree borrowing. | Complete virtualization and pipeline/protocol methods |
| flui-objects | FittedBox layout, paint and hit-test transform agreement, scroll/text architecture. No replacement chosen by age alone. | Every render object's intrinsics/semantics/layout |
| flui-animation | Animation/Curve bounds, spring retargeting and smoothing; prior numerical/ticker fixes. Correct obsolete widget and performance documentation. | Complete curve catalog and proxy/listener competition |
| flui-interaction | Prediction sampling/extrapolation and One Euro filters. Correct unsupported Kalman claims; retain owner-local arena state. | Complete recognizers, focus, teams and mouse routing |
| flui-widgets | Router parsing/encoding and image decode coalescing/subscriber lifetime. Keep domain path parser and shared future contract. | Most widget/form/navigation/text/scroll state machines |
| flui-assets | Asset keys/handles, cache/registry admission and locked Moka initializer implementation. Coalesce cold registry loads and remove always-empty registry stats. | Fresh full decoder/network protocol review |
| flui-material | Navigation bar count/selection and controller policy. Enforce configuration in release profiles. | Complete themes, drawers, decoration and messenger |
| flui-cupertino | Tab scaffold controller/build and standalone bar selection. Validate current bar and preserve recovery; correct lazy-builder description. | Other routes, buttons, themes and navigation bars |
| flui-app | Composition/module map, retry backoff and plugin render callback. Wire explicit plugin lifetime and font transition obligations. | Every native run loop, lifecycle and embedder adapter |
| flui-cli | Complete process probe reader and iOS version parsing. Transfer captured output with mem::take; inherit shared serde. | Build/scaffold/deploy/watch command bodies |
| flui-hot-reload | Dynamic loader, scene driver, image ownership, artifact stamps and render hook. Separate polling/building and preserve subsecond revisions. | Every dispatch registry race and live native image reload |
| flui-platform | Complete Task/executor source and ready-task consumers. Remove unnecessary result bounds; keep spawn bounds. | Native OS backends and event translation |
| flui-runtime | Execution ownership prefix, held-input prefix and architecture. Preserve realm-bound synchronous frame topology. | Full shutdown/replay/presentation/telemetry bodies |
| flui-scheduler | Task recovery seams and complete ID generator. Use checked atomic try_update for sticky exhaustion. | Full pacing, budgets and telemetry algorithms |
| flui-testing | Architecture, replay prefix and widget harness root/finder resolution. Remove the stale render-root cache and verify replacement/error/recovery. | Complete text-store kit, accessibility and host bootstrap |
| flui-view | Seq implementation, StateCell prefix, reload trait and prior reactive recovery work. Inherit slab and preserve owner-local state. | Complete reconciliation, contexts, elements and sliver services |
| flui-devtools | Profiler history/config and timeline guard/clear/export ownership. Respect zero capacity and invalidate old guards at clear. | Full agent endpoint, tracing interleavings and inspector |

The facade exports, Cargo profiles/lints/configuration and selected xtask
execution, classification, dependency, documentation and device-capture code
were read separately. Std io::pipe/LazyLock and owned UTF-8 conversion were
already present. Platform linkers and optimization profiles were retained
without inventing performance claims. Selected counter/web/scene examples were
read; native deployment and browser execution remain separate validation.

Focused regressions were verified independently of this inventory.
Before the final gate, nine targeted contract families passed in
`target/audit-contracts-recovery-fixed.log`, and the Cupertino navigation family
passed in `target/audit-cupertino-fixed.log`. Those focused runs do not replace
the final changed-crate gate.

## Renewed regression evidence

- Restoring the old ClaimSlot source makes the reentrant clone child reach its
  timeout and hostile wake/destructor children abort. The corrected family
  passes its 15 isolated scenarios (`target/audit-claim-slot-fixed.log`,
  `target/audit-claim-slot-old-source.log`). Exceptional opaque retention
  preserves progress and failure priority; it is not a guarantee against
  competing panics inside user-defined aggregate destruction.
- Old Task bounds reject Rc, PhantomPinned and borrowed results at compile time.
  Old developer history fails zero-capacity, stale-guard and fresh-guard rows.
  Old scheduler identity allocation admits IDs after exhaustion. Old registry
  admission starts duplicate loads and fails cancellation/error sharing
  (`target/audit-task-old-source.log`, `target/audit-history-old-source.log`,
  `target/audit-identity-old-source.log`, `target/audit-coalescing-old-source.log`).
- The navigation families pass with debug assertions disabled specifically in
  both catalog packages. Under the same Cargo profile overrides, original
  sources fail all four Material invalid-configuration rows and both Cupertino
  selection/error-recovery rows. This exercises release validation semantics;
  it is not a claim that the complete release/LTO profile was built
  (`target/audit-navigation-no-debug-fixed.log`,
  `target/audit-navigation-no-debug-old-source.log`).
- Replacing explicit plugin font ownership with ordinary shared retention calls the retired source when rasterizing.
  Coarse timestamps lose a same-second artifact replacement. The old harness
  cannot find the newly mounted RenderPadding
  (`target/audit-font-owning-policy-mutant.log`,
  `target/audit-coarse-revision-mutant.log`,
  `target/audit-cached-render-root-mutant.log`).
- Required-GPU paragraph readback passes with three font transition rows.
  Removing source admission fails repaint from NoDamage in all three; keeping
  repaint but removing atlas replacement fails actual pixels in all three
  (`target/audit-font-transitions-fixed.log`,
  `target/audit-font-no-repaint-mutant.log`,
  `target/audit-font-stale-atlas-mutant.log`). The headless path shares the
  production selector; windowed method wiring received source review.
- Five final recovery families pass, including the actual scene-hook reset
  acknowledgement and public harness replacement/error/recovery
  (`target/audit-new-recovery-final.log`). Independent source reviews found no
  remaining concrete blocker in the new claim, coalescing, navigation, font
  transition and harness fixes.

The first renewed full gate passed 625 of 626 tests and exposed a contract
regression in blanket font copying: the ordinary registry no longer held the
shaper cache source alive, so pruning changed its blob ID. The existing
`a_held_blob_keeps_its_keys_across_a_prune` test was retained unchanged. Admission
now copies only in the plugin atlas; ordinary admission preserves shared source
ownership. Independent review checked both constructor paths and atlas policy
changes. Final verification of this correction is recorded below.

## Renewed final verification

The corrected final production source passed the following commands with one
compiling worker and one nextest test thread. Logs are local evidence in the
ignored target directory, rather than CI or native-platform execution claims.

| Command | Observed result | Log |
|---|---|---|
| `cargo xtask checks` | Source, manifest, architecture, documentation and registry gates passed | `target/audit-checks-final.log` |
| `cargo xtask check-changed` | Full workspace Clippy and engine testing Clippy passed; nextest 626 passed, 10 configured skips; strict rustdoc and doctests passed; available Windows/macOS cross-Clippy passed | `target/audit-check-changed-final.log` |
| `cargo xtask deps` | Bans, licenses, sources and advisories passed; cargo-shear found no issues | `target/audit-deps-final.log` |
| `FLUI_REQUIRE_GPU=1 cargo nextest run -p flui-engine --features testing --lib --test raster_backpressure_allocation --locked --no-fail-fast --test-threads 1` | 59 passed, zero skipped | `target/audit-gpu-final.log` |
| `cargo nextest run -p flui-painting --lib --tests --locked --no-fail-fast` | 36 passed, zero skipped, including the unchanged cache-prune identity test and the owning-source regression | `target/audit-font-policy-fixed.log` |

The doctest result summaries record 617 passed and 390 ignored; ignored
examples were not executed. The font policy counterfactual replaced
`SwashRasterizer::with_owned_fonts` with ordinary source retention: the
owning-source regression failed because rasterization called the retired
source (`target/audit-font-owning-policy-mutant.log`). The earlier full-gate
identity failure is preserved in
`target/audit-check-changed-prune-regression.log`; no existing identity
assertion was weakened. Required-GPU source transitions passed again after
the conditional policy repair. Independent source and report reviews found
no remaining concrete blocker in the selected changes.

Android, iOS and wasm targets are absent on this host. The Linux platform
suite needs Xvfb and was not run; its native non-UTF-8 library-path fixture
was not executed here. Windows/macOS cross-Clippy establishes compilation,
not native event translation or plugin reload behavior. Android live reload,
macOS GUI execution and exhaustive per-feature combinations remain unverified.
The per-crate table identifies implementation areas still needing deeper
behavioral review; passing these gates does not remove that distinction.
