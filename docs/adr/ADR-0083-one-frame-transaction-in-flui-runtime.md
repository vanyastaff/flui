# ADR-0083: One frame transaction lives in `flui-runtime` above `flui-widgets`

- **Status:** Accepted in part (2026-09-26): §1's placement (tier K, kind `internal`, above
  `flui-widgets`, a normal graph that reaches none of the K set) and the first three moves (see
  `## Migration`). Move 4 has moved; its acceptance waits on CI's `cross-typecheck`,
  `wasm-check` and `wasm-test`, which this host cannot run. §1's ordering before `flui-testing` follows §4 and is not yet in place
  (`flui-testing` still sits below the runtime). §1's ownership list is accepted for the items
  those moves placed (the presentation lanes, the frame sink seam, `PerformanceStats`,
  `ExecutionServices`, and the realm core: `UiRealm`, `PresentationState`, the presentation
  forest, the per-presentation lifecycle, frame-failure reporting and its ADR-0048
  containment); the rest of it and §2–§5 remain Proposed.
- **Date:** 2026-09-25
- **Supersedes in part:** [ADR-0041](ADR-0041-workspace-topology-contract.md)
  (the paragraph "No `flui-runtime` without two consumers")
- **Amends (on acceptance):** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §9 (the public
  realm/runtime surface); [ADR-0037](ADR-0037-presentation-ownership-domains.md) §1 and §4 (the
  composition and `PresentationState` leave `flui-app`) and §12 (the two-production-consumer
  condition, §5 below); [ADR-0044](ADR-0044-driver-loop-hybrid.md)
  (the headless driver row); [ADR-0047](ADR-0047-unified-execution-services.md) (where
  `ExecutionServices` lives)
- **Related:** [ADR-0018](ADR-0018-async-builder-seam.md),
  [ADR-0021](ADR-0021-hero-flight-seam.md),
  [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md),
  [ADR-0048](ADR-0048-frame-transaction-boundary.md),
  [ADR-0075](ADR-0075-derived-state-and-effects.md),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md),
  [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md)
- **Refs:** decision D2 of the
  [architecture review](../research/2026-09-25-architecture-review/report-architecture.ru.md);
  index in [`design/decisions.md`](../../design/decisions.md)

## Context

A frame is produced by two different implementations.

- **The product.** The desktop, Android, iOS and wasm runners drive each presentation through
  `UpdateScheduler::drive_frame_with_lane` (`crates/flui-scheduler/src/scheduler.rs:1874`;
  production callers at `crates/flui-app/src/app/runner/desktop.rs:504`,
  `crates/flui-app/src/app/runner/android.rs:454`,
  `crates/flui-app/src/app/runner/ios.rs:426`, `crates/flui-app/src/app/runner/web.rs:241`,
  `crates/flui-app/src/app/runner/mod.rs:624,681` and
  `crates/flui-app/src/app/runner/frame_pacing.rs:1115,1195`), with the per-presentation
  `catch_unwind` boundary of ADR-0048 in `UiRealm::draw_frame_entered`
  (`crates/flui-app/src/app/ui_realm/frame.rs:74`). The realm, its presentations and the
  execution services are private to `flui-app`: `UiRealm` (`ui_realm/mod.rs:145`),
  `PresentationState` (`presentation.rs:157`), `AppRuntime` (`runtime.rs:570`) and
  `ExecutionServices` were all `pub(crate)`; the execution services have since moved to
  `flui_runtime::execution` (move 3 below).
- **The tests.** `flui-testing`'s `HeadlessBinding::pump_frame`
  (`crates/flui-testing/src/lib.rs:955-1079`) advances a virtual clock, settles gestures,
  ticks controllers, then calls the same scheduler entry point (`lib.rs:1017`) around its own
  `run_pipeline`. The multi-presentation driver is `pump_presentation`/`pump_all`
  (`lib.rs:1270,1301`), which ADR-0044's headless row names. Because `flui-testing` cannot
  depend on `flui-app`, it re-implements the frame instead of running it.

Tests therefore prove the harness. ADR-0075 records the failure this produces: its prototype's
effects "never ran in the product", because `run_effects` was called only from
`HeadlessBinding::pump_frame` (ADR-0075, "What the prototype got wrong", item 1).

The runtime cannot move below `flui-widgets`. The realm's composition names widget types:
`use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};`
(`crates/flui-app/src/app/ui_realm/attach.rs:6`), `use flui_widgets::NavigatorCommand;`
(`ui_realm/commands.rs:8`), `use flui_widgets::{MediaQuery, MediaQueryData};`
(`crates/flui-app/src/app/media_query_root.rs:9`); 22 files in `crates/flui-app/src` import
`flui_widgets`.

