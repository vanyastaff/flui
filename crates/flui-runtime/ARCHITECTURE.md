# flui-runtime Architecture

The frame runtime of [ADR-0083](../../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) §1: the
UI realm (`ui_realm::UiRealm`) and the per-presentation frame machinery it
drives, placed below the hosts (`flui-app`'s runners, platform wiring, raster
lane and realm dispatch layer) and above the widget spine. It arrived in
steps; the ADR's `## Migration` section lists them, and what is still to move
(the realm-neutral owner host and typed operation application). Native-window
demultiplexing and the platform/raster tails of an owner turn remain in the
host.

## Invariants

- **No host, platform or GPU edge.** The crate's normal dependency closure
  names none of `flui-platform`, `winit`, `android-activity`, `ndk`,
  `windows`, `objc2-app-kit`, `objc2-ui-kit`, `wgpu`, `flui-engine` or
  `flui-app`: tier K's forbid set in the root
  `[workspace.metadata.flui.reach]`, checked by `cargo xtask reach` over
  every root build. A seam that needs a
  host type (the frame sink, the platform window) crosses as a trait this
  crate defines or one from `flui-platform-api`; a window's accessibility
  bridge is `flui_semantics::platform::PlatformAccessibility` (ADR-0082 §2,
  amended), and the development reload tier is this crate's own
  `reload::ReloadTier`, which the host translates from its hot-reload driver.
- **A realm is owner-affine.** `UiRealm` is `!Send + !Sync` (a raw-pointer
  `PhantomData` marker; pinned by `assert_not_impl_any!` in the realm tests).
  Everything that crosses a thread goes through a `UiCommandSender` into a
  bounded inbox that the owner drains only while the scheduler is idle.
- **The owner host is an ordinary owner-affine value.** The planned
  `OwnerHost` is `!Send + !Sync` and owns only the realm registry, one
  host-wide FIFO of typed operations carrying `PresentationAddress`, realm
  checkout/restore state and deferred realm-map mutations. It owns no TLS,
  native-window registry, platform capability, surface applier, engine/raster
  object, application service or execution-pool lifetime. `flui-app`'s sole
  `APP_RUNTIME` trampoline contains it beside those host-only owners.
- **Owner work is non-reentrant across realms.** A reentrant operation for any
  realm appends to the same FIFO; only the outermost owner turn executes it.
  Before ADR-0083 move 5a, `flui-app` rejected realm B while realm A was
  checked out, which could lose B's UI close after its native window had
  already closed. The app now provides the host-wide FIFO behavior that the
  extraction must preserve. `flui-app` now executes at most 32 logical
  operations per continuation callback, sharing the budget between fresh
  native roots and carried FIFO entries, counts stale entries against it, and
  requests one sequence-stamped continuation opportunity when work remains.
  A fresh native root stays synchronous while budget remains, while a close fences its exact
  `PresentationAddress` from later work at admission time so root priority
  cannot let input jump a deferred terminal operation. The remaining
  pre-extraction work is the closed operation vocabulary and its
  operation-specific admission rules.
  Checkout restoration and deferred mutations are unwind-safe, and
  realm-owning values are dropped after every mutable host borrow is released.
- **Owner operations form a closed vocabulary.** Input, lifecycle, normalized
  metrics, close, install, uninstall, pump and background-pump are typed
  operations or explicit methods. The host accepts no arbitrary
  `Box<dyn FnOnce(&UiRealm)>`, `Box<dyn FnOnce(&mut UiRealm)>`, `dyn Any` or
  executor job. A bounded typed ingress plus a wake capability is the only
  cross-thread path; an owner-local dispatcher is `!Send` and appends to the
  same FIFO. Operation-specific admission distinguishes lossless transitions,
  latest-value state and edge-coalesced wakes.
- **Every entry composes every presentation.** `UiRealm::enter` activates a
  `GlobalKey` registry composite over all the realm's presentations for the
  whole dynamic extent of the call, closing included. A binding whose own
  lock is held reports itself busy and the composite skips it (`flui-view`'s
  `key::registry`), so no entry needs to exclude a presentation (pinned by
  `closing_presentations_own_key_resolves_while_it_detaches` and
  `drawer_style_state_read_during_the_realm_frame_does_not_deadlock`).
