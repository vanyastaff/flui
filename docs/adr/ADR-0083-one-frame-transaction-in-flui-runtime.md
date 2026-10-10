# ADR-0083: One frame transaction lives in `flui-runtime` above `flui-widgets`

- **Status:** Accepted in part (2026-09-26): §1's placement (tier K, kind `internal`, above
  `flui-widgets`, a normal graph that reaches none of the K set) and the first three moves (see
  `## Migration`). Move 4 has moved; its acceptance waits on CI's `cross-typecheck`,
  `wasm-check` and `wasm-test`, which this host cannot run. §1's ordering before `flui-testing` is
  in place: `flui-testing` sits above the runtime (order 6). §1's ownership list is accepted for the items
  those moves placed (the presentation lanes, the frame sink seam, `PerformanceStats`,
  `ExecutionServices`, and the UI runtime core: `UiRuntime`, `PresentationState`, the presentation
  forest, the per-presentation lifecycle, frame-failure reporting and its ADR-0048
  containment); §1's frame transaction is accepted as `UiRuntime::pump`, which every runner
  drives (the first half of move 5), with the same CI wait for its Android, iOS and wasm sites.
  §4 is accepted in part (move 6a): `flui-testing` sits above the runtime and absorbs
  `flui_widgets::testing`, and its widget harness drives `UiRuntime::pump` on a manual clock with a
  headless sink; `HeadlessBinding::pump_frame` remains for the raw-owner suites until move 6b.
  The rest of §1 and §2, §3, the rest of §4 and §5 remain Proposed.
- **Date:** 2026-09-25
- **Supersedes in part:** [ADR-0041](ADR-0041-workspace-topology-contract.md)
  (the paragraph "No `flui-runtime` without two consumers")
