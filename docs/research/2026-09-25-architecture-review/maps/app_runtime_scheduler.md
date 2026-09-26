# Codebase map: app_runtime_scheduler — flui-app (AppRuntime, UiRealm, presentations, multi-window, runners, raster lane, execution/lifecycle services, hot-reload seam), flui-scheduler, flui-animation, flui-hot-reload

_Raw output of the `map:app_runtime_scheduler` agent (read-only review of main @ cab06137d, 2026-09-25). Unedited; claims are the agent's and were only partly re-verified._

## Summary

How it really works today (main @ cab06137d):

One platform event-loop thread per process hosts everything UI. `AppRuntime` (crates/flui-app/src/app/runtime.rs) lives in a single `thread_local! APP_RUNTIME: RefCell<AppRuntime>` (runner/host.rs:25-47) and holds a linear `RealmRegistry` of `RealmSlot { realm: Option<UiRealm>, queue, draining, address, surface_applier }` (runtime.rs:220-330). Platform callbacks are `Send` closures that re-enter that TLS through a checkout/re-entrancy FIFO (runner/realm_dispatch.rs:921 `dispatch_platform_realm`). `WindowPolicy::SeparateRealms` (default) gives each window its own `UiRealm` on the SAME thread; `SharedRealm` adds a second `PresentationState` to the first realm. So ADR-0027's "multiple realms may execute concurrently" is not implemented: N realms are N ownership domains serialized on one thread.

A `UiRealm` (ui_realm/mod.rs:145) is `!Send`, owns a `PresentationForest`, a `GlobalKeyScope`, a focus coordinator, a per-realm `UpdateScheduler` + `AsyncDriver` + `LocalPostFrameLane` (runtime.rs:167-186). Each presentation owns its own element tree/BuildOwner (hence its own signals `Reactive` graph, flui-view/src/owner/build_owner.rs:444,722), PipelineOwner, `FrameClock`, `Vsync` registry, semantics host. The frame transaction is `UiRealm::draw_frame_entered` → `render_frame_with_sink` (ui_realm/frame.rs:74,500): scheduler `drive_frame` phases, per-presentation build/layout/paint/composite, then a `SceneSnapshot` into `RasterLane`.

The raster lane is the ADR-0045 mailbox protocol but only in `RasterMode::Inline`: `RasterOwner::pump` runs synchronously on the owner thread (raster_lane.rs:1-13); there is no raster thread in production; web still uses `DirectSink` with `Arc<Mutex<Option<Renderer>>>` (runner/web.rs:85). ADR-0045 is still Proposed.

Background execution: `ExecutionServices` (execution.rs) lazily builds two tokio multi-thread runtimes (IO 2 workers, compute N workers used as a plain thread pool, execution.rs:321-345); a Task/Worker/Service lifecycle layer (lifecycle.rs, ADR-0049) sits on top, but its only entry point is `AppConfig::with_service` (config.rs:390). Views cannot reach these lanes: widget async is `AsyncDriver`, a per-frame poller on the owner thread that nevertheless requires `Send` futures (flui-scheduler/src/async_driver.rs:102). Other tokio runtimes exist in flui-platform (`BackgroundExecutor`, executor.rs:66) and flui-assets (`BridgeRuntime`, registry/bridge.rs:38), and image loading goes through flui-assets, not the app IO lane.

flui-scheduler (layer 2, ~20.6k) is a Flutter SchedulerBinding port: `UpdateScheduler` = `Arc<SchedulerInner>` with roughly 20 `parking_lot::Mutex` fields and a `DashMap` (scheduler.rs:743-855), `Send` callbacks, plus a physical-time `FrameClock` (frame_clock.rs) and a global `TIME_DILATION` atomic (config.rs:43). flui-animation drives controllers two ways: the scheduler `Ticker` (wall clock) and the `Vsync` registry (virtual time, per presentation). `AnimationController` is `Arc<Mutex<…>>`, which it records as a "scoped exception until the engine-wide !Send flip" (controller.rs:177-180).

Hot reload is the dylib worker/scene-plugin ABI (flui-hot-reload, layer 6), wired into flui-app through `app/hot_reload.rs` (enabled/disabled shims) and requiring the three-crate project split (docs/hot-reload.md). The plan (G4) has already chosen Subsecond instead, and no Subsecond code exists (grep -ri subsecond → 0 hits).

Verdict: flui-app is a composition root in its public surface, but internally it has grown past that. It owns the realm, frame transaction, frame-failure containment, input holding, semantics host, close-request veto, execution pools, service lifecycles, window registry, device and surface recovery, frame pacing, and four platform runners. That is about 34-40k production lines, with 109 dead_code expectations, 167 target cfg sites and 70 stale references to a `runner.rs` that no longer exists. Most of this is runtime machinery that should sit below the composition root so that flui-testing, embedders and devtools can reuse it.

## Responsibilities and boundaries