- **A failed presentation segment is contained to that presentation.**
  `draw_frame_entered` runs each presentation's segment under its own
  `catch_unwind` (ADR-0048); a panic or structured pipeline error is
  reported through `frame_failure` and re-dirties only that presentation,
  and siblings still frame (pinned by
  `an_escaped_segment_panic_is_contained_to_its_own_presentation_and_the_sibling_still_frames`).
- **A realm's frame runs only through `UiRealm::pump`.** The pump takes
  `&mut self`, so no second frame on the same realm can start while one runs,
  and it enters the realm itself for the whole transaction, in this order:
  apply commands (the owner inbox, at the Idle boundary) → begin frame →
  draw frame (persistent callbacks, then the pipeline and the submit through
  the sink) → end frame (both post-frame queues, the realm's owner-local lane
  included). Its clock is read once: that timestamp is the scheduler's
  frame time and the time `Vsync` controllers tick at, though a scheduler
  `Ticker` still reads the wall clock (see "`Vsync` controllers tick at the
  frame's timestamp" below).
  A wake with frames disabled runs `UiRealm::pump_background` instead: clear
  the frame latch, then poll the async driver, no frame. Whether a wake
  becomes a pump is the host's per-backend wake gate (ADR-0058), not the
  realm's. Pinned by `ui_realm/tests/pump_transaction.rs`, each test failing
  against a pump that skips or reorders the phase it names; `flui-app`'s
  `runner_frame_ordering` scan pins that every runner goes through it.
  Outside this crate the invariant holds by type: the draw step
  (`render_frame`) is crate-private and the owner-local post-frame lane
  (`local_post_frame_lane`) is `test-support` only, so a host has no way to
  draw a frame, or end one, except the pump. Tests of the draw step alone
  reach it through `render_frame_for_test` and `draw_frame`, both under
  `test-support`.
- **The realm renders through a sink, never an engine.** `UiRealm::pump`
  takes any `&mut dyn FrameSink`; the host picks one (`flui-app`'s raster
  lane, or its direct sink over a borrowed backend on the web runner), and
  the realm tests pick `testing::ScriptedSink`. How a host maps its
  backend's outcomes to verdicts is that host's to test.
- **Internal, and only the host depends on it.** Tier K,
  `tier-kind = "internal"`: nothing here is an embedder API (ADR-0027 §9)
  except the `execution` host-injection seam below.
  `allowed-dependents = ["flui-app"]` makes `flui-app` the only crate allowed a
  normal edge, checked by `cargo xtask workspace`. That rule is what
  keeps ADR-0047's invariant true now that `ExecutionServices` is `pub`: only
  a host crate, one of the runtime's `allowed-dependents`, constructs the
  services, and no other workspace crate reaches the pools. ADR-0083 §4 adds
  `flui-testing` to that list when the test driver runs the real frame, as a
  host of its own headless loop. Dev edges are not restricted.
- **The execution host-injection seam carries the Stable promise.**
  `HostExecutors`, `HostComputePool`, `HostIoPool`, `ComputeJob`, `IoFuture`,
  `SpawnError` and `DeterministicExecutors` are defined in `execution` but
  re-exported as `flui_app::…` and, through the facade's
  `pub use flui_app as app`, as `flui::app::…`; `AppConfig::with_executors`
  takes `HostExecutors`. The promise follows the item, not its crate's
  `tier-kind` (ADR-0089 §1), so a change to any of these signatures is a
  breaking change of `flui` (`SpawnError` is `#[non_exhaustive]`, so a new
  variant is not).
  The rest of `execution` (`ExecutionServices`) is reached
  only by `flui-app` and carries no promise. `execution_public_paths` in
  `flui-app` pins the re-exported paths.
- **The test driver shares the transaction, not the production host.** Once
  `flui-testing` moves above this crate it drives `UiRealm::pump` directly
  with its manual clock and headless sink. It neither constructs `OwnerHost`
  nor reproduces native event-loop routing; owner-turn behavior is tested in
  this crate's own host tests.
- **Per realm, per presentation or per host loop, never per process.** Every
  type here is owned by one realm (`UiRealm`, its scheduler and command
  inbox), one presentation (`PresentationState`, `HeldPointerQueue`,
  `SemanticsHost`, `PerformanceStats`, the commit epoch) or, for
  `ExecutionServices`, one host loop, constructed only by the host's
  composition root. The one static is `realm_services`' incarnation counter,
  a monotonic ID counter the globals gate exempts.
- **The realm's surface is the host's, not an embedder's.** `UiRealm`,
  `PresentationState` and their methods are `pub` only where `flui-app`
  calls them; what only `flui-app`'s tests call is `pub` under
  `test-support`; the rest is `pub(crate)`. `flui-app` re-exports none of
  them, only `frame_failure`'s report types and `RenderingFlutterBinding`,
  at their old `flui_app` paths.
- **The frame sink is the host's, the verdict is the realm's.** A host
  implements `sink::FrameSink`; the realm reads its `SubmitVerdict` and
  classifies retry, device loss and not-shown (ADR-0068). The trait stays
  object-safe: `UiRealm::pump` drives it as `&mut dyn FrameSink` (pinned by
  `sink::tests::a_host_sink_is_driven_through_dyn_frame_sink`).
  `SubmitVerdict` stays exhaustive, never `#[non_exhaustive]`: a new variant
  must make the compiler name the realm's match site and every host's
  mapping, and a wildcard arm would swallow it (pinned by the enum's
  doctest, which matches every variant from outside the crate).
- **A realm owns one text context over the app's font collection.**
  `UiRealm::new` takes the app's `FontCollection` (the host's shared engine
  services hold it), and `RealmServices::construct` builds the realm's one
  `TextContext` over it (ADR-0092 §3). A presentation builds none, no static
  holds one, and the context drops with the realm. Pinned by
  `ui_realm::tests::text_context`.
