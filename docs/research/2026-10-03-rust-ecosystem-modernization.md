# Rust 1.99 ecosystem modernization audit

Research date: 2026-10-03. Baseline: Rust 1.99.0 and main at `465b8e13d`.

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

These are crate-local implementation and feature-selection changes. They add no unwired public
surface and change no cross-crate ownership contract, so they do not need a new ADR.

## Unresolved architecture findings

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

## Validation

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
- `cargo xtask check-changed`: passed. The final run passed all 612 selected tests (10
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