Owns (flui-app): process/loop host (AppRuntime TLS), realm registry and presentation→realm demux, UiRealm and its frame transaction (build→layout→paint→composite→submit), per-presentation state (FrameClock, Vsync, semantics host, held input), frame-failure containment (ADR-0048), multi-window policy (ExitPolicy/WindowPolicy, open_window/open_secondary_window, Application/AppHandle), close-request veto, execution pools + task/worker/service lifecycles (ADR-0047/0049), inline raster lane adoption, device/surface recovery, frame pacing (ADR-0058), per-OS runners (desktop, Android, iOS session controller, web), hot-reload seam, and re-export of Flutter-named "bindings".

Where the boundary leaks:
(1) The frame transaction, a runtime concept, lives at layer 9. flui-testing (layer 6) therefore re-implements it in `HeadlessBinding::pump_frame` (flui-testing/src/lib.rs:955-1079). Widget tests exercise a parallel pipeline, not the production one.
(2) The async/execution model is split three ways. Frame-thread futures are in flui-scheduler (layer 2). Pools and lifecycles are in flui-app (layer 9), so View code cannot reach them. Asset IO runs its own runtime in flui-assets.
(3) Semantics delivery (semantics_host.rs), held input (held_input.rs, 1876 lines), and surface and device lifecycle (runner/surface_lifecycle.rs 1217, device_recovery.rs 1324) are subsystem behavior housed in the root. ADR-0027 itself rejects "one god runtime object ... behavior stays in subsystems".
(4) flui-view's `runtime-internals` feature (flui-view/Cargo.toml:118) is a published, additive feature that any crate can turn on. It is a policy gate, not a type boundary.
(5) flui-scheduler is layer 2 "substrate" but carries app-level policy: AppLifecycleState listeners, performance mode, timings callbacks, budget, and the global time dilation.

What belongs elsewhere:
- A `flui-runtime` (or a module in flui-view/flui-rendering) layer below testing should hold the realm, presentation and frame transaction that both UiRealm and HeadlessBinding drive.
- Execution capability handles should sit in a low layer and be vended via LifecycleContext.
- Platform runners and window/surface recovery are the only things that must remain in the composition root.
- Hot reload is an official package per the plan, integrated through a narrow, stable hook rather than runner-level cfg forks.

## Key types and contracts