- **Test hooks stay behind `test-support`.** Items that exist for tests, or
  that have no production caller yet (`HeldPointerQueue::append`/`len`,
  `SemanticsHost::ensure_semantics`, `outstanding_handles`,
  `ExecutionServices::with_limits`/`owns_default_pools`/`default_pools_started` and
  `platform_semantics_enabled`, the announce/event delivery, the realm's
  `for_test` constructors and `*_for_test` probes such as
  `text_context_for_test`, the `testing` doubles),
  compile only under `cfg(test)` or the `test-support` feature, which only
  dev edges enable. Wiring one into production removes its gate in the same
  change.

## Mapping decisions

### Flutter's binding mixins become a runtime crate below the hosts

Flutter composes its frame runtime from `WidgetsBinding`, `RendererBinding`,
`SemanticsBinding` and `SchedulerBinding` mixins on one process-wide
singleton that the embedder drives. FLUI's runtime is per realm and per
presentation (ADR-0027), and its host is not the only thing that drives
frames: a test driver pumps the same realm on a virtual clock. The runtime
therefore lives in its own crate that names no host type, and the hosts depend
on it. Semantics enablement follows: `SemanticsHost` is one per presentation
instead of `SemanticsBinding`'s single instance, so two windows never share an
enablement count or a platform callback. Pinned by
`presentation::tests::semantics_host_is_exclusive_to_this_presentation`.

### The owner host is scheduling state, not a fourth physical owner

ADR-0037 keeps three physical owners: the event-loop/native-window side in
`flui-app`, the realm/presentation state here, and the raster owner in
`flui-engine`. `OwnerHost` is the owner-thread non-reentrant operation
mechanism for the middle owner, not a facade that forwards platform or raster
operations.
`flui-app` consumes `WindowId`, applies renderer surface changes, normalizes
native appearance, removes native mappings before close, and then submits a
FLUI-owned addressed operation. The runtime admits and drains that operation
without naming the window, platform backend or engine.

The acceptance scenario installs realms A and B and runs an A operation whose
user callback synchronously requests B's close. A completes first; B's close
and Detached teardown then run exactly once; B's address is gone; A remains
live; and the host FIFO is empty. Companion tests pin A/B/A FIFO order, a
stale operation queued behind close being dropped, panic-safe realm restore,
and destruction outside mutable `OwnerHost` and `APP_RUNTIME` borrows.

### A submit returns a verdict

Flutter's `FlutterView.render(scene)` returns nothing: whether the engine
presented the frame, dropped it for a lost surface or lost the device is the
engine's business, and the framework never retries. `FrameSink::submit`
returns a `SubmitVerdict` instead, and the realm classifies it: a stale surface
or a lost device arms a retry and keeps the frame's input epochs, a frame that
rendered but could not be shown is retained rather than counted as done, and a
frame with nothing to present falls back to no-present pacing (ADR-0068). The
divergence predates this crate; it is recorded here because the verdict is now
a crate contract. Pinned by `flui-app`'s raster-lane classification tests, for
example `app::raster_lane::tests::a_withheld_frame_is_not_collapsed_into_no_present`.

