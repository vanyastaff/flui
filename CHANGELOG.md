# Changelog

All notable changes to the FLUI workspace are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
FLUI is pre-release and not published to crates.io; entries are grouped under
`[Unreleased]` until a first tagged release cuts them over. Workspace version:
`0.2.0` (all crates share `[workspace.package].version`). Fine-grained phase
history lives in [`docs/ROADMAP-TRACKER.md`](docs/ROADMAP-TRACKER.md); this
file records the repo-consumer-visible summary.

## [Unreleased]

### Added

- **Runtime.1 conformance registry and public-API freeze gate** (#576):
  `docs/runtime-conformance.toml` is the machine-readable inventory of every
  normative ADR-0027/0037/0029/0039 clause (state verified against current
  code, evidence or an owning issue) and of every public runtime/platform/
  scheduler/raster surface, with a stability classification (stable-candidate
  / experimental / transitional / removal-target), thread affinity, actual
  owner, and failure semantics. `scripts/check-runtime-conformance.sh`
  validates citation existence, evidence rules (documentation is never
  implementation evidence), forbidden retired identifiers, and registration
  nets for ambient singletons and SP-6 lock exemptions. Wired as
  `just runtime-conformance-check`, into `just ci`, and into CI's `checks`
  gate.
- **Generational presentation addressing** (#585): every frame is now
  addressed by `PresentationAddress` (`realm_id` + `presentation_id`,
  promoted to `flui-foundation`) instead of a bare `presentation_id`, since
  two different `UiRealm` incarnations can mint an identical `PresentationId`
  and only the full pair disambiguates a frame's owner. `PlatformWindow::id()`
  gives every window a stable identity; `SceneSnapshot` and every
  `RasterAck`/`PumpOutcome` variant (now individually `#[non_exhaustive]`)
  carry the full address; `RasterOwner` rejects a submit whose address
  doesn't match (`RasterSubmitError::AddressMismatch`) instead of accepting
  it on a partial (`presentation_id`-only) match. `WindowRegistry` is
  flui-app's sole native-window-to-presentation map. A new coalesced
  `SurfaceState` slot carries `SurfaceOutdated`/`DeviceLost` outside the
  lossy telemetry ack channel, since both are recovery-critical.
- **`OwnerPlatform` capability and typed owner-lane proxy** (#577, ADR-0039
  slice 2): `OwnerPlatform` (a `!Send + !Sync` owner-thread capability,
  minted only by a backend) and `PlatformProxy` (a `Clone + Send + Sync`
  cross-thread request capability) replace the ambient `&dyn Platform` every
  backend's `on_ready` used to hand out — the old shape stayed fully
  `Send + Sync` until a later trait split, so safe code on another thread
  could call an owner-affine method through it. `flui-foundation`'s new
  `ClaimSlot<T>`/`ClaimHandle<T>` (`Pending` → `Delivered` → `Claimed`, plus
  an `OwnerGone` terminal state and a `Future` impl) back `PendingWindow`'s
  at-most-once open-window reply, closing a window-leak-on-drop gap in the
  winit owner lane. `Platform::run` and its `on_ready` callback are now
  fallible (`anyhow::Result<()>`), so a bootstrap failure (window creation,
  GPU init, root-widget attach) stops the loop on every backend instead of
  running with no UI. `SharedPlatform` is the `Clone + Send + Sync`-safe
  residual of `Platform` a proxy can actually hold across threads.
- **Feature-selective facade, Material-first** (#574): `flui-material`
  (default), `flui-cupertino`, and the newly exposed `flui-localizations` are
  optional dependencies behind their own facade features; a module whose
  feature is off is absent from the graph, not an empty stub. `flui-hot-reload`
  leaves the production graph — optional in `flui-app` behind `hot-reload`,
  asserted absent from `flui-app`'s default `cargo tree` rather than assumed.
  The workspace's test-support crate is renamed to `flui-testing` (directory
  moved with history; `HeadlessBinding` keeps its name). `flui-app::theme` (`AppTheme`/
  `AppColorScheme`) is deleted outright per ADR-0042 — theming belongs to the
  design system (`ThemeMode` on `flui-material`'s `MaterialApp`), not the app
  framework. `just facade-combos` compiles each supported facade combination
  in isolation against `flui` alone, since a `--workspace` build proves
  nothing about the facade surface (feature unification would turn a broken
  combination green).
- **`flui-log` restored as a standalone, composition-only logging backend**
  (#571): `flui-foundation` no longer installs a process-global tracing
  subscriber (libraries emit through `tracing` only); `flui-log`'s
  three-outcome `SubscriberOwnership` (`Inherit`/`Auto`/`Install`) makes
  ownership explicit and never panics on an already-owned slot, replacing an
  `init()` that used to panic the moment a host process already owned one.
  Fixes three real defects found while restoring it: a stray `LevelFilter`
  ceiling that silently discarded events `RUST_LOG` had just selected, an
  illegal Apple subsystem identifier synthesized from an arbitrary app
  display name, and a logcat field-renderer separator that never fired when
  the message arrived first — untested because its tests lived inside a
  `cfg(target_os = "android")` island that never compiled anywhere.
  `docs/workspace-layers.toml` gains `allowed_dependents`, so a normal edge
  into `flui-log` from anything but `flui-app`, `flui-cli`, or the facade now
  fails `just inventory-check`.
- **`cargo-deny` CI job** (merge-blocking): the in-repo `deny.toml` was never
  executed in CI; the first wired run surfaced four real advisories —
  `anyhow` 1.0.102 unsound `downcast_mut` (RUSTSEC-2026-0190),
  `crossbeam-epoch` 0.9.18 invalid deref (RUSTSEC-2026-0204), and a
  `quick-xml` DoS pair (RUSTSEC-2026-0194/0195). Three fixed by lockfile
  bumps; the transitive quick-xml pair (build-time Wayland scanner) and the
  unmaintained `ttf-parser` notice carry documented ignores. `just deny`
  added.

- **CI gates**: integration tests now run in CI (previously `--lib` only —
  the Core.0/Core.2 exit-gate suites in `crates/*/tests/` were never
  executed); new `doc-test` job runs every rustdoc example; new `msrv` job
  verifies the declared MSRV floor; new advisory `miri` job checks the
  `flui-rendering` subtree arena (the workspace's densest `unsafe` hot spot);
  the `gpu-test` WARP readback suite is promoted from advisory to
  merge-blocking after 3 consecutive green full-suite runs.
- **Panic policy** ([`docs/PANIC-POLICY.md`](docs/PANIC-POLICY.md)):
  `Result` for caller-triggerable failures, `expect("BUG: <invariant>")` for
  internal invariants, enforced by `clippy::unwrap_used` at workspace level
  (tracked crate-level opt-outs burned down per quality wave).
- This changelog.

### Changed

- **Four miri-confirmed UB paths closed in the Android page-aligned
  allocator** (#584): `Drop` recomputed the dealloc layout as
  `capacity * size_of::<T>()`, which undershoots the real page-rounded
  allocation whenever `size_of::<T>()` doesn't evenly divide it — now stores
  the allocated byte size verbatim and reuses it. Zero-capacity construction
  and zero-sized `T` both reached the global allocator with a zero-size
  `Layout` (the former now uses a non-null dangling pointer instead of
  allocating; the latter is now a compile-time assertion). Separately,
  `alloc_page_aligned`'s rounding arithmetic could wrap to zero under
  release's `overflow-checks = false`, reaching the allocator with a
  zero-size `Layout` for a large enough request — now checked, returning
  `Err` instead of wrapping. All four verified by replicating the auditor's
  scratch-crate `cargo +nightly miri test` harness: reproduces the original
  UB before the fix, clean after.
- **Toolchain 1.96.1 → 1.97.1, MSRV 1.96 → 1.97.** `rust-toolchain.toml` no
  longer mirrors the MSRV: it is now explicitly the *development* toolchain
  (a pin at the floor hides new lints and codegen changes from the developer
  until CI surfaces them), while the floor stays a separate promise checked by
  the one `msrv` job. Three 1.97 stabilizations earn the bump: **v0 symbol
  mangling by default** (the release binary now carries 5110 v0 symbols and
  zero legacy — generic frames demangle with real type parameters instead of
  an opaque hash), **`build.warnings`** (below), and the integer
  bit-manipulation APIs (`bit_width`, `isolate_lowest_one`,
  `isolate_highest_one`, `lowest_one`, `highest_one`) that the render-node
  dirty-flag bitset is a candidate for. One new pedantic lint
  (`clippy::manual_assert_eq`) fired at two sites and was fixed.
- **CI warnings gate: `RUSTFLAGS=-D warnings` → `CARGO_BUILD_WARNINGS=deny`.**
  `RUSTFLAGS` is part of the rustc fingerprint, so the miri job — which needs
  a different value — got a disjoint `target/` cache and had to blank the flag
  wholesale, losing every other check with it. The Cargo knob is applied after
  compilation: measured, toggling it recompiles nothing while switching
  `RUSTFLAGS` recompiles. miri now sets `CARGO_BUILD_WARNINGS=warn` and shares
  the cache.
- **Release profile: `strip = "symbols"` → `strip = "debuginfo"`.** Stripping
  the symbol table left release builds unprofilable and crash reports
  unsymbolicated — `perf`, flamegraph, samply, Tracy and minidumps all resolve
  frames through it. Cost measured on `target/release/flui`: 3 437 096 →
  4 394 832 bytes (+935 KiB, +27.9%); DWARF is still dropped.
- **Performance overlay wired to `AppConfig`.** `show_performance_overlay` was
  write-only: the builder set it and nothing read it, while the layer, the
  rolling stats window and a real wgpu draw path all already existed. The chain
  is joined in `draw_frame` phase 4. Scope: it reports FPS and average frame
  time only — the renderer ignores the frame counter and the option mask — and
  the sampled interval is between *composited* frames, so it is a repaint rate.
- **`Defunct` is now an absorbing lifecycle state.** `Lifecycle::can_activate` /
  `can_deactivate` existed but were called only from tests; every mutator
  assigned unconditionally, so `Defunct → Active` was reachable through the
  public `ElementCore::activate` and would revive an element whose state was
  disposed. Both predicates are now asserted (debug-only) in `ElementCore` and
  in the hand-rolled `RootRenderElement`/`ErrorElement`.
- **`TextRange` consolidated into flui-types**, whose copy was already a strict
  superset; the canonical type gains `Clone, Copy, PartialEq, Eq, Hash`. Under
  0.x this is a breaking change (`cargo semver-checks`: `copy_impl_added` +
  `struct_missing`) — 0.2.0 → 0.3.0 when published.
- **`once_cell` dropped as a direct dependency** in favour of
  `std::sync::{OnceLock, LazyLock}`, which most crates already used. It remains
  in the lockfile transitively via `ahash` ← `hashbrown` ← `dashmap`.
- **Workspace lints:** `unexpected_cfgs`, `unsafe_op_in_unsafe_fn` and
  `unused_must_use` at `deny`, each measured at zero sites first so they are a
  regression bar rather than a migration. `clippy::undocumented_unsafe_blocks`
  was tried and reverted — see the note in `Cargo.toml`: it only sees what the
  Linux job compiles (~91 further sites live in the Windows/macOS backends), and
  enabling it before auditing produced comments that stated invariants the code
  does not establish.
- **`just test-release` went from 4 red suites to 1.** The recipe now excludes
  flui-platform, matching the CI `test` job — that crate's suite is red
  independently of the profile (the STATUS_HEAP_CORRUPTION investigation), so
  including it made the recipe permanently red. With that scoped, eleven
  `#[should_panic]`-over-`debug_assert!` tests across eight files could not pass
  in release, where the assertion does not exist; they are now
  `cfg(debug_assertions)`-gated. Two were introduced by this branch, nine
  predated it.
  One suite is still red and is NOT fixed here: flui-interaction's
  `eager_dispose_clears_state` has a deliberate release-only branch asserting
  that a post-`dispose` `add_pointer` does not reach the arena, and it does.
  Verified red on `main` independently of this branch — a real defect in the
  recognizer's dispose guard, not a profile artifact.

- **`wasm-check` now passes.** The job had never been green: 11 errors across
  `flui-scheduler` (2), `flui-platform`'s web backend (4) and `flui-app` (5).
  The `flui-app` five are not dead code — the job runs `cargo check` without
  `--all-targets`, so the desktop runner (`cfg(not(target_arch = "wasm32"))`)
  and the tests that consume them are absent from the wasm lib check; they
  carry `#[cfg_attr(target_arch = "wasm32", allow(dead_code))]` naming the
  consumer rather than a blanket allow.
- Lockfile: `wgpu` 29.0.3 → 29.0.4, `anyhow` → 1.0.103, `crossbeam-epoch`
  → 0.9.20, `swash` → 0.2.9 (off a yanked version). `clippy.toml` gains
  an `msrv` key so MSRV-aware lints track the declared floor.
- **One integration-test binary per heavy crate**: flui-widgets (49 → 1 +
  the pre-existing `parity` target), flui-rendering (36 → 1), flui-view
  (24 → 1) — each root `tests/*.rs` used to statically link the whole wgpu
  stack into its own binary (~5.9 GB across 188 executables). Files stay in
  place as `#[path]` modules; test parity proven exactly (550/782/252 tests
  unchanged). Bevy-style `dynamic_linking` (`flui-dylib`) noted as the next
  lever if needed.
- **Dev profile `debug = 1` → `"line-tables-only"`**: panic backtraces keep
  file:line, the variable/scope DWARF that ballooned `target/debug/deps`
  toward ~20 GB is gone, and the local default now matches what CI has built
  with since PRs #236/#242. `--profile dbg` remains the full-debuginfo
  opt-in. New `just sweep` recipe (cargo-sweep) prunes stale artifacts.

- **Lint normalization**: every workspace crate now inherits
  `[workspace.lints]` via `[lints] workspace = true` (12 crates previously
  bypassed workspace lints entirely; 3 carried stale local copies), enforced
  by a new drift guard in `scripts/check-workspace-inventory.sh`.
- `flui-assets` restored to `[workspace] members` — it is built and tested by
  CI again.

### Removed

- **Singleton retirement, completed in six PRs (#586–#593).** `flui-app`'s
  process-global service host — `AppBinding`, plus its `WidgetsFlutterBinding`
  alias — is deleted outright, not deprecated, along with
  `flui-foundation`'s `impl_binding_singleton!` macro and the
  `HasInstance`/`BindingBase` trait pair, `SemanticsBinding`, and the
  `PaintingBinding`/`Scheduler` singleton `::instance()` accessors. Ownership
  moves to where it should have lived all along: `AppRuntime` (loop-scoped:
  wake, clipboard), `UiRealm`/`PresentationState` (realm/presentation-scoped:
  renderer, vsync, frame counters, haptics, and semantics fan-out through a
  new per-presentation `SemanticsHost`), and a `WeakScheduler`-backed
  `Scheduler` owned fresh per realm — the weak handle breaks the
  `scheduler → transient queue → ticker closure → scheduler` `Arc` cycle
  that used to leak every active ticker for the scheduler's whole lifetime.
  `UiRealm`'s transitional at-most-one-construction guard
  (`REALM_CLAIMED`, `UiRealmError::AlreadyExists`) — which existed only
  because `UiRealm` used to front that process-global state — is deleted
  too; four new tests prove actual realm coexistence (independent mounts,
  scheduler phases, and gesture arenas, including across two threads and
  across a `GlobalKey` collision in two different realms) rather than
  merely the absence of the deleted guard. The test locks this family
  needed — `SINGLETON_WINDOW_TEST_LOCK`, `SCHEDULER_PHASE_TEST_LOCK`,
  `SEMANTICS_TEST_LOCK` — are gone along with the state they serialized;
  each is now a `forbidden_pattern` ratchet in `docs/runtime-contract.toml`
  so a PR cannot reintroduce the pattern by reintroducing the name.

  **Breaking (pre-1.0, sanctioned):** `flui_app::{AppBinding,
  WidgetsFlutterBinding}` and the `flui::app` facade re-export of both are
  gone; `crates/flui-foundation/src/binding.rs` (`BindingBase`,
  `HasInstance`, `impl_binding_singleton!`) is deleted wholesale;
  `SemanticsBinding` is deleted, not slimmed. `run_app`/
  `run_app_with_config`/`run_direct` signatures are unchanged — this is an
  internal-ownership change, not a public entry-point break. Recorded here
  as the semver trigger for a `0.2.0` → `0.3.0` bump once this workspace is
  published; no git tag is cut by this entry (release-lead's call, separate
  from this changelog sweep).

### Pre-changelog milestones

Recorded retroactively from `docs/ROADMAP-TRACKER.md`; evidence links live
there.

- **2026-07-01 — Core.2 exit**: full render-object catalog (37 concrete
  RenderBox/RenderSliver objects extracted to `flui-objects`), 250/250
  per-object harness tests, catalog CI guard.
- **2026-06-30 — Core.0 exit / Core.1 substantially delivered**: view/element
  core contracts locked (`specs/004-view-element-core`, keyed reconciliation,
  `IntoView` authoring surface, element storage); `flui-widgets` slice with 14
  widget families; `flui-animation` re-enabled; production vsync + lazy
  slivers end-to-end; C1.11 contract-validation report (4,847 tests passing).
  Core.1 formal exit still awaits a windowed run (C1.10/C1.12 — see the OPEN
  ITEM in the tracker).
- **2026-06 — GPU engine hardening**: WGSL readback/oracle suite (~440 tests)
  runs on CI via WARP; image-filter pipeline (blur/ColorFilter) sized to
  content bounds; deterministic-replay IR purity witness.
- **Business.1 (in flight)**: Flutter widget-catalog port continues
  (`RichText`/`Icon` landed); tracked in `docs/ROADMAP.md`.