- **Amends (on acceptance):** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §9 (the public
  UI runtime/runtime surface); [ADR-0037](ADR-0037-presentation-ownership-domains.md) §1 and §4 (the
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

**Superseded-by:** [ADR-0182](ADR-0182-complete-owner-execution.md) for scheduler
execution authority and the raw background sequence. The runtime now submits
preparation and pipeline through complete owner operations.

**Superseded-by:** [ADR-0136 §2](ADR-0136-owner-local-ui-surfaces.md) for the frame
entry API: `drive_frame(&OwnerFrame, ..)` replaces the former lane-specific driver
named in this record's migration history. `UiRuntime::pump` retains the transaction;
its background arm calls `finish_async_pump()` then `OwnerFrame::poll_ready()`.

A frame is produced by two different implementations.

- **The product.** The desktop, Android, iOS and wasm runners drive each presentation through
  `UpdateScheduler::drive_frame_with_lane` (`crates/flui-scheduler/src/scheduler.rs:1874`;
  production callers at `crates/flui-app/src/app/runner/desktop.rs:504`,
  `crates/flui-app/src/app/runner/android.rs:454`,
  `crates/flui-app/src/app/runner/ios.rs:426`, `crates/flui-app/src/app/runner/web.rs:241`,
  `crates/flui-app/src/app/runner/mod.rs:624,681` and
  `crates/flui-app/src/app/runner/frame_pacing.rs:1115,1195`), with the per-presentation
  `catch_unwind` boundary of ADR-0048 in `UiRuntime::draw_frame_entered`
  (`crates/flui-app/src/app/ui_runtime/frame.rs:74`). The UI runtime, its presentations and the
  execution services are private to `flui-app`: `UiRuntime` (`ui_runtime/mod.rs:145`),
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

The runtime cannot move below `flui-widgets`. The UI runtime's composition names widget types:
`use flui_widgets::{FocusRoot, GestureArenaScope, VsyncScope};`
(`crates/flui-app/src/app/ui_runtime/attach.rs:6`), `use flui_widgets::NavigatorCommand;`
(`ui_runtime/commands.rs:8`), `use flui_widgets::{MediaQuery, MediaQueryData};`
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
UI runtime on the loop (`crates/flui-app/src/app/runner/host.rs:25-47`), and its own comment says
why it is in TLS: the platform callback surface requires `Send`, so the `!Send` UI runtime cannot be
reached from those callbacks any other way (`host.rs:32-34`).

Before move 5a, the owner-turn dispatcher had a correctness hole that could not be moved as a
contract. While UI runtime A was checked out, a synchronous callback addressed to resident UI runtime B was
rejected as `NestedCrossRuntimeDispatchRejected`. For example, an
event handler in A may close B's native window, whose close callback re-enters the dispatcher;
the native close has happened, but B's close task is lost and its UI runtime can remain installed.
Move 5a replaced that rejection with one host-wide FIFO in `flui-app`: reentrant work for any
UI runtime is appended and runs in enqueue order after restoring each checkout. This is a correctness
repair, not the final scheduling contract: it deliberately inherits the old unbounded
drain-until-empty behavior. Move 5b must replace that policy with bounded batches and a coalesced
continuation wake before the queue is extracted into `OwnerHost`.

ADR-0041 gated a `flui-runtime` crate on "a managed entry point and an embedded/host-driven one
both driving the same core". ADR-0037 §12 forbids an anemic `flui-presentation` crate unless a
"deep, policy-free abstraction with two production consumers" exists.

## Decision

### 1. `flui-runtime` owns the frame

A new crate, `flui-runtime`, tier K, kind `internal` (ADR-0081), ordered after `flui-widgets` and
before `flui-testing` and `flui-sdk`. It owns:

- the UI runtime (`UiRuntime`, its command vocabulary and dispatcher) and each presentation's
  `PresentationState` (moved from `flui-app`);
- **the frame transaction**: one method per UI runtime,
  `UiRuntime::pump(&mut self, clock: &mut dyn FrameClockSource, sink: &mut dyn FrameSink) -> FrameOutcome`,
  whose phase order keeps the scheduler's Flutter-shaped frame (ADR-0021): apply commands → begin
  frame (`handle_begin_frame`, `scheduler.rs:1274`: transient callbacks, so animation tickers
  advance, then the microtask flush) → draw frame (`handle_draw_frame`, `scheduler.rs:1422`:
  persistent callbacks and the priority task queue) → drain build → run effects (the phase
  ADR-0075 reserves; empty until that record is accepted) → layout → compositing → paint →
  semantics → produce the `SceneSnapshot` → end frame (`end_frame_with_lane`,
  `scheduler.rs:1552`, which runs the post-frame callbacks of both queues). The
  per-presentation panic boundary of ADR-0048 moves with it unchanged. The clock is read once
  per pump: it is the scheduler's frame timestamp and the time the UI runtime's `Vsync` controllers
  tick at. A scheduler `Ticker` still measures elapsed time on the wall clock, a divergence
  from Flutter's `Ticker._tick` recorded and pinned in `flui-scheduler`'s `ARCHITECTURE.md`.
  Whether a wake becomes a frame stays the runner's per-backend wake gate (ADR-0058); a wake
  with frames disabled calls `UiRuntime::pump_background` (clear the frame latch, then poll the
  async driver) and runs no frame;
- `ExecutionServices` and the UI runtime's instances of runtime-owned capabilities
  (`AsyncDriver`/`Spawner` instances, the registry of ADR-0084);
- `OwnerHost`: an ordinary loop-scoped, `!Send + !Sync` host of UI runtimes. It owns the
  UI runtime-neutral registry, one host-wide FIFO of addressed typed operations, checkout state and
  deferred UI runtime-map mutations. It does **not** own the native-window registry, platform or
  engine objects, native frame drivers, application services, execution pools or the TLS that lets
  an OS callback find it.

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
and `WidgetsBinding::draw_frame` (`crates/flui-view/src/binding.rs`) with
`BindingRuntime::draw_frame_with_phase_marker` (`crates/flui-view/src/__runtime.rs`, a method of
the sealed `flui_view::__runtime::BindingRuntime`, ADR-0081 §4) — are called from `flui-runtime`
and nowhere else outside their own crates' tests.

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
delegates, UIKit, Android JNI) reach exactly **one** thread-local cell: `APP_RUNTIME` in
`flui-app`, the named permanent exception of ADR-0097. `APP_RUNTIME` contains the ordinary
`OwnerHost` alongside host-only state. No second TLS cell is added in `flui-runtime`.