The harness also sits in the wrong place for such a move. `flui-widgets` depends on
`flui-testing` optionally for its `testing` feature (`crates/flui-widgets/Cargo.toml:89`), and
22 modules under `crates/flui-widgets/src` use `crate::testing`
(`grep -rln 'crate::testing' crates/flui-widgets/src | grep -v src/testing`), a 2,144-line
module that `flui-material` and `flui-cupertino` tests also use. If `flui-testing` depends on a
crate above `flui-widgets`, those in-crate unit tests would compile a second copy of
`flui-widgets` through the dev cycle; the scopes installed by one copy would not be found by
`TypeId` lookups in the other. That symptom is a hypothesis: the dev-cycle duplication is
documented Cargo behaviour, the failure has not been compiled.

The process-global side is narrower than it looks. One `thread_local! APP_RUNTIME` hosts every
realm on the loop (`crates/flui-app/src/app/runner/host.rs:25-47`), and its own comment says
why it is in TLS: the platform callback surface requires `Send`, so the `!Send` realm cannot be
reached from those callbacks any other way (`host.rs:32-34`).

ADR-0041 gated a `flui-runtime` crate on "a managed entry point and an embedded/host-driven one
both driving the same core". ADR-0037 §12 forbids an anemic `flui-presentation` crate unless a
"deep, policy-free abstraction with two production consumers" exists.

## Decision

### 1. `flui-runtime` owns the frame

A new crate, `flui-runtime`, tier K, kind `internal` (ADR-0081), ordered after `flui-widgets` and
before `flui-testing` and `flui-sdk`. It owns:

- the realm (`UiRealm`, its command vocabulary and dispatcher) and each presentation's
  `PresentationState` (moved from `flui-app`);
- **the frame transaction**: one method per realm,
  `Realm::pump(&mut self, clock: &mut dyn FrameClockSource, sink: &mut dyn FrameSink) -> FrameOutcome`,
  whose phase order keeps the scheduler's Flutter-shaped frame (ADR-0021): apply input → begin
  frame (`handle_begin_frame`, `scheduler.rs:1274`: transient callbacks, so animation tickers
  advance, then the microtask flush) → draw frame (`handle_draw_frame`, `scheduler.rs:1422`:
  persistent callbacks and the priority task queue) → drain build → run effects (the phase
  ADR-0075 reserves; empty until that record is accepted) → layout → compositing → paint →
  semantics → produce the `SceneSnapshot` → end frame (`end_frame_with_lane`,
  `scheduler.rs:1552`, which runs the post-frame callbacks of both queues). The
  per-presentation panic boundary of ADR-0048 moves with it unchanged;
- `ExecutionServices` and the realm's instances of runtime-owned capabilities
  (`AsyncDriver`/`Spawner` instances, the registry of ADR-0084);
- `OwnerHost`: the loop-scoped host of realms that replaces `AppRuntime`'s realm slot.