### `Vsync` controllers tick at the frame's timestamp

Flutter's tickers see the frame's timestamp: `SchedulerBinding.handleBeginFrame`
hands its `timeStamp` to every transient callback, and `Ticker._tick` measures
elapsed time from it. The realm's `Vsync` registry ticks at the timestamp the
pump's `FrameClockSource` returned (`now_secs` reads it for the frame's
duration, relative to the realm's start), so a controller advances by frame
time, not by whenever the tick happened to read the wall clock. A frame
driven outside a pump (a bare `draw_frame`/`render_frame` in a test) falls
back to the wall clock, and a test can still override it with
`set_now_secs_for_test`. Pinned by
`pump_ticks_vsync_controllers_at_the_frame_clocks_time`.

This covers the realm's `Vsync` registry only. A controller built on the
scheduler (`AnimationController::new(d, realm.scheduler())`) is ticked by a
`flui_scheduler::Ticker`, which ignores the timestamp it is handed and
measures elapsed time on the wall clock, so a pump driven on a manual clock
does not advance it. That divergence is recorded and pinned in
`flui-scheduler`'s `ARCHITECTURE.md` ("A ticker's elapsed time is wall-clock
time, not the frame timestamp"); `pump_advances_a_scheduler_ticker_between_two_pumps`
therefore lets real time pass between its pumps.

### `Vsync` ticks in the persistent phase, not among the transient callbacks

Flutter's tickers are transient frame callbacks, so they run in begin frame,
before the microtask flush and before any persistent callback. The realm's
`Vsync` registry is ticked by `draw_frame_entered` at the start of the draw
step, which runs in the scheduler's persistent phase: after the transient
callbacks and microtasks, and after any persistent callback registered before
the pipeline. A controller's listener that schedules a microtask therefore
sees it flushed at the next frame's begin, not this one's. The two sets are
disjoint (a controller registered with a scheduler ticks in begin frame, one
registered with `Vsync` ticks here), so no controller advances twice, and the
tick still precedes every presentation's build, which is the ordering the
segment relies on. Moving the tick into begin frame is a separate change.
Pinned by `pump_ticks_vsync_in_the_persistent_phase_not_among_transient_callbacks`.

### Addressed dispatch retains redraw demand across unwind

**Rule.** An owner-thread operation that may run user mutation follows
`catch dispatch → request the resolved presentation's redraw → resume the
original panic`. Graph-addressed `SignalWrite`, active-presentation keyboard
dispatch and presentation-addressed IME dispatch all use this ordering, as the
pointer path already did. A partial signal commit can therefore become visible
on a later frame even when its callback unwinds; the operation never redirects
demand to the primary or wakes an unrelated sibling.

The `SignalWrite` command callback is `FnMut`, although it is invoked at most
once. Its envelope stays owned outside the caught invocation. Redraw demand is
durable before a successful callback's captures are destroyed; a callback
panic retains the opaque envelope. A stale command likewise rearms an existing
FIFO tail before destroying its envelope. If a tail arrives concurrently while
that destruction is blocked and its ingress wake fails, a caught destructor
panic retries the shared delivery debt before it resumes. This prevents framework
state from being stranded even though Rust cannot recover from two panicking field
destructors inside one aggregate's generated drop glue.

Every command and input-redraw wake has realm-scoped delivery debt shared by
the realm and every `UiCommandSender`. Replaceable identity tokens, rather than
a boolean latch or finite integer generation, prevent an older overlapping
successful wake from erasing a newer failed delivery and cannot saturate on
32-bit targets. Later command ingress or a completed owner-inbox drain retries
the newest unacknowledged token; no retry is promised without a
later host opportunity. A send refused because the bounded inbox is full is
also a host opportunity: it retries existing debt before returning the rejected
command, because otherwise no successful ingress could reach the wake path.

This is continuation safety, not rollback or callback isolation. The panic
still leaves the dispatch boundary, and arbitrary external effects remain the
application's responsibility. Pinned by the panicking secondary-presentation
signal command, command-capture destructor, and addressed keyboard/IME tests.

`execution` has no Flutter counterpart to map: runtime and scheduling
topology, including background execution, is outside Flutter's reference
(ADR-0027), and ADR-0047 records its design.