The boundary follows ADR-0037's three owners. `flui-app` keeps `WindowRegistry` and native
window demultiplexing, renderer resize operations, platform appearance normalization,
close-admission routing, the owner-platform capability, raster/engine state, application
services and execution-pool lifetime. It translates a platform callback into a FLUI-owned,
`flui-platform-api`-only operation carrying its exact `PresentationAddress`; `OwnerHost`
admits and drains that operation. A resize therefore applies the host surface first in
`flui-app`, then sends normalized metrics to the addressed presentation. Removing a native
mapping still precedes admitting no further work for that incarnation.

The operation vocabulary is closed. `OwnerHost` does not accept arbitrary
`Box<dyn FnOnce(&UiRuntime)>` or `Box<dyn FnOnce(&mut UiRuntime)>` tasks: those would preserve
today's `RuntimeTask::Frame`/`Pump` escape hatch and contradict ADR-0037 §3. Input, lifecycle,
metrics, close, install, uninstall, pump and background-pump are typed operations or explicit
methods. True cross-thread producers use a bounded typed ingress plus the host's wake
capability; an owner-local dispatcher is `!Send` and only appends to the same FIFO. The owner
executes a bounded batch, then requests exactly one continuation wake when work remains. Each
operation class declares whether it is lossless, latest-value coalescible or edge-coalesced.

Owner admission combines only adjacent pending `Metrics`, `SafeArea` and `Brightness`
observations for the same exact `PresentationAddress`. The batch retains the last size/DPI
pair and the last value of each other property. Applying it resizes the native surface
once, then updates the presentation's media query together. Any other operation, including
input, frame delivery, lifecycle, close and another window's observation, ends the batch.
An operation already executing is never replaced; admission adds no debounce delay.
The combined operation consumes one unit of the physical callback budget.
If native resize fails, its size/DPI pair remains unpublished, while accepted
appearance and safe-area values still settle. Media-query publication, redraw
and owner completion preserve the first failure; failure of one batched property
must not erase another property's accepted value. The public owner contract
`resize_failure_preserves_other_batched_window_state` verifies these values through
a mounted widget, competing failures and a subsequent successful resize.
`queued_state_bursts_coalesce_between_observing_frames` verifies widget-visible metrics
and brightness; `pending_metrics_do_not_cross_ordered_operations` verifies the barriers.
This owner policy does not specify downstream pointer-motion sampling in `GestureBinding`.

Keyboard and IME dispatch finish pending motion on their resolved presentation before
invoking the following input's callbacks. This is distinct from frame sampling: it drains
accepted resampler events regardless of the next sample time, retains each contact and
its interpolation state, and snapshots pending events before user callbacks. It neither
invents an Up/Cancel nor changes the sequence's resampling mode. Motion dispatch, deferred
arena resolution and the following input have separate containment; the first failure is
preserved while the accepted following input still gets its dispatch opportunity.
`pointer_stream_and_keyboard_survive_interleaved_resize` pins keyboard ordering and
competing callback failures through a mounted widget. `input_barrier_preserves_pinch_contacts_and_continuity`
pins contact continuity and scale across the interaction-layer operation.

While platform callbacks still require `Send` (ADR-0082 §4), the UI runtime stays behind this one
app-owned trampoline cell; the extraction does not wait for the `Send` removal.

### 4. `flui-testing` runs the product transaction

`flui-testing` moves above `flui-runtime` (tier K, after it). `HeadlessBinding::pump_frame` and its
private `run_pipeline` are deleted; the headless driver calls `UiRuntime::pump` with a manual clock
and a headless sink. It does not construct or drive `OwnerHost`: sharing the product frame
transaction, not copying the production event-loop topology, is the test-driver contract.
`pump_presentation`/`pump_all` become thin loops over the same call.
`flui_widgets::testing` is absorbed into `flui-testing`.