- AppRuntime (pub(crate), runtime.rs) — loop-scoped composition root held in thread_local APP_RUNTIME (runner/host.rs:25-47); owns RealmRegistry, WindowRegistry, ExecutionServices, ServiceRegistry, OwnerPlatform, loop-wide needs_redraw Arc<AtomicBool> (runtime.rs:727)
- UiRealm (pub(crate), ui_realm/mod.rs:145) — !Send single-writer owner: PresentationForest, GlobalKeyScope, FocusCoordinator, per-realm UpdateScheduler/AsyncDriver/LocalPostFrameLane (RealmServices::construct, runtime.rs:175)
- PresentationState (presentation.rs) — per-surface element tree + BuildOwner (hence per-presentation signals Reactive), PipelineOwner, FrameClock, Vsync, SemanticsHost
- RealmTask / PlatformToUi / RealmDispatcher (runner/realm_dispatch.rs:65-237) — stamped per-realm FIFO; checkout-based re-entrancy guard
- WindowPolicy {SeparateRealms, SharedRealm}, ExitPolicy {OnLastWindowClosed, ExplicitQuit} — #[non_exhaustive] public knobs (runtime.rs:400-480)
- Public entry points: run_app / run_app_with_config / run_direct / Application<V,F> + AppHandle + StartupWindow / open_window / open_secondary_window / run_app_android / run_app_ios (lib.rs, app/mod.rs:39-106)
- AppConfig — builder with window chrome, diagnostics, executors, frame_failure_handler, close_request_handler, services, worker_plugin_path (config.rs:75-222); no plugin/capability registration hook
- FrameSink {RasterLane (inline RasterOwner pump), DirectSink (web)} + SubmitVerdict (raster_lane.rs:60-120); SceneSnapshot + FrameStamp + SurfaceGeneration (ADR-0027 §5, ADR-0045)
- ExecutionServices (pub(crate)) + public HostExecutors/HostComputePool/HostIoPool/DeterministicExecutors/SpawnError (ADR-0047); TaskHandle/WorkerHandle/ServiceDefinition/ServiceContext/TaskSpawner (ADR-0049, native-only)
- Frame failure: FrameFailureHandler/Report/Kind/Disposition, SegmentPhase (ADR-0048)
- flui-scheduler: UpdateScheduler (Arc<SchedulerInner>, Send+Sync, ~20 Mutex fields), drive_frame/drive_frame_with_lane, FrameClock + DemandMask (per-presentation physical time), AsyncDriver (BoxedTask = Pin<Box<dyn Future + Send>>), LocalPostFrameLane (#[doc(hidden)] !Send), Ticker, global time_dilation()
- flui-animation: AnimationController (Arc<Mutex<Inner>>, Send+Sync exception), Vsync registry (Arc<Mutex<VsyncInner>>, tick_all on virtual time), Curves/Tween/Simulation
- flui-hot-reload: C-ABI scene/app plugin factories with ABI-token handshake (abi.rs, dynlib.rs), WorkerReload driver; flui-app side WorkerReload/RebuildHookGuard/ScenePlugin shims (app/hot_reload.rs)
- BuildContext::async_driver() / LifecycleContext capability split (flui-view/src/context/build_context.rs:106,377,406)

## Dependencies

flui-app (layer 9) depends normally on flui-view (with `runtime-internals`), flui-rendering, flui-objects, flui-types, flui-foundation, flui-log, flui-interaction, flui-scheduler, flui-painting, flui-layer, flui-semantics, flui-engine, flui-platform, flui-animation, flui-widgets, and optionally flui-hot-reload. External deps: tokio (rt-multi-thread, sync, time; non-wasm) and tokio-util, crossbeam-channel, parking_lot, pollster, raw-window-handle, android-activity. On Linux it forces winit-backend and vulkan features (crates/flui-app/Cargo.toml:83-160). It uses flui-widgets for root composition only: MediaQuery, FocusRoot, VsyncScope and NavigatorCommand (grep of flui_widgets:: in src). Inbound: the facade `flui` (src/lib.rs:157-266, which re-exports run_app, AppConfig, Application, WindowPolicy and open_window but not TaskSpawner or ServiceDefinition), and flui-testing as a dev-only cycle. flui-scheduler (layer 2) depends only on flui-foundation, dashmap, event-listener and parking_lot. Its dependents are animation, rendering, view, widgets, testing, devtools, material, cupertino and app. flui-animation (layer 3) depends on types, foundation, scheduler and macros. flui-hot-reload (layer 6) depends on flui-layer, plus optional view (runtime-internals), rendering and foundation. Its dependents are flui-app (optional) and flui-cli. Tokio also appears independently in flui-platform (BackgroundExecutor), flui-assets (BridgeRuntime) and flui-widgets (optional, via assets).

## Fit with the plan

H0 (beta):
- D3 (async Task/Worker, cancel on unmount, FutureBuilder on signals) is blocked by layering. The Task/Worker types live in flui-app (layer 9), so a View cannot obtain one. The ADR-0047 promise that "realms and presentations will receive capability handles" never reached LifecycleContext.
- G7 (record/replay) and golden tests (E7) need one frame transaction. Today there are two: UiRealm and HeadlessBinding.
- G4 (Subsecond) has no code. The existing dylib/3-crate design, which the plan explicitly rejects, is still wired into the flui-app runners.
- B0 (no file >3000 lines in flui-app) is met for ui_realm, but realm_dispatch.rs is 7149 lines, mostly tests.
- A5 (idle = 0 fps, unified demand #1172) is not structurally unified. Demand has at least five sources: FrameClock DemandMask, loop-wide needs_redraw, scheduler frame_scheduled/on_frame_scheduled, the Vsync registry keeping the loop alive, and AsyncDriver wakes.

H1:
- PlatformCapability plugins have no registration seam in AppConfig/AppRuntime (config.rs:75-222). There is also no in-process agent/inspector hook: devtools depends only on scheduler/foundation, and desktop-mcp drives the app from outside via UIA.
- The mobile runners exist (android.rs, ios.rs with a session controller), but the lifecycle services are native-only and web lacks the raster lane and services, so the platform matrix of runtime features is uneven.

H2 (raster/IO lanes in production, damage, multi-window as the norm):
- The raster lane protocol is ready but inline only (ADR-0045 Proposed).
- The IO lane exists but images bypass it (flui-assets runtime).
- Multi-window is multiple realms on one thread. The ADR-0027 verdict of concurrent realms plus the H2 "parallel layout inside realm?" question presuppose owner threads that do not exist.

H3 (API freeze by tiers):
- The public surface is fragmented. Entry points: run_app, run_app_with_config, run_direct, Application, open_window, open_secondary_window, run_app_android, run_app_ios.
- Flutter-named `bindings` re-exports (WidgetsBinding, RenderingFlutterBinding, PipelineOwner, UpdateScheduler) are public and would freeze a Dart-shaped surface.
- Cargo features `desktop`/`android`/`ios`/`web`/`debug-overlay`/`performance-overlay` are declared but unused in code.

Principle 3 (no global state):
- It is violated in this area by TIME_DILATION (flui-scheduler/src/config.rs:43).
- The widgets decode cache static (flui-widgets/src/image/decode_cache.rs:96) also violates it.
- The docs/runtime-contract.toml ratchet the roadmap cites no longer exists and no xtask gate replaces it.

Delivery layers: hot reload is planned as an official package, but it is feature-forked into the runner (app/hot_reload.rs). Material and Cupertino are not in this scope.

## Strengths

- ADR-0027 is a genuinely good, market-aware runtime model: !Send single-writer realms, typed Send capabilities, reliability-classed lanes (bounded owner inbox, coalesced invalidations, latest-frame-wins snapshot mailbox, one-shot shutdown), channel identity as lifetime boundary, freshness per work class — and it explicitly leapfrogs Flutter's process-global bindings
- Process-global singletons of the old AppBinding/SchedulerBinding/SemanticsBinding era are actually retired: each realm constructs its own UpdateScheduler (runtime.rs:167-186), semantics enablement is per presentation (semantics_host.rs), GlobalKey scope is per realm (ADR-0043)
- Raster boundary is protocol-first: every desktop/Android frame already crosses as an owned SceneSnapshot with FrameStamp + SurfaceGeneration and typed SubmitVerdict (raster_lane.rs:60-120), so threading the lane later changes who calls pump, not what a frame is
- Execution services have a real, careful contract: work classes instead of priorities, bounded admission with typed Saturated/ShuttingDown/Unavailable, cancel-then-join shutdown under deadline, runtime-neutral host injection (HostComputePool/HostIoPool) and a DeterministicExecutors reference implementation (ADR-0047, execution.rs)
- Frame-failure containment (ADR-0048) and typed frame disposition (ADR-0068) give per-presentation fault isolation that Flutter does not have
- Multi-window policy is instantiation, not architecture: SeparateRealms vs SharedRealm and ExitPolicy are #[non_exhaustive] knobs with drain-before-decide exit semantics (runtime.rs:400-480)
- AsyncDriver is a dependency-free, deterministic, coalescing frame-thread executor with cancel-on-drop tokens and readiness index — good substrate for FutureBuilder/signals async
- Scheduler has recorded, test-pinned re-entrancy bounds (watermark / count budgets / microtask cap) with measured failure modes documented in ARCHITECTURE.md
- Heavy investment in tests of realm seams (ui_realm/tests/*, realm_dispatch tests, frame_clock_segment_gate) — the runtime is well-pinned even where the shape is wrong

## Problems

### Realms are not concurrent: all realms of a process are serialized on one platform thread inside a thread-local

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** runner/host.rs:25-47 `thread_local! APP_RUNTIME: RefCell<AppRuntime>`; runtime.rs:220-330 RealmRegistry is a Vec of RealmSlot on 'this thread'; WindowPolicy doc (runtime.rs ~440) 'the realm(s) already hosted on this thread'; secondary windows go through install_realm_alongside into the same TLS (realm_dispatch.rs:591). ADR-0027 verdict: 'Multiple realms may execute concurrently'; its own open question calls the TLS slot 'the sanctioned transitional form'.
- **Impact:** The core claim of the runtime ADR is not realized. A slow frame in window A stalls every other window, including SeparateRealms, which only isolates state, not time. H2 'multi-window as the norm' and the 'parallel layout inside realm?' spike rest on owner threads that do not exist. The platform callback surface still requires `Send` closures that reach into TLS, which is the event-loop inversion ADR-0027 leaves open.
- **Direction:** Decide explicitly, as an ADR amendment, whether per-realm owner threads are a goal before H2. If they are, introduce an `OwnerExecutor` abstraction (the platform main thread on AppKit/UIKit/wasm, dedicated threads on Win32/Linux/headless) and move the realm registry out of TLS into it. If they are not, rewrite the ADR-0027 verdict to say one owner thread with N isolated realms, and stop citing concurrency as a property.

### The frame transaction lives in the composition root, so tests run a second, re-implemented pipeline

- **Kind:** layering · **Severity:** high
- **Evidence:** UiRealm::draw_frame_entered / render_frame_with_sink (flui-app/src/app/ui_realm/frame.rs:74,500), layer 9. flui-testing (layer 6) HeadlessBinding::pump_frame (flui-testing/src/lib.rs:955-1079) re-sequences deadlines→controllers→build_scope→layout/paint around scheduler.drive_frame_with_lane, with comments asserting it matches what 'the desktop / android / wasm runners call' (lib.rs:997). The flui-app→flui-testing dev-dependency cycle is documented in flui-app/Cargo.toml.
- **Impact:** Widget tests, golden tests (E7), semantic goldens (G3), record/replay (G7) and agent scenarios exercise an ordering that production does not share. Held-input replay, frame-failure containment and per-presentation Vsync ticking are not in the headless path unless copied. Every runtime change must be done twice, and this is exactly the 'passes both ways' test risk AGENTS.md warns about. Record/replay needs one deterministic transaction to replay into.
- **Direction:** Extract a platform-free `Realm`/`Presentation`/frame-transaction core into a layer ≤5, either a new flui-runtime crate or a module of flui-view with rendering, parameterized by a clock and a FrameSink. UiRealm becomes that core plus platform wiring, and HeadlessBinding becomes the same core with a ManualClock and a headless sink.

### flui-app is drifting into a god crate: subsystem behavior housed in the root

- **Kind:** layering · **Severity:** high
- **Evidence:** ~34-40k production lines (52k incl. in-file tests). Modules include held_input.rs 1876, presentation.rs 1865, lifecycle.rs 2656, runtime.rs 2348, execution.rs 1504, semantics_host.rs 666, close_request.rs 864, runner/device_recovery.rs 1324, surface_lifecycle.rs 1217, frame_pacing.rs 1274, main_window.rs 1477, secondary_window.rs 1604 and realm_dispatch.rs 7149. There are 109 `dead_code` expectations and 167 target_os/target_arch cfg sites in src/. ADR-0027 'Alternatives rejected' says 'One god runtime object — UiRealm/AppRuntime own, wire and vend capabilities; behavior stays in subsystems'.
- **Impact:** Everything below layer 9 is unusable without flui-app's runners: embedders, devtools, A2UI renderer and a future PlatformCapability host. Contributors face the densest, most cfg-forked code in the repo, which hurts bus factor and the H3 'external contributor finishes a feature' exit. Coupling to four platform runners makes each runtime change a four-platform change.
- **Direction:** Split by responsibility, not size. Realm, presentation and frame core go down a layer (previous problem). Execution and lifecycles go to a low 'services' module/crate vended via capabilities. Semantics host goes next to flui-semantics, and held input next to flui-interaction. flui-app keeps only runners, window/surface/device lifecycle and policy wiring. Target: flui-app under 15k lines and only composition.

### No coherent async model: Task/Worker lanes unreachable from Views, AsyncDriver demands Send, 3-4 tokio runtimes

- **Kind:** runtime_architecture · **Severity:** high
- **Evidence:** TaskSpawner/ServiceDefinition are used only inside flui-app, with AppConfig::with_service (config.rs:390) as the sole entry and no facade re-export (src/lib.rs:157-266). LifecycleContext offers only `async_driver()` (flui-view/src/context/build_context.rs:406). AsyncDriver BoxedTask = `Pin<Box<dyn Future<Output=()> + Send>>` (flui-scheduler/src/async_driver.rs:102) although it polls only on the owner thread; ADR-0027 §9 says widget authors use Rc/RefCell freely. Tokio runtimes: flui-app IO + compute (execution.rs:321-345), flui-platform BackgroundExecutor (executor.rs:66, 'slated for removal' per ADR-0047), flui-assets BridgeRuntime (registry/bridge.rs:38). AssetImage/NetworkImage resolve through flui-assets, not the app IO lane (flui-widgets/Cargo.toml:71-74,169).
- **Impact:** This blocks H0 D3 (Task/Worker, cancel on unmount, FutureBuilder on signals) and the plan's 'explicit async model, tokio optional'. Tokio is effectively mandatory on native and duplicated. An app author cannot write `cx.spawn_io(...)` from a widget. The Send bound pushes Arc/Mutex into UI futures, contrary to the realm model. H2 'IO lane in prod' has a lane with no widget consumer.
- **Direction:** Write one async ADR that supersedes the ADR-0047/0049 edges. A runtime-neutral `Spawner` capability trait lives low (foundation or scheduler), and the realm vends it through LifecycleContext with cancel-on-unmount. AsyncDriver gets a `!Send` local-future variant, making the owner lane the default for UI futures. flui-assets takes an injected IO spawner instead of its bridge runtime. Delete BackgroundExecutor/Task/Priority from flui-platform. Put tokio behind a feature of the default executor.

### Per-realm UpdateScheduler is a lock-heavy Send+Sync singleton-era object inside a single-writer domain

- **Kind:** performance · **Severity:** medium
- **Evidence:** flui-scheduler/src/scheduler.rs:743-855 lists SchedulerInner with ~20 parking_lot::Mutex fields plus DashMap `cancelled`, `frame_thread: Mutex<Option<ThreadId>>` and `Box<dyn FnOnce() + Send>` microtasks/idle. `UpdateScheduler` is Arc<SchedulerInner> (scheduler.rs:1006). The existence of scheduler/lock_discipline_tests.rs (828 lines) and the `#[doc(hidden)]` `LocalPostFrameLane` workaround for !Send callbacks (post_frame.rs:36-45) point the same way. Each realm owns its own scheduler (runtime.rs:175).
- **Impact:** AGENTS.md says 'locks guard shared infrastructure only' and 'a lock on per-node state touched inside perform_layout/paint puts contention on every frame', yet every frame takes dozens of uncontended lock acquisitions and heap-allocated Send closures. The dual Send/local API surface is confusing and will be frozen at H3. Re-entrancy bugs (#1057, #1058, #1159) came from lock-scoped queue draining.
- **Direction:** Split UpdateScheduler into an owner-affine `!Send` core (Cell/RefCell queues, local callbacks) and a tiny `Send` `SchedulerWaker`/request handle (atomic flag plus wake callback) for cross-thread demand. Then delete LocalPostFrameLane and the Send bounds on callbacks. Measure with the existing frame_clock_gate/async_driver_pump benches.

### Frame demand is spread over five mechanisms; the loop-wide redraw flag is shared across realms

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** FrameClock DemandMask + try_arm_redraw_request (flui-scheduler/src/frame_clock.rs header). AppRuntime `needs_redraw: Arc<AtomicBool>` is loop-scoped (runtime.rs:727,822) and cloned into every UiRealm (ui_realm/mod.rs 'a clone of AppRuntime's own needs_redraw flag'). Scheduler `set_on_frame_scheduled` wake hook (runtime.rs:2048). The Vsync registry 'keeps the frame loop alive until the last running controller completes' (ui_realm/frame_clock.rs:36-40). AsyncDriver wakes call request_frame. frame_pacing.rs (1274 lines) holds a separate FallbackWake policy. The roadmap A5/#1172 'frame demand unified' is still open.
- **Impact:** The H0 budget 'idle app = 0 fps' and H2 damage/partial repaint need one authoritative per-presentation demand signal. With a loop-wide flag, one realm's demand is observable as every realm's demand (hypothesis: extra wakes/frames for idle siblings; not measured). Diagnosing a spurious frame requires reading five subsystems.
- **Direction:** Make FrameClock's DemandMask the single per-presentation authority. The scheduler, AsyncDriver, Vsync and input only `mark_demand(reason)` on their presentation's clock. The loop wake becomes a pure 'some clock is armed' edge. Add a test/bench asserting zero produced frames over N idle seconds per presentation.

### Two animation clocks: scheduler Ticker (wall time) and Vsync registry (virtual time), plus a process-global time dilation

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** flui-animation/src/vsync.rs:1-20: 'AnimationController also carries an auto-scheduling Ticker that advances it off wall-clock Instant::now() ... Vsync bypasses that ticker entirely'. AnimationController is Arc<Mutex<Inner>> with a 'scoped exception until the engine-wide !Send flip' (controller.rs:177-180,207-208). TIME_DILATION is a static AtomicU64 (flui-scheduler/src/config.rs:43), read in controller.rs:1803,2419.
- **Impact:** Determinism (the plan's 'determinism in tests' and record/replay) depends on which path a controller uses. A controller created explicitly with `AnimationController::new(duration, &scheduler)` ticks on wall time unless also registered with Vsync. The global dilation breaks principle 3 (no global state, per-realm everything) and would bleed across realms and in-process test threads. The Send+Sync controller shape will be frozen at H3.
- **Direction:** Keep one clock source per presentation: a FrameClock-provided frame timestamp, virtual in tests, from which every controller advances; retire the self-scheduling wall-clock Ticker path. Move time dilation into the realm or presentation clock. Make AnimationController `!Send` (Rc/Cell) as ADR-0027 intends, and record the flip as an ADR.

### Raster lane exists only inline; ADR-0045 still Proposed and web is on a different sink

- **Kind:** missing_capability · **Severity:** medium
- **Evidence:** raster_lane.rs:1-13: pump 'runs synchronously on the owner (UI) thread — RasterMode::Inline'. raster_lane.rs FrameSink doc: DirectSink 'still used by the web runner'. runner/web.rs:85 `Arc<Mutex<Option<Renderer>>>`. ADR-0045 Status 'Proposed — accepted when surface acquisition and present run on the raster thread'. macOS is pinned inline by a wgpu-hal constraint (ADR-0045 §1).
- **Impact:** The owner thread still blocks on acquire/present (Fifo), so the UI budget includes GPU pacing. H2 'raster lanes in prod' is unstarted in practice. Two sink paths (lane and direct) mean two frame-submit behaviors to keep equivalent. Because macOS is structurally inline, the 'threaded lane' will never be the uniform shape.
- **Direction:** Accept the lane as mode-agnostic and make Inline a first-class, permanent mode (macOS/wasm). Move web onto the lane protocol by driving an async-ready backend through the same FrameSink, and delete DirectSink. Run the threaded mode on Win32/Linux behind a flag with the CI pacing budget ADR-0045 names as its acceptance test.

### Hot reload architecture contradicts the chosen direction (Subsecond) and is feature-forked through the runners

- **Kind:** plan_misfit · **Severity:** medium
- **Evidence:** flui-hot-reload is a C-ABI dylib scene/app plugin system (crates/flui-hot-reload/ARCHITECTURE.md; src/abi.rs, dynlib.rs, worker.rs). docs/hot-reload.md requires the three-crate `-types/-logic/-host` split. flui-app/src/app/hot_reload.rs keeps enabled/disabled shims per target (WorkerReload desktop, ScenePlugin Android 'may take over a frame entirely'). The roadmap (G4) chooses Subsecond and says 'свой движок и ручной разрез на 3 крейта не делаем' (we will not build our own engine or do the manual 3-crate split). `grep -ri subsecond` over the repo's .rs/.toml finds nothing.
- **Impact:** The H0 B1 exit ('flui run --hot preserves state on macOS and Windows') has no implementation path in code. The current seam hard-codes a plugin shape into the runners, and a scene plugin can bypass the frame transaction. Under the plan's delivery layers, hot reload is an official package, but here it is a cfg-forked part of the core runner.
- **Direction:** Define a narrow `DevReloadHook` in the realm core: a Subsecond `call` around the build entry plus an owner-queued 'reassemble' command, which ADR-0027 §9 already lists as a command. Let the official hot-reload package implement it. Retire the dylib ABI, ScenePlugin and the three-crate template once Subsecond proves state preservation; keep hot restart as the fallback the roadmap names.

### Signals are 'realm-scoped' by ADR but presentation-scoped in code

- **Kind:** runtime_architecture · **Severity:** medium
- **Evidence:** ADR-0074 says 'every node owned by one UiRealm' and the graph is 'dropped with the realm' (lines 82,128). The code puts `reactive: Reactive` in BuildOwner (flui-view/src/owner/build_owner.rs:444,722). Each presentation owns its own BuildOwner/element tree (ADR-0043; presentation.rs:503 with_build_owner_mut). The UiRealm struct has no Reactive field (ui_realm/mod.rs:145-200).
- **Impact:** Under WindowPolicy::SharedRealm (one session across windows, the case that policy exists for) an app-level signal cannot be shared between presentations. Per ADR-0074 line 129, graph ids are process-unique, so cross-graph use is an error (hypothesis: SignalError on cross-presentation read; not executed). This undermines A3/A4 as the canonical state layer before it ships.
- **Direction:** Move the Reactive graph to the realm, as one graph shared by the realm's presentations, with element-level reader tracking keyed by (presentation, element). Otherwise amend ADR-0074 to say presentation-scoped and provide an explicit realm-level store. Decide this before the signals feature leaves `off by default`.

### No extension seams in the runtime for PlatformCapability plugins, agent/inspector protocol, or embedders

- **Kind:** extension_point · **Severity:** medium
- **Evidence:** AppConfig fields (config.rs:75-222) cover window chrome, diagnostics, executors, frame failure, close request, services and worker_plugin_path; there is no capability/plugin registry. `UiRealm`, dispatcher and command protocol are pub(crate) by ADR-0027 §9 ('A public realm/runtime surface is designed separately'), and it has not been designed. flui-devtools depends only on flui-scheduler/flui-foundation (optional) and has no link to flui-app. tools/desktop-mcp drives apps externally through UIA.
- **Impact:** H1 PlatformCapability (typed native capability, `Unsupported` when absent) needs a runtime registration and vending point, which does not exist. Principle 4 (machine-readable frame events, DevTools as a client of the agent protocol) needs an in-process observation seam on the realm (frame telemetry, tree snapshots, semantics). Embedders (H4 kiosks/embedded) have only HostExecutors.
- **Direction:** Design the public runtime surface as an ADR before H1. Registration at app build time: `AppBuilder::with_capability<C: PlatformCapability>()`, with the realm vending capabilities via LifecycleContext. A read-only `RealmObserver` gets frame telemetry, semantics snapshots and tree diffs; devtools, MCP and record/replay subscribe to it. An `Embedder` trait covers bring-your-own event loop and surface.

### Public API fragmentation and Dart-shaped surface in the composition root

- **Kind:** api_dx · **Severity:** medium
- **Evidence:** Entry points: run_app, run_app_with_config, run_direct, Application<V,F>::new().run(), open_window, open_secondary_window, run_app_android(_with_config), run_app_ios(_with_config) (lib.rs, app/mod.rs:39-106). Examples: 23 use run_app, 3 Application::new, 10 open_window/open_secondary_window. `pub mod bindings` re-exports GestureBinding, PipelineOwner, PipelineCell, RenderingFlutterBinding, UpdateScheduler, WidgetsBinding (bindings/mod.rs; lib.rs prelude). `pub mod runner` and `pub mod direct` are public modules. Cargo features desktop/android/ios/web/debug-overlay/performance-overlay are declared (Cargo.toml features) with zero `feature = "..."` uses in src.
- **Impact:** The H3 API freeze would lock a Flutter-binding-shaped surface and several overlapping startup paths. Contributors and agents cannot tell which entry point is canonical, which costs time to first screen. Dead features look like switches that do nothing.
- **Direction:** Choose one builder: `flui::App::new(root).config(..).run()` with window/realm policy on it, and `run_app` as a one-line sugar. Make `bindings`, `runner` and `direct` crate-private or `unstable`. Delete unused features. Put the surface under the H3 tiering: Stable is App/AppConfig/WindowPolicy, Experimental is run_direct/HostExecutors.

### Process-global state and the ambient-reach ratchet are gone from the gates

- **Kind:** safety · **Severity:** low
- **Evidence:** The roadmap cites `docs/runtime-contract.toml` and a 'ratchet ambient-reach = 0'; docs/ has no such file and tools/xtask/src has no ambient/runtime-contract check (grep returned nothing). Remaining statics include flui-scheduler TIME_DILATION (config.rs:43), flui-widgets image decode `static CACHE: LazyLock` (decode_cache.rs:96), and the FONT_SYSTEM OnceLock initialized from SharedEngineServices::resolve (runtime.rs:137-150, 'The read path stays ambient on layout hot paths').
- **Impact:** Principle 3 ('ratchet ambient-reach = 0') has no enforcement. New globals can land silently, and multi-realm and parallel-test guarantees erode.
- **Direction:** Reinstate the ratchet as `cargo xtask` (inside `checks`), for example as a clippy disallowed-types/disallowed-methods list for `static` + LazyLock/OnceLock/Mutex in framework crates with an allowlist file whose count may only drop. Alternatively use a dylint, per 'make rules types, not reviews'.

### SharedEngineServices is an empty seam; composition-root docs describe a history, not the current shape

- **Kind:** tech_debt · **Severity:** low
- **Evidence:** runtime.rs:78-151: the only field `accessibility_features` has three `expect(dead_code, reason = "this change only creates the resolution seam; a later change wires the first real consumer")`. The module doc speaks of 'this change', 'runner.rs' and 'transitional RealmHost' (runtime.rs:1-31). There are 70 references to the nonexistent `runner.rs` across flui-app/src, 115 'issue #NNN' mentions and 109 dead_code expectations. Also lib.rs:43 '// Ship bar (wave 4)', a process marker banned by AGENTS.md.
- **Impact:** ADR-0027's SharedEngineServices (GPU device/queue, ImageCache, font service, pools) is not where the plan says. GPU and fonts are still owned elsewhere or globally. Misleading docs make the densest crate harder for agents and new contributors to reason about, which works against the H3 external-contributor exit.
- **Direction:** Either make SharedEngineServices real (own the font system and glyph atlas per the A2/B1 per-realm FontSystem work, the GPU instance and adapter, and image cache handles) or delete it and document where each shared service actually lives. Do a doc sweep that states invariants instead of change history.

### Platform runtime parity is uneven: services and raster lane are native-desktop-first, mobile/web runners diverge

- **Kind:** runtime_architecture · **Severity:** low
- **Evidence:** Task/worker/service lifecycles are `#[cfg(not(target_arch = "wasm32"))]` (lib.rs:61-67). Execution has a wasm `expect(dead_code)` (execution.rs:57-68). Web uses DirectSink and cannot detach its owner host (ADR-0027 §7 last paragraph). iOS does not compile AppRuntime realm hosting ('not available on iOS, where AppRuntime/UiRealm's realm-hosting machinery itself is not compiled', lib.rs:78-80) and uses a separate session_controller. Android and web do not install the exit-policy hook (runtime.rs ExitPolicy doc). Runner sizes: desktop 809 + main_window 1477 + secondary_window 1604, android 704, ios 551, web 438.
- **Impact:** 'Proof, not assertion' per platform becomes four different runtimes to prove. H1 (mobile beta) and the web-beta candidate inherit runtime gaps: no services, no lane, no detach. Each fix lands per runner.
- **Direction:** Once the realm/frame core is extracted, make each runner a thin `PlatformHost` adapter implementing one trait (install owner, wake, surface events, lifecycle), with a conformance test suite run against headless, then per platform via cross-typecheck and live smoke.

## Unwired or dead surface

- SharedEngineServices::accessibility_features / accessibility_features() / set_accessibility_features() — `expect(dead_code)` 'a later change wires the first real consumer' (flui-app/src/app/runtime.rs:78-130)
- flui-app Cargo features `desktop`, `android`, `ios`, `web`, `debug-overlay`, `performance-overlay` — declared, no `feature = "..."` use in src (grep returned only a test comment)
- Task/Worker/Service public API (TaskSpawner, TaskHandle, WorkerHandle, ServiceContext, service_events…) re-exported from flui-app but not from the `flui` facade and unreachable from View code; only production entry is AppConfig::with_service (config.rs:390)
- ExecutionServices on wasm32 — compiled 'for API parity but nothing drives them' (execution.rs:57-68)
- flui-platform BackgroundExecutor / Task / Priority — 'compatibility surface (slated for removal)', FLUI-managed runs never start it (flui-platform/src/executor.rs:13-20; ADR-0047)
- UiRealm::renderer() accessor — expect(dead_code) 'exists for tests and future external callers' (ui_realm/frame_clock.rs:18-28)
- RealmMapMutation::Install and RealmRegistry::get — desktop-only, dead on android/ios/wasm (runtime.rs:280-300,378-386)
- AppRuntime direct `wake` convenience — expect(dead_code) outside tests (runtime.rs:1457-1470)
- ADR-0045 threaded raster mode (`run_until_shutdown` driver) — protocol exists in flui-engine, no production caller; only RasterMode::Inline is wired (raster_lane.rs:1-13)
- `signals` feature on flui-app/flui-view — off by default pending ADR-0074 go/no-go
- flui-hot-reload ScenePlugin path (Android) and worker dylib path — live but slated to be superseded by Subsecond (roadmap G4)

## Open questions

- Is per-realm owner-thread concurrency still a goal? If it is, what is the owner-executor model per platform (AppKit/UIKit must stay on main)? If it is not, ADR-0027's verdict should be amended before H2 plans rely on it.
- Where should the extracted realm/presentation/frame-transaction core live: a new `flui-runtime` crate at layer 5, or a module of flui-view (which already owns BuildOwner and the lifecycle contexts)? The owner's 'layers, not micro-crates' rule suggests one of these but not which.
- Should the signals `Reactive` graph be per realm (as ADR-0074 says) or per presentation (as the code does)? This decides SharedRealm semantics for app state.
- Is tokio kept as the default executor behind a feature, or replaced by a smaller pool (std threads plus a minimal IO reactor) with tokio only when the host injects it? What happens to flui-assets' BridgeRuntime?
- Can AsyncDriver drop the `Send` bound on tasks, given it polls only on the owner thread and ADR-0027 §9 promises Rc/RefCell in UI code? Are there cross-thread wake paths that need Send futures?
- Which of the ~20 locks in SchedulerInner are genuinely cross-thread (wakes, lifecycle from platform) and which are owner-only? A measured split is needed before any rewrite.
- Does the loop-wide `needs_redraw` flag cause observable extra frames in idle sibling windows? This is unmeasured and should be checked with frame telemetry across two SeparateRealms windows.
- Does Subsecond's `call` boundary compose with the owner-queued reassemble command and the frame-failure containment? What is the hot-reload story for web and iOS, which the roadmap says will be published as limits?
- What is the public runtime surface for H3 tiering: which of Application/AppHandle/open_window/run_direct/HostExecutors are Stable and which are Experimental?