`flui-runtime` depends only on crates in tiers V–K and on `flui-platform-api` (ADR-0082). Its
normal graph must not reach `flui-platform`, `winit`, `wgpu`, `flui-engine` or `flui-app`
(ADR-0081's K set). It talks to windows through `Arc<dyn PlatformWindow>` handles the runner
passes in, and hands frames out through `FrameSink`; the wgpu raster lane and `RasterOwner` stay
in `flui-app` and `flui-engine`.

**Types stay low, instances move up.** `LifecycleContext` lives in `flui-view` and cannot name a
`flui-runtime` type. A type reached through `LifecycleContext` or `BuildContext` therefore stays
in `flui-view` or below (`AsyncDriver` in `flui-scheduler`, `GlobalKeyScope` in `flui-view`,
`FontContext` in `flui-painting`); the runtime owns the instance.

### 2. The frame entry points are reachable only from `flui-runtime`

"One transaction" is defined by reachability, not by function names. The entry points that drive
frame phases — `UpdateScheduler::drive_frame`/`drive_frame_with_lane`
(`scheduler.rs:1860,1874`), `handle_begin_frame`/`handle_draw_frame` (`scheduler.rs:1274,1422`)
and `WidgetsBinding::draw_frame`/`draw_frame_with_phase_marker`
(`crates/flui-view/src/binding.rs:1218,1232`) — are called from `flui-runtime` and nowhere else
outside their own crates' tests.

The mechanism, in order of preference:

1. the composition moves into `flui-runtime` and the lower crates expose phase primitives
   that require a witness value (`FramePhase`) which only the transaction constructs;
2. where a lower crate cannot express that, the entry point moves under the owning crate's
   `#[doc(hidden)] __runtime` module (ADR-0081 §4) and a `cargo xtask frame-entry` scan, with a
   `--self-test`, fails on any call outside `crates/flui-runtime/src`.

A `#[doc(hidden)]` path alone is not a seal; the scan is what enforces option 2.

### 3. The runner keeps one trampoline cell

`flui-app` shrinks to runners: it creates the platform, the raster lane and the `FrameSink`,
installs `OwnerHost`, and routes OS callbacks. OS callback trampolines (Win32 `WndProc`, AppKit
delegates, UIKit, Android JNI) reach exactly **one** thread-local cell that holds the
`OwnerHost`. That cell is the named, permanent exception of ADR-0097. While platform callbacks
still require `Send` (ADR-0082 §4), the realm stays in that cell; the extraction does not wait
for the `Send` removal.

### 4. `flui-testing` runs the product transaction

`flui-testing` moves above `flui-runtime` (tier K, after it). `HeadlessBinding::pump_frame` and its
private `run_pipeline` are deleted; the headless driver calls `Realm::pump` with a manual clock
and a headless sink. `pump_presentation`/`pump_all` become thin loops over the same call.
`flui_widgets::testing` is absorbed into `flui-testing`.

`flui-widgets` stops depending on `flui-testing` (`crates/flui-widgets/Cargo.toml:89` goes). That
is prepared by its own change, which lands first: the tests in the 22 `src/` files that used
`crate::testing` and reach the harness move to `crates/flui-widgets/tests/`, where the library
links once; the tests there that do not reach the harness stay unit tests. The compiler keeps it
that way: `flui_widgets::testing` is `#[cfg(all(feature = "testing", not(test)))]`, so a unit test
under `src/` that names it fails to build. A moved test that still reads a private item reaches it
through `flui_widgets::__test_access`: doc-hidden, always compiled (no visibility feature, one type
layout), for `crates/flui-widgets/tests` only, and **temporary**. It holds probe traits for
methods on public types, re-exports of private types raised to `pub` inside private modules, and
test types that stand in for a capability that must stay unreachable (a route that finalizes only
itself, rather than a probe that finalizes through any `RouteBindingSlot`). Not every entry is a
read: some drive state the navigator normally drives (`pop_paced`, a modal's offstage flag, a
hero's flight), and a re-exported type brings its `pub` methods along. A unit test pins its whole
surface — every name and probe method, with the reason for each entry — and refuses a glob, a
module re-export or any other kind of `pub` item. It is kept apart from the runtime's
`__runtime` seam so the two can be removed separately. An entry leaves when its tests assert
through public API — for the navigator internals, the Router conformance suite of
[ADR-0093](ADR-0093-router-is-the-primary-navigation-api.md) — and the module is deleted when
empty.

### 5. The two-consumer condition is met

ADR-0041's gate asked for two entry points driving one core: `flui-runtime` has two drivers of
one `Realm::pump`, the platform runners in `flui-app` and the headless driver in `flui-testing`.
ADR-0037 §12 asked for a deep, policy-free abstraction with *two production consumers*; the
runtime has one production consumer (`flui-app`), since the headless driver is test
infrastructure. This record therefore amends ADR-0037 §12: a runtime crate is justified by one
production consumer plus the test driver that must run the same transaction, because the defect
it removes (ADR-0075's effects that never ran in the product) comes from two implementations of
one frame, not from a missing second product. It is not the forbidden `flui-presentation` crate: it owns
the realm state rather than handles to it, it does not depend on application policy (runners,
backends and the GPU stay in H), and it is not a fourth owner forwarding operations; the realm,
the platform backend and the raster owner remain the three owners of ADR-0037 §1.

### Not decided here

- **Build during layout.** A `LayoutCallbackScope` spike ran on 2026-09-26
  ([ADR-0017](ADR-0017-build-during-layout-callback-seam.md), "Revisited"). It reached one pass
  for plain lazy rows, regressed `LayoutBuilder` rows, and left the double borrow unsolved, so
  ADR-0017 §3 and the ADR-0003 fixpoint stay and the phase order above keeps between-pass
  servicing. A superseding ADR needs the four conditions recorded there.
- **Realm concurrency.** One owner thread hosting isolated realms is ADR-0091's decision.
- **A public embedder API.** `Realm` and `OwnerHost` are `pub` in `flui-runtime` because
  `flui-app` and `flui-testing` are separate crates, but the crate's kind is `internal`; a
  supported embedder surface is still designed separately, as ADR-0027 §9 says.

## Alternatives considered

- **`flui-runtime` below `flui-widgets`.** The realm's composition installs widget scopes and
  routes `NavigatorCommand`; placing the runtime lower requires moving those first, or replacing
  them with trait objects for no second implementation. `NavigatorCommand` later becomes a
  design-neutral navigation intent (ADR-0093), which does not require the runtime to move.
- **The runtime inside `flui-view`.** It would pull scheduling, presentations and the command
  vocabulary into the spine that every widget recompiles against, and still could not name
  widget scopes.
- **Keep the runtime in `flui-app` and let `flui-testing` depend on `flui-app`.** The test driver
  would link the platform backends, the engine and wgpu; headless tests would stop being
  headless, and ADR-0081's K reach facts could not hold for `flui-testing`.
- **Keep two drivers and share a "phase list" helper.** A helper both call still lets either
  skip or reorder a phase; the ADR-0075 failure was exactly a phase present in one driver only.
- **Enforce with "no `pub fn pump_frame` in `flui-testing`".** A rename defeats it, and it
  checks the wrong functions: `pump_frame` does not call the runner's own sequence.

## Consequences

- `crates/flui-app/src/app/runner/realm_dispatch.rs` (about 1,690 lines of production code; its
  tests live in `realm_dispatch/tests.rs`) and the `ui_realm` modules move to `flui-runtime`; `flui-app` keeps
  runners, the raster lane and platform wiring. The review targets under 15k lines for
  `flui-app`; that is a goal, not a measurement.
- Test code that constructs `HeadlessBinding` and calls `pump_frame` changes to the new driver.
  Behaviour differences between the two drivers surface as test failures during the move; they
  are fixed in the product path, not by keeping the old driver.
- ADR-0021's statement that every frame driver, including `HeadlessBinding::pump_frame`, goes
  through `drive_frame` (its "Every frame driver" paragraph) becomes historical; the rule it protects holds by construction.
- ADR-0043's "`BuildOwner` and `WidgetsBinding` are public surface … consumed by
  `flui-hot-reload`" narrows: the frame entry of `WidgetsBinding` moves under `__runtime`.
- Material and Cupertino tests that use `flui_widgets::testing` switch to `flui-testing`.
- Rollback for the series: the old driver can be kept behind a feature for one minor while the
  runners move; it is removed before the next release.

## Migration

The crate is created first and filled in five moves, each independently mergeable. The
[migration plan](../plans/2026-09-25-architecture-migration-plan.md) tracks them.

| Move | What moves or changes | Waits on |
|---|---|---|
| 1. Lanes (done) | `flui-runtime` is created: tier K, `internal`, `order = 5`, layer 6, with no `flui-widgets` edge until the realm core needs one. It holds the presentation lanes that need nothing from the realm core: `epoch` (`TreeRevision`, `FrameCommitState`), `held_input` (`HeldPointerQueue`, `HeldPointerReplay`) and `semantics_host` (`SemanticsHost`). Items with no production caller compile only under `cfg(test)` or the `test-support` feature | — |
| 2. Frame sink (done) | `FrameSink` and `SubmitVerdict` (engine-free; they name only `flui_layer::Scene`) move to `flui_runtime::sink`, and `PerformanceStats` to `flui_runtime::performance_stats` (it is fed while the layer tree is built, not at submit); `RasterLane<B>` and `DirectSink` stay in `flui-app` and implement the trait | move 1 |
| 3. Execution (done) | `ExecutionServices` (ADR-0047) moves to `flui_runtime::execution`; `flui-app` re-exports `ComputeJob`, `DeterministicExecutors`, `HostComputePool`, `HostExecutors`, `HostIoPool`, `IoFuture` and `SpawnError`, so their public paths do not change. `allowed-dependents = ["flui-app"]` on the runtime keeps ADR-0047's invariant true now that the services are `pub`: only a host crate (one of the runtime's `allowed-dependents`) constructs `ExecutionServices`, and no other workspace crate reaches the pools | move 1 |
| 4. Realm core (moved; acceptance waits on CI's `cross-typecheck`, `wasm-check` and `wasm-test`) | `ui_realm` (with `attach`, whose root wrappers are `flui-widgets`'), `presentation`, `presentation_forest`, `lifecycle_state`, `frame_failure`, `media_query_root` (with its window constructor: it names only `PlatformWindow`, and its only callers are `PresentationState`'s constructors), `renderer_binding` (`RenderingFlutterBinding`) and the realm's `RealmServices`/`next_identity`; the ADR-0048 `catch_unwind` moves unchanged. The realm renders through `UiRealm::render_frame(&mut impl FrameSink)` and names no engine type: `flui-app`'s `RealmRaster` trait keeps the engine-backed entry points (`render_frame_entered` over a `DirectSink`, `render_frame_on_lane`). The realm tests move with a headless sink (`flui_runtime::testing::ScriptedSink`, under `test-support`), and `UiRealm::enter_for_close` is deleted. Items `flui-app` calls in production are `pub`; items only its tests call are `pub` under `test-support`. Three names the realm used that no runtime edge may carry moved down first: `PlatformAccessibility` to `flui-semantics` (ADR-0082 §2, amended), `REDACTED_VALUE` to `flui_foundation::diagnostics` (only composition roots may depend on `flui-log`), and the hot-reload tier, which the realm now takes as `flui_runtime::reload::ReloadTier` and `flui-app` translates from `flui-hot-reload`'s | `PlatformWindow` in `flui-platform-api` (ADR-0082 §3, second change), since `PresentationState` and `UiRealm` name it; moves 2 and 3 |
| 5. Transaction | `Realm::pump` absorbs the runners' `drive_frame_with_lane` calls; `OwnerHost` replaces `AppRuntime`'s realm slot and is §3's one trampoline cell; the production part of `realm_dispatch.rs` moves; the two verification tests below land. Rollback: a `legacy-frame-driver` cargo feature on `flui-app` for one minor | move 4 |

§2 (sealing the entry points), §4 (`flui-testing` above the runtime, taking an `order` after it)
and the widgets' inline test modules follow move 5.

`cargo xtask reach` (ADR-0081 §2) checks the K-set fact for `flui-runtime` over every root
build: its tier K forbids `flui-platform`, `winit`, `android-activity`, `ndk`, `windows`,
`objc2-app-kit`, `objc2-ui-kit`, `wgpu`, `flui-engine` and `flui-app`, and no
`reach-exceptions` entry excuses any of them.

Two defects the realm core would have carried across are fixed with move 1, each pinned by a
test that failed before the fix:

- **A `GlobalKey` read inside its own presentation's frame deadlocked.** `WidgetsBinding` holds
  its own write lock across a frame, attach, detach and layout-builder build, and the registry
  read it again from the same thread. The registry now reads without blocking and reports a
  busy member; the realm composite skips it, and `GlobalKey` resolves to `None` for keys of the
  presentation whose frame is running (a Flutter divergence recorded in `flui-view`'s
  `ARCHITECTURE.md`). This made the closing-presentation exclusion in
  `UiRealm::enter_for_close` redundant; move 4 deleted it, so a key of the closing
  presentation now resolves while it detaches, before its teardown takes the lock
  (`closing_presentations_own_key_resolves_while_it_detaches`). Tests:
  `global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking`,
  `global_key_in_a_sibling_binding_resolves_during_this_bindings_frame`,
  `global_key_lookup_from_dispose_during_detach_returns_instead_of_deadlocking` (`flui-view`),
  `drawer_style_state_read_during_the_realm_frame_does_not_deadlock` and
  `state_read_across_presentations_during_a_segment_resolves` (`flui-app`).
- **`ElementBase::depth` returned the sibling slot.** The tree now stamps the depth on the
  element whenever it sets a node's depth (`ElementBase::set_depth`), as Flutter's
  `Element._depth` is set in `mount` and repaired in `_updateDepth`. Tests:
  `element_depth_is_the_tree_depth_not_the_sibling_slot`,
  `globalkey_retake_restamps_element_depth_for_the_moved_subtree`.

## Verification

Only the first exists: both crates are tier K, and `cargo xtask reach` checks them on every
change.

- `cargo xtask reach` (ADR-0081): `flui-runtime` and `flui-testing` reach none of the K set.
- `cargo xtask frame-entry --self-test` (if option 2 of §2 is used): a planted call to
  `drive_frame_with_lane` in `flui-widgets` fails the gate.
- A test in `flui-testing` that fails when a phase is missing from `Realm::pump`: a post-frame
  callback registered during build observes this frame's committed layout, driven through the
  headless sink.
- The ADR-0075 acceptance test, once that record is accepted: an effect runs under the product
  frame driven by the runner's code path, not only under the harness.
- A grep-free structural check: a `reach-forbid` fact (ADR-0081 §2) that `flui-testing` is
  absent from `flui-widgets`' normal closure with all features. (`cargo tree -i` cannot state it:
  it errors on an absent package.)
- A test that fails when the begin-frame phase is skipped: an animation controller driven through
  `Realm::pump` with a manual clock advances its value between two frames.