`flui-widgets` stops depending on `flui-testing` (`crates/flui-widgets/Cargo.toml:89` goes). That
is prepared by its own change, which lands first: the tests in the 22 `src/` files that used
`crate::testing` and reach the harness move to `crates/flui-widgets/tests/`, where the library
links once; the tests there that do not reach the harness stay unit tests. Review keeps it that
way, not the compiler: `flui-testing` depends on `flui-widgets`, so `flui-widgets` names it only on
a dev edge, and a unit test under `src/` that drives the harness links a second copy of the
library. That still compiles, since the harness takes a `flui_view::View` and there is one
`flui_view`, but the UI runtime installs the other copy's inherited scopes, so the test's
`crate::MediaQuery::maybe_of` and every other lookup of a `flui-widgets` type reads `None`. The
planned "harness stays above the runtime" gate (`design/architecture.md`) turns this into a check.
`reach-forbid = ["flui-testing"]` keeps the test driver out of the widgets' normal closure. A moved test that still reads a private item reaches it
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
one `UiRuntime::pump`, the platform runners in `flui-app` and the headless driver in `flui-testing`.
ADR-0037 §12 asked for a deep, policy-free abstraction with *two production consumers*; the
runtime has one production consumer (`flui-app`), since the headless driver is test
infrastructure. This record therefore amends ADR-0037 §12: a runtime crate is justified by one
production consumer plus the test driver that must run the same transaction, because the defect
it removes (ADR-0075's effects that never ran in the product) comes from two implementations of
one frame, not from a missing second product. It is not the forbidden `flui-presentation` crate: it owns
the UI runtime state rather than handles to it, it does not depend on application policy (runners,
backends and the GPU stay in H), and it is not a fourth owner forwarding operations; the UI runtime,
the platform backend and the raster owner remain the three owners of ADR-0037 §1.

### Not decided here

- **Build during layout.** A `LayoutCallbackScope` spike ran on 2026-09-26
  ([ADR-0017](ADR-0017-build-during-layout-callback-seam.md), "Revisited"). It reached one pass
  for plain lazy rows, regressed `LayoutBuilder` rows, and left the double borrow unsolved, so
  ADR-0017 §3 and the ADR-0003 fixpoint stay and the phase order above keeps between-pass
  servicing. A superseding ADR needs the four conditions recorded there.
- **Runtime concurrency.** One owner thread hosting isolated UI runtimes is ADR-0091's decision.
- **A public embedder API.** `UiRuntime` is `pub` because `flui-app` and `flui-testing` drive its
  frame, and `OwnerHost` is `pub` only for `flui-app`; the crate's kind is `internal`. A
  supported embedder surface is still designed separately, as ADR-0027 §9 says.

## Alternatives considered

- **`flui-runtime` below `flui-widgets`.** The UI runtime's composition installs widget scopes and
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

- The UI runtime-neutral registry, addressed owner-turn FIFO, checkout/restore discipline, deferred
  UI runtime-map mutations and typed operation application move from
  `crates/flui-app/src/app/runner/owner_dispatch.rs` and `app/runtime.rs` to `flui-runtime`.
  Native-window routing, surface application and the platform/application tails of an owner
  turn stay in `flui-app`; moving a file wholesale is not the goal. The review targets under
  15k lines for `flui-app`; that is a goal, not a measurement.
- Test code that constructs `HeadlessBinding` and calls `pump_frame` changes to the new driver.
  Behaviour differences between the two drivers surface as test failures during the move; they
  are fixed in the product path, not by keeping the old driver.
- ADR-0021's statement that every frame driver, including `HeadlessBinding::pump_frame`, goes
  through `drive_frame` (its "Every frame driver" paragraph) becomes historical; the rule it protects holds by construction.
- ADR-0043's "`BuildOwner` and `WidgetsBinding` are public surface … consumed by
  `flui-hot-reload`" narrows: the frame entry of `WidgetsBinding` moves under `__runtime`.
- Material and Cupertino tests that use `flui_widgets::testing` switch to `flui-testing`.
- Rollback for the series: each move is one self-contained revert. No feature keeps the old
  driver: `UiRuntime::pump` is the same `drive_frame_with_lane` around the same `render_frame`,
  and a second frame body in every runner would bring back the two-driver drift this record
  removes.

## Migration

The crate is created first and filled in five moves, each independently mergeable. The
[migration plan](../plans/2026-09-25-architecture-migration-plan.md) tracks them.

| Move | What moves or changes | Waits on |
|---|---|---|
| 1. Lanes (done) | `flui-runtime` is created: tier K, `internal`, `order = 4` (after `flui-widgets`; 5 until `flui-localizations` was deleted), layer 6, with no `flui-widgets` edge until the UI runtime core needs one. It holds the presentation lanes that need nothing from the UI runtime core: `epoch` (`TreeRevision`, `FrameCommitState`), `held_input` (`HeldPointerQueue`, `HeldPointerReplay`) and `semantics_host` (`SemanticsHost`). Items with no production caller compile only under `cfg(test)` or the `test-support` feature | — |
| 2. Frame sink (done) | `FrameSink` and `SubmitVerdict` (engine-free; they name only `flui_layer::Scene`) move to `flui_runtime::sink`, and `PerformanceStats` to `flui_runtime::performance_stats` (it is fed while the layer tree is built, not at submit); `RasterLane<B>` and `DirectSink` stay in `flui-app` and implement the trait | move 1 |
| 3. Execution (done) | `ExecutionServices` (ADR-0047) moves to `flui_runtime::execution`; `flui-app` re-exports `ComputeJob`, `DeterministicExecutors`, `HostComputePool`, `HostExecutors`, `HostIoPool`, `IoFuture` and `SpawnError`, so their public paths do not change. `allowed-dependents = ["flui-app"]` on the runtime keeps ADR-0047's invariant true now that the services are `pub`: only a host crate (one of the runtime's `allowed-dependents`) constructs `ExecutionServices`, and no other workspace crate reaches the pools | move 1 |
| 4. Runtime core (moved; acceptance waits on CI's `cross-typecheck`, `wasm-check` and `wasm-test`) | `ui_runtime` (with `attach`, whose root wrappers are `flui-widgets`'), `presentation`, `presentation_forest`, `lifecycle_state`, `frame_failure`, `media_query_root` (with its window constructor: it names only `PlatformWindow`, and its only callers are `PresentationState`'s constructors), `renderer_binding` (`RenderingBinding`) and the UI runtime's `RuntimeServices`/`next_identity`; the ADR-0048 `catch_unwind` moves unchanged. The UI runtime renders through `UiRuntime::render_frame(&mut impl FrameSink)` and names no engine type: `flui-app`'s `RuntimeRaster` trait keeps the engine-backed entry points (`render_frame_entered` over a `DirectSink`, `render_frame_on_lane`). The UI runtime tests move with a headless sink (`flui_runtime::testing::ScriptedSink`, under `test-support`), and `UiRuntime::enter_for_close` is deleted. Items `flui-app` calls in production are `pub`; items only its tests call are `pub` under `test-support`. Three names the UI runtime used that no runtime edge may carry moved down first: `PlatformAccessibility` to `flui-semantics` (ADR-0082 §2, amended), `REDACTED_VALUE` to `flui_foundation::diagnostics` (only composition roots may depend on `flui-log`), and the hot-reload tier, which the UI runtime now takes as `flui_runtime::reload::ReloadTier` and `flui-app` translates from `flui-hot-reload`'s | `PlatformWindow` in `flui-platform-api` (ADR-0082 §3, second change), since `PresentationState` and `UiRuntime` name it; moves 2 and 3 |
| 5a. Owner FIFO correctness (done; platform-target acceptance waits on CI) | Before extraction, replace the per-UI runtime/rejection dispatch with one host-wide FIFO of addressed owner work. A reentrant dispatch to any UI runtime appends instead of recursing or being lost; execution-time admission drops a target made stale by earlier work; checkout restoration and UI runtime-owning drops remain panic-safe and happen outside live mutable host borrows. `NestedCrossRuntimeDispatchRejected` is gone. This transitional queue retains the pre-existing unbounded drain-until-empty policy and private `RuntimeTask` vocabulary; neither is an extraction contract | move 4 |
| 5b. Bounded owner batches (done; platform-target acceptance waits on CI) | Bound each continuation callback and request one sequence-stamped later opportunity when work remains. Fresh native roots and carried FIFO entries share the callback budget; a root stays synchronous while budget remains, and excess roots join the FIFO. Terminal close admission fences later work for that exact presentation incarnation. Desktop/iOS use their owner signal, Android uses a non-dirty redraw poke, and web consumes the continuation on the next RAF. Deterministic tests prove a self-enqueueing cross-UI runtime chain yields at every budget boundary and that fresh roots cannot extend a continuation beyond its budget | move 5a |
| 5c. Closed owner operations | Replace arbitrary `RuntimeTask::Frame`/`Pump` closures with target-typed presentation, UI runtime and registry operations plus explicit pump/background-pump methods. Give lossless transitions, latest-value state and edge-coalesced wakes distinct admission rules. True cross-thread producers use a bounded typed ingress plus a wake capability; the owner-local dispatcher remains `!Send` | move 5b |
| 5d. Transaction and host extraction (the pump has moved; acceptance of its Android, iOS and wasm sites waits on CI's `cross-typecheck`, `wasm-check` and `wasm-test`) | `UiRuntime::pump` absorbs the runners' `drive_frame_with_lane` calls: every runner's frame wake is its gate, then the pump, then its pacing; the device-recovery wrapper brackets the whole pump; the background arm is `UiRuntime::pump_background`; `flui-app`'s `RuntimeRaster` is test-only. Extract the UI runtime-neutral registry, FIFO, checkout and deferred-mutation machinery as ordinary `flui_runtime::OwnerHost`. `APP_RUNTIME` remains the sole TLS in `flui-app` and contains it beside the `WindowRegistry`, native frame drivers and other host-only state; no platform, engine, application-service or execution-pool owner moves with it. The pump verification tests remain in `flui-runtime`, where the pump lives: they reach its crate-private pipeline. Rollback: revert (no `legacy-frame-driver` feature, see `## Consequences`) | move 5c |
| 6a. Test driver on the pump (done) | `flui-testing` moves above the runtime (`order = 6`, a normal dependent the runtime admits beside `flui-app`) and absorbs `flui_widgets::testing` as `flui_testing::widgets`; `flui-widgets` loses its `testing` feature. A UI runtime takes its clock as a `ClockSource`, so one `ManualClock` drives its frame time, gesture deadlines and produce gate. `HeadlessHost` hosts a `UiRuntime` over a headless window and sink and raises a contained frame failure after the pump, the first one authoritative. The widget harness (`lay_out`, `harness::mount`) runs every frame, the mount included, through `UiRuntime::pump`; the tests that pinned what the old driver lacked (no root `MediaQuery` or `VsyncScope`, no post-frame lanes, no text-input capability) mount on `HeadlessBinding` or assert the UI runtime's behaviour | move 4 |
| 6b. Substrate suites on the pump | `HeadlessBinding::pump_frame`, `run_pipeline` and `pump_presentation`/`pump_all` are deleted; the raw-owner suites (`flui-view`, animation, scheduler, interaction, `flui-widgets`' `perf` target and its baseline, the facade's `tests/*.rs`) mount under the UI runtime's root scopes, which changes their tree shapes and needs its own review; per-presentation clocks become separate UI runtimes | move 6a |

§2 (sealing the entry points) and the widgets' inline test modules follow move 5d. Move 6 does
not wait on 5c or 5d: §4 keeps the test driver off `OwnerHost`, so it needs only the pump.

`cargo xtask reach` (ADR-0081 §2) checks the K-set fact for `flui-runtime` over every root
build: its tier K forbids `flui-platform`, `winit`, `android-activity`, `ndk`, `windows`,
`objc2-app-kit`, `objc2-ui-kit`, `wgpu`, `flui-engine` and `flui-app`, and no
`reach-exceptions` entry excuses any of them.

Two defects the UI runtime core would have carried across are fixed with move 1:

- **A `GlobalKey` read inside its own presentation's frame deadlocked.** `WidgetsBinding` holds
  its own write lock across a frame, attach, detach and layout-builder build, and the registry
  read it again from the same thread. The registry now reads without blocking and reports a
  busy member; the UI runtime composite skips it, and `GlobalKey` resolves to `None` for keys of the
  presentation whose frame is running (a Flutter divergence recorded in `flui-view`'s
  `ARCHITECTURE.md`). This made the closing-presentation exclusion in
  `UiRuntime::enter_for_close` redundant; move 4 deleted it, so a key of the closing
  presentation now resolves while it detaches, before its teardown takes the lock.
  **Unasserted:** no test pins this. Tests:
  `global_key_lookup_from_build_during_draw_frame_returns_instead_of_deadlocking` (`flui-view`)
  and `state_read_across_presentations_during_a_segment_resolves` (`flui-runtime`).
- **`ElementBase::depth` returned the sibling slot.** The tree now stamps the depth on the
  element whenever it sets a node's depth (`ElementBase::set_depth`), as Flutter's
  `Element._depth` is set in `mount` and repaired in `_updateDepth`. **Unasserted:** no test
  pins this.

## Verification

The first exists: both crates are tier K, and `cargo xtask reach` checks them on every change.
The pump tests stay in `flui-runtime`, where the pump lives (`ui_runtime/tests/pump_transaction.rs`,
driven through `flui_runtime::testing::ScriptedSink` and `flui_foundation::ManualClock`, which
implements `FrameClockSource`; they reach the UI runtime's crate-private pipeline). What the test
driver adds on top is tested in `flui-testing` (`tests/headless_host.rs`,
`tests/runtime_driver.rs`).

- `cargo xtask reach` (ADR-0081): `flui-runtime` and `flui-testing` reach none of the K set.
- `cargo xtask frame-entry --self-test` (if option 2 of §2 is used): a planted call to
  `drive_frame_with_lane` in `flui-widgets` fails the gate.
- A test that fails when a phase is missing from `UiRuntime::pump`: a post-frame callback observes
  this frame's committed layout, driven through the headless sink
  (`pump_post_frame_callback_observes_this_frames_committed_layout`).
- The ADR-0075 acceptance test, once that record is accepted: an effect runs under the product
  frame driven by the runner's code path, not only under the harness.
- A grep-free structural check (done): a `reach-forbid` fact (ADR-0081 §2) that `flui-testing`
  is absent from `flui-widgets`' normal closure with all features, `reach-forbid = ["flui-testing"]`
  in `flui-widgets`' manifest. (`cargo tree -i` cannot state it: it errors on an absent package.)
- The begin-frame phase: an animation controller driven through `UiRuntime::pump` with a manual
  clock advances its value between two frames; and the pump publishing its clock: a `Vsync`
  controller lands exactly halfway after two pumps 50 ms apart on the manual clock.
  **Unasserted:** no test pins this.
- The owner-turn regression test installs UI runtimes A and B, dispatches an A operation whose user
  callback synchronously requests B's close, and proves: A finishes first; B's close and
  Detached teardown run exactly once afterwards; B's address is gone; A remains live; and the
  host FIFO is empty. The test failed against the pre-5a nested-cross-UI runtime guard in debug builds
  and returned `NestedCrossRuntimeDispatchRejected` without closing B in release builds.
- Panic and ordering companions prove an A/B/A reentrant sequence preserves FIFO order, a stale
  presentation queued behind its close is dropped, a panic restores the checked-out UI runtime, and
  UI runtime-owning destructors run with no mutable `OwnerHost` or `APP_RUNTIME` borrow held.
