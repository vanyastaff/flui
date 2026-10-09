# flui-runtime Architecture

The frame runtime of [ADR-0083](../../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) §1: the
UI runtime (`ui_runtime::UiRuntime`) and the per-presentation frame machinery it
drives, placed below the hosts (`flui-app`'s runners, platform wiring, raster
lane and UI runtime dispatch layer) and above the widget spine. It arrived in
steps; the ADR's `## Migration` section lists them, and what is still to move
(the UI runtime-neutral owner host and typed operation application). Native-window
demultiplexing and the platform/raster tails of an owner turn remain in the
host.

## Invariants

- **Inherited DPR agrees with the render pipeline.** Initial media data uses
  the pipeline's accepted ratio. Direct updates reject non-positive and
  non-finite ratios before mutation. Native metrics reject these observations
  before coalescing, preserving an earlier accepted resize and independent
  appearance observations. `initial_inherited_scale_matches_the_renderer`,
  `resize_and_surface_restore_reach_the_product_frame` and
  `queued_state_bursts_coalesce_between_observing_frames` pin these paths.

- **Preference projection is updated with the accepted snapshot.** Root media
  publication derives text scale, contrast and ordered preferred locales together.
  Unknown contrast uses
  normal contrast without altering the raw host observation. The mounted row
  `resize_and_surface_restore_reach_the_product_frame` checks contrast changes,
  unknown fallback and the seed seen by a later runtime's first build.
  `preferred_locales_select_resources_and_direction` checks actual localized
  resources and direction after publication and on a late runtime's first build
  (ADR-0173).

- **Rebuild delivery is assembled before mount.** Every presentation connects
  its build owner and widgets binding to the scheduler and its own weak window
  before creating elements. Rebuild handles capture that hook when mounted;
  installing it later in a runner leaves existing handles without a wake.
  `preference_fanout_survives_a_failing_runtime` observes the wake before driving
  a frame, and covers competing wake/completion failures, sibling delivery and
  the next preference update. `queued_state_bursts_coalesce_between_observing_frames`
  pins preference snapshots on opposite sides of a queued frame boundary.

- **Assembly is separate from publication.** `UiRuntime::presentation_factory`
  captures the capabilities for assembling another presentation without keeping
  the runtime borrowed. Its scheduler reference is weak; assembly temporarily
  upgrades it and may invoke platform callbacks. A successful assembly grants no
  publication authority: the host checks its exact authorizer again before
  installing the result. The app's
  `presentation_assembly_reentry_revalidates_its_authorizer` pins that reentry
  cannot publish through a presentation that closed during assembly.
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
- **A UI runtime is owner-affine.** `UiRuntime` is `!Send + !Sync` (a raw-pointer
  `PhantomData` marker; pinned by `assert_not_impl_any!` in the UI runtime tests).
  Everything that crosses a thread goes through a `UiCommandSender` into a
  bounded inbox that the owner drains only while the scheduler is idle.
- **The owner host is an ordinary owner-affine value.** The planned
  `OwnerHost` is `!Send + !Sync` and owns only the UI runtime registry, one
  host-wide FIFO of typed operations carrying `PresentationAddress`, UI runtime
  checkout/restore state and deferred UI runtime-map mutations. It owns no TLS,
  native-window registry, platform capability, native resize driver, engine/raster
  object, application service or execution-pool lifetime. `flui-app`'s sole
  `APP_RUNTIME` trampoline contains it beside those host-only owners.
- **Owner work is non-reentrant across UI runtimes.** A reentrant operation for any
  UI runtime appends to the same FIFO; only the outermost owner turn executes it.
  Before ADR-0083 move 5a, `flui-app` rejected UI runtime B while UI runtime A was
  checked out, which could lose B's UI close after its native window had
  already closed. The app now provides the host-wide FIFO behavior that the
  extraction must preserve. `flui-app` now executes at most 32 logical
  operations per continuation callback, sharing the budget between fresh
  native roots and carried FIFO entries, counts stale entries against it, and
  requests one sequence-stamped continuation opportunity when work remains.
  There is no additional per-runtime queue: each checkout executes exactly one
  entry selected by the host-wide FIFO. The app test
  `carried_work_shares_one_callback_budget_across_runtimes` verifies delivery
  across callbacks, shared fresh/carried budget, nested callback scope and no
  continuation after the queue settles.
  A fresh native root stays synchronous while budget remains, while a close fences its exact
  `PresentationAddress` from later work at admission time so root priority
  cannot let input jump a deferred terminal operation. The remaining
  pre-extraction work is the closed operation vocabulary and its
  operation-specific admission rules.
  Checkout restoration and deferred mutations are unwind-safe, and
  UI runtime-owning values are dropped after every mutable host borrow is released.
- **Owner operations form a closed vocabulary.** Input, lifecycle, normalized
  metrics, close, install, uninstall, pump and background-pump are typed
  operations or explicit methods. The host accepts no arbitrary
  `Box<dyn FnOnce(&UiRuntime)>`, `Box<dyn FnOnce(&mut UiRuntime)>`, `dyn Any` or
  executor job. A bounded typed ingress plus a wake capability is the only
  cross-thread path; an owner-local dispatcher is `!Send` and appends to the
  same FIFO. Operation-specific admission distinguishes lossless transitions,
  latest-value state and edge-coalesced wakes.
- **Pending window state batches stop at observable operations.** Adjacent metrics,
  safe-area and brightness updates for one exact presentation retain the latest
  values together. Input, frames, lifecycle, close and another presentation's work
  terminate the batch. Size and DPI are one value, and native resize precedes the
  media-query update. No executing operation is replaced or delayed to collect a
  batch. `queued_state_bursts_coalesce_between_observing_frames` and
  `pending_metrics_do_not_cross_ordered_operations` pin this owner-admission policy;
  pointer-motion sampling remains the interaction layer's responsibility.
  A failed native resize leaves its size/DPI pair unpublished but does not discard
  the batch's accepted safe-area and appearance values. State publication and
  redraw settlement preserve the first failure across competing wake and owner
  completion failures. `resize_failure_preserves_other_batched_window_state`
  reads those values through a mounted `MediaQuery` consumer, then renders a
  subsequent successful resize.
- **Inherited window data is runtime-owned.** Hosts do not obtain the mutable
  `MediaQuerySource`. A direct `set_device_pixel_ratio_for` updates both render
  scale and inherited data; the native resize batch uses an internal render-only
  step before publishing the complete accepted size/DPI/appearance batch.
  `resize_and_surface_restore_reach_the_product_frame` reads the new ratio from
  a mounted consumer after either entry. The headless host uses that same direct
  scale entry without separately repairing the inherited state.
- **Every entry composes every presentation.** `UiRuntime::enter` activates a
  `GlobalKey` registry composite over all the UI runtime's presentations for the
  whole dynamic extent of the call, closing included. A binding whose own
  lock is held reports itself busy and the composite skips it (`flui-view`'s
  `key::registry`), so no entry needs to exclude a presentation.
  `state_read_across_presentations_during_a_segment_resolves` pins a read of a
  live sibling's key; a closing presentation's keys are **Unasserted:** no test
  pins this.
- **A failed presentation segment is contained to that presentation.**
  `draw_frame_entered` runs each presentation's segment under its own
  `catch_unwind` (ADR-0048); a panic or structured pipeline error is
  reported through `frame_failure` and re-dirties only that presentation,
  and siblings still frame (pinned by
  `an_escaped_segment_panic_is_contained_to_its_own_presentation_and_the_sibling_still_frames`).
- **A UI runtime's frame runs only through `UiRuntime::pump`.** The pump takes
  `&mut self`, so no second frame on the same UI runtime can start while one runs,
  and it enters the UI runtime itself for the whole transaction, in this order:
  apply commands (the owner inbox, at the Idle boundary) → begin frame →
  draw frame (persistent callbacks, then the pipeline and the submit through
  the sink) → end frame (both post-frame queues, the UI runtime's owner-local lane
  included). Its clock is read once: that timestamp is the scheduler's
  frame time and the time `Vsync` controllers tick at, though a scheduler
  `Ticker` still reads the wall clock (see "`Vsync` controllers tick at the
  frame's timestamp" below).
  A wake with frames disabled runs `UiRuntime::pump_background` instead: clear
  the frame latch, then poll the async driver, no frame. Whether a wake
  becomes a pump is the host's per-backend wake gate (ADR-0058), not the
  UI runtime's. Pinned by `ui_runtime/tests/pump_transaction.rs`, each test failing
  against a pump that skips or reorders the phase it names; `flui-app`'s
  `runner_frame_ordering` scan pins that every runner goes through it.
  Outside this crate the invariant holds by type: the draw step
  (`render_frame`) is crate-private and the owner-local post-frame lane
  (`local_post_frame_lane`) is `test-support` only, so a host has no way to
  draw a frame, or end one, except the pump. Tests of the draw step alone
  reach it through `render_frame_for_test` and `draw_frame`, both under
  `test-support`. `trybuild_ui::ui_tests` checks the private draw-step diagnostic
  from a host caller and compiles a valid pump caller. It also rejects moving
  a clone from `RenderingBinding::root_pipeline_owner` to another thread while
  accepting local access to that same binding's pipeline.
- **The UI runtime renders through a sink, never an engine.** `UiRuntime::pump`
  takes any `&mut dyn FrameSink`; the host picks one (`flui-app`'s raster
  lane, or its direct sink over a borrowed backend on the web runner), and
  the UI runtime tests pick `testing::ScriptedSink`. How a host maps its
  backend's outcomes to verdicts is that host's to test.
- **Internal, and only the hosts depend on it.** Tier K,
  `tier-kind = "internal"`: nothing here is an embedder API (ADR-0027 §9)
  except the `execution` host-injection seam below.
  `allowed-dependents = ["flui-app", "flui-testing"]` makes the application
  host and the headless test driver the only crates allowed a normal edge,
  checked by `cargo xtask workspace`. That rule is what keeps ADR-0047's
  invariant true now that `ExecutionServices` is `pub`: only a host crate, one
  of the runtime's `allowed-dependents`, constructs the services, and no other
  workspace crate reaches the pools. `flui-testing` is the host of its own
  headless loop (ADR-0083 §4) and constructs no execution services. Dev edges
  are not restricted.
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
- **The test driver shares the transaction, not the production host.**
  `flui-testing` sits above this crate: its `HeadlessHost` drives
  `UiRuntime::pump` directly with its manual clock and headless sink, and its
  widget harness runs every frame through it. It neither constructs
  `OwnerHost` nor reproduces native event-loop routing; owner-turn behavior
  is tested in this crate's own host tests.
- **A UI runtime reads time from one `ClockSource`.** `UiRuntime::new` takes the
  source: the UI runtime's frame-time origin, every presentation's gesture arena
  (its deadlines) and `FrameClock` (its produce gate) read it, so a
  `ClockSource::Manual` UI runtime keeps them on the timeline its driver
  advances, and a host passes `ClockSource::Platform`. The pump's
  `FrameClockSource` is the frame's timestamp; a driver hands it the same
  `ManualClock` (`a_ui_runtime_on_a_manual_clock_fires_gesture_deadlines_on_that_clock`,
  `a_manual_clock_ui_runtime_gates_its_min_produce_interval_on_that_clock`).
- **Per UI runtime, per presentation or per host loop, never per process.** Every
  type here is owned by one UI runtime (`UiRuntime`, its scheduler and command
  inbox), one presentation (`PresentationState`, `HeldPointerQueue`,
  `SemanticsHost`, `PerformanceStats`, the commit epoch) or, for
  `ExecutionServices`, one host loop, constructed only by the host's
  composition root. The one static is `runtime_services`' incarnation counter,
  an identity counter with an explicit ADR-0097 counter grant. Its `try_update`
  helper preserves permanent exhaustion instead of wrapping into a previous UI runtime.
- **The UI runtime's surface is the host's, not an embedder's.** `UiRuntime`,
  `PresentationState` and their methods are `pub` only where `flui-app`
  calls them; what only `flui-app`'s tests call is `pub` under
  `test-support`; the rest is `pub(crate)`. `flui-app` re-exports none of
  them, only `frame_failure`'s report types and `RenderingBinding`,
  at their old `flui_app` paths. `flui-testing` hosts a UI runtime in its
  `HeadlessHost` and keeps it crate-private, so `flui::testing` does not
  reach it either.
- **The frame sink is the host's, the verdict is the UI runtime's.** A host
  implements `sink::FrameSink`; the UI runtime reads its `SubmitVerdict` and
  classifies retry, device loss and not-shown (ADR-0068). The trait stays
  object-safe: `UiRuntime::pump` drives it as `&mut dyn FrameSink`, so the
  compiler holds object safety at that signature.
  `SubmitVerdict` stays exhaustive, never `#[non_exhaustive]`: a new variant
  must make the compiler name the UI runtime's match site and every host's
  mapping, and a wildcard arm would swallow it (pinned by the enum's
  doctest, which matches every variant from outside the crate).
- **A UI runtime owns one text context over the app's font collection.**
  `UiRuntime::new` takes the app's `FontCollection` (the host's shared engine
  services hold it), and `RuntimeServices::construct` builds the UI runtime's one
  `TextContext` over it (ADR-0092 §3), behind a `TextContextHandle`. A
  presentation builds none: `PresentationState::new`, the one place a
  presentation's pipeline gets its capabilities, builds that pipeline from the
  UI runtime's handle (`RuntimeCapabilities::text`, a required field; `PipelineOwner`
  has no constructor without one), so every presentation's
  layout, intrinsic and dry queries measure through the UI runtime's one context
  (ADR-0092 §10 step 3). No static holds one, and the context drops with the
  UI runtime and its presentations. Pinned by `ui_runtime::tests::text_context`,
  among them `two_ui_runtimes_measure_text_through_their_own_contexts` and
  `every_presentation_pipeline_holds_the_ui_runtimes_text_context`.
- **Test hooks stay behind `test-support`.** Items that exist for tests, or
  that have no production caller yet (`HeldPointerQueue::append`/`len`,
  `SemanticsHost::ensure_semantics`, `outstanding_handles`,
  `ExecutionServices::with_limits`/`owns_default_pools`/`default_pools_started` and
  `platform_semantics_enabled`, the announce/event delivery, the UI runtime's
  `for_test` constructors and `*_for_test` probes such as
  `text_context_for_test`, the `testing` doubles),
  compile only under `cfg(test)` or the `test-support` feature, which only
  dev edges and the headless test driver `flui-testing` enable (it is a
  test-only crate, reached by applications through dev edges or the facade's
  `testing` feature). Wiring one into production removes its gate in the same
  change. `UiRuntime::active_text_store` is one of them until a pull-model
  platform input method reads it. The exception is the `FrameClockSource`
  impl for `ManualClock`, which a trait impl cannot gate per caller.

## Mapping decisions

### Observing input follows its presentation's accepted motion

Keyboard and IME drain a frozen measured motion prefix before dispatch without
advancing frame time or ending contacts (ADR-0163). Keyboard retains the active
presentation resolved at admission through reentrant focus changes; IME retains
its addressed presentation. Callback failures finish accepted motion, deferred
arena settlement and the observing input before the first failure resumes.
Reentrant movement stays debt for the next operation or frame. This observes
input-produced state; layout and paint still follow their frame transaction.

The public flui-testing `containment_and_isolation_matrix` rows
`mouse_motion_precedes_keyboard_without_a_frame`,
`touch_motion_precedes_keyboard_without_a_frame` and their resampled variants
pin measured coordinates before Key. The single and competing rows
`motion_failure_keeps_following_keyboard_and_contact_terminal`,
`keyboard_failure_keeps_preceding_motion_and_contact_terminal` and
`motion_failure_precedes_competing_keyboard_failure_and_recovers` pin recovery.
`ime_commit_observes_preceding_measured_motion` and
`ime_commit_survives_competing_motion_and_owner_failures` pin actual text edits.
`keyboard_reads_all_frozen_contacts_after_sibling_failure`,
`keyboard_barrier_keeps_reentrant_contact_motion_for_the_next_round`,
`keyboard_barrier_keeps_frozen_coalesced_motion_before_reentrant_replacement`,
`keyboard_coalesced_prefix_survives_reentrant_capture_release`,
`runtime_keyboard_barrier_preserves_scale_contacts_and_continuity` and
`keyboard_motion_barrier_uses_resolved_focus_owner_during_reentrant_focus_change`
pin sibling prefixes, newer debt, gesture continuity and resolved ownership.

The default-policy frozen Contact payload retains its exact live contact
authority when a callback replaces its queued marker. Old cleanup leaves newer
debt intact; terminal/replacement invalidation still refuses stale publication.
Capture guards hold the committed prefix through reentrant release, then loss
settlement drains its accepted tail and one Cancel before observing input.

Independent removal of Keyboard or IME barrier wiring makes the corresponding
public rows observe stale motion state and the wrong competing first failure.
Per-contact dispatch-time measured draining admits newer reentrant movement
into the old Key round. Removing coalesced prefix authority loses the committed
old Move; removing only its capture guard loses that Move during release.
These source inverses distinguish production behavior from eventual frame
delivery and from a test-only flush seam (ADR-0163).

### Frame input and ambient hover belong to each presentation

The frame drains deferred arena decisions and queued pointer motion in
presentation insertion order. Each phase has its own containment boundary:
a failing callback does not suppress the remaining motion or a sibling's
accepted work, and the first failure resumes after the input pass completes.
Ambient hover refresh uses each presentation's own hit-test tree only when
that tree's terminal revision has been acknowledged. A failed or uncommitted
sibling supplies neither another window's tree nor a reason to skip a
committed window's refresh.

The public `flui-testing` rows
`a_secondary_contact_move_is_delivered_by_the_next_frame`,
`a_secondary_deferred_arena_verdict_is_delivered_by_the_next_frame` and
`a_secondary_layout_refreshes_its_stationary_hover` pin the frame producers.
`a_panicking_primary_motion_does_not_erase_the_secondary_motion` and
`competing_frame_motion_failures_preserve_the_first_and_recover` pin accepted
sibling delivery, exact first-failure authority and the next healthy frame.

### Suspension drains queued motion while focus loss preserves hover

Pointer cancellation first delivers terminal Cancel to active widget routes.
Hidden, paused and detached execution additionally invokes the binding's
lifecycle drain, including when a Cancel callback fails; ordinary window
focus loss keeps its queued hover. A completed contact held for the first
tree acknowledgement remains accepted input across a reversible pause.

`host_pause_discards_a_queued_hover_before_resume`,
`window_blur_keeps_a_queued_hover` and
`host_pause_keeps_a_completed_held_tap_for_the_first_commit` exercise these
contracts through addressed platform input and the real realm pump in
`flui-testing`'s `containment_and_isolation_matrix`.

The binding commits this drain before emitting its optional diagnostic.
`a_panicking_pause_diagnostic_cannot_skip_motion_drain` and
`a_cancel_failure_precedes_a_pause_diagnostic_failure_and_recovers` verify
the actual diagnostic is delivered, stale motion is gone despite its panic,
the first terminal failure remains authoritative and new input recovers.

### The frame runtime is a crate below the hosts

The frame runtime is per UI runtime and per presentation (ADR-0027, no process-wide
singleton), and its host is not the only thing that drives
frames: a test driver pumps the same UI runtime on a virtual clock. The runtime
therefore lives in its own crate that names no host type, and the hosts depend
on it. Semantics enablement follows: `SemanticsHost` is one per presentation,
so two windows never share an enablement count or a platform callback.
**Unasserted:** no test pins this.

### The owner host is scheduling state, not a fourth physical owner

ADR-0037 keeps three physical owners: the event-loop/native-window side in
`flui-app`, the UI runtime/presentation state here, and the raster owner in
`flui-engine`. `OwnerHost` is the owner-thread non-reentrant operation
mechanism for the middle owner, not a facade that forwards platform or raster
operations.
`flui-app` consumes `WindowId`, applies renderer surface changes, normalizes
native appearance, removes native mappings before close, and then submits a
FLUI-owned addressed operation. The runtime admits and drains that operation
without naming the window, platform backend or engine.

The acceptance scenario installs UI runtimes A and B and runs an A operation whose
user callback synchronously requests B's close. A completes first; B's close
and Detached teardown then run exactly once; B's address is gone; A remains
live; and the host FIFO is empty. Companion tests pin A/B/A FIFO order, a
stale operation queued behind close being dropped, panic-safe UI runtime restore,
and destruction outside mutable `OwnerHost` and `APP_RUNTIME` borrows.

### A submit returns a verdict

Whether the engine presented a frame, dropped it for a lost surface or lost
the device is something the UI runtime must know in order to retry.
`FrameSink::submit` returns a `SubmitVerdict`, and the UI runtime classifies it: a stale surface
or a lost device arms a retry and keeps the frame's input epochs. `Retry` does the
same for transient rendering failure without a surface restamp or device rebuild:
it marks full repaint as well as waking, so a static scene progresses without new
input (ADR-0101). A frame that
rendered but could not be shown is retained rather than counted as done, and a
frame with nothing to present falls back to no-present pacing (ADR-0068). The
verdict is a crate contract. Pinned by `flui-app`'s raster-lane classification tests, for
example `app::raster_lane::tests::a_device_loss_classifies_device_lost_and_recovery_reminting_unblocks`,
and on the UI runtime side by `surface_lost_keeps_needs_redraw_armed_for_a_retry` and
`the_withheld_retry_is_bounded_and_then_parks`.

### `Vsync` controllers tick at the frame's timestamp

The UI runtime's `Vsync` registry ticks at the timestamp the
pump's `FrameClockSource` returned (`raw_frame_time` reads it for the
frame's duration, relative to the UI runtime's start), so a controller advances by
frame time, not by whenever the tick happened to read the wall clock. A frame
driven outside a pump (a bare `draw_frame`/`render_frame` in a test) falls
back to the wall clock, and a test can still override it with
`set_now_secs_for_test`. **Unasserted:** no test pins the pump timestamp.

Each presentation maps that one raw time through its own
`flui_animation::MotionClock` before ticking its registry, so the time a
registry sees is finite and never runs backwards: an override that is not a
duration, or a raw time earlier than the last, holds the animation
(`an_invalid_or_backwards_frame_time_holds_the_animation`). The clock runs at
its default rate while `AnimationController` still applies the scheduler's
process-wide time dilation, so slow motion has one source.

This covers the UI runtime's `Vsync` registry only. A controller built on the
scheduler (`AnimationController::new(d, UI runtime.scheduler())`) is ticked by a
`flui_scheduler::Ticker`, which ignores the timestamp it is handed and
measures elapsed time on the wall clock, so a pump driven on a manual clock
does not advance it. That behaviour is recorded and pinned in
`flui-scheduler`'s `ARCHITECTURE.md` ("A ticker's elapsed time is wall-clock
time, not the frame timestamp").

### `Vsync` ticks in the persistent phase, not among the transient callbacks

Tickers registered with a scheduler are transient frame callbacks, so they run
in begin frame, before the microtask flush and before any persistent callback.
The UI runtime's `Vsync` registry is ticked by `draw_frame_entered` at the start of the draw
step, which runs in the scheduler's persistent phase: after the transient
callbacks and microtasks, and after any persistent callback registered before
the pipeline. A controller's listener that schedules a microtask therefore
sees it flushed at the next frame's begin, not this one's. The two sets are
disjoint (a controller registered with a scheduler ticks in begin frame, one
registered with `Vsync` ticks here), so no controller advances twice, and the
tick still precedes every presentation's build, which is the ordering the
segment relies on. Moving the tick into begin frame is a separate change.
**Unasserted:** no test pins this.

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

Every command and input-redraw wake has UI runtime-scoped delivery debt shared by
the UI runtime and every `UiCommandSender`. Replaceable identity tokens, rather than
a boolean latch or finite integer generation, prevent an older overlapping
successful wake from erasing a newer failed delivery and cannot saturate on
32-bit targets. Later command ingress or a completed owner-inbox drain retries
the newest unacknowledged token; no retry is promised without a
later host opportunity. A send refused because the bounded inbox is full is
also a host opportunity: it retries existing debt before returning the rejected
command, because otherwise no successful ingress could reach the wake path.

A queued `HotReload` follows the same failure ordering. If reassembly or its
frame-request callback panics, partial tree changes retain redraw demand and a
fresh owner wake is attempted before the first panic resumes. A failed retry
keeps shared delivery debt for the next ingress or owner boundary. The accepted
FIFO tail is neither discarded nor executed inside the failed turn. This does
not roll back reassembly or promise a host turn after every delivery fails.
`failed_reload_wake_rearms_the_accepted_tail` and
`competing_reload_wakes_preserve_the_first_failure_and_retry`, and
`reassemble_failure_keeps_priority_over_a_failed_rearm` exercise the real
widget reassembly callback with a queued agent read.

This is continuation safety, not rollback or callback isolation. The panic
still leaves the dispatch boundary, and arbitrary external effects remain the
application's responsibility. Pinned by the panicking secondary-presentation
signal command, command-capture destructor, and addressed keyboard/IME tests.

Runtime and scheduling topology, including background execution
(`execution`), is designed for Rust (ADR-0027); ADR-0047 records its design.

### Agents read the committed tree through the owner inbox

**Rule.** `UiRuntime::semantics_agent` vends a `SemanticsAgent` (`Clone + Send + Sync`) for one
presentation. Its `read` and `act` enqueue `UiCommand::SemanticsRead` and
`UiCommand::SemanticsAgentAction` on the UI runtime's bounded inbox and return an `AgentReply` the
owner fills at its next drain, a frame boundary. A read projects the pipeline's semantics owner
as it stands after the last committed frame (`flui_semantics::SemanticsOwner::read_wire`); an
action is resolved against that same tree and dispatched through
`PresentationState::dispatch_semantics_action`, the path an assistive technology's action
takes. An action's `Ok` means it was delivered to the element's semantics handler, not that its
effect happened: a `GestureDetector` runs a semantics tap in the frame after the drain, so the
effect reads two frames on. No lock guards per-node state: the owner reads its own tree on its
own thread, and the agent's side holds only its channel ends and the record of handles its reads
reported, which tells `gone` from `unknown_handle`. The record keeps the newest generation
reported per render slot, so it is bounded by the slots the presentation has used, not by how
many elements came and went. Element handles are render identities, scoped to the one
presentation; a server that spans windows keeps its own table over them (ADR-0095 §3).

**Enablement.** Every clone of an agent shares one `SemanticsHandle`, so semantics are
collected while any clone lives, whatever assistive technology does; the tree an agent reads is
the one published to assistive technology. Vending requests a frame, and until the first
semantics frame commits a read answers `NoTreeYet` (`busy`, retry `soon`). Dropping the last
clone lets the next frame's reconcile stop collection; an unanswered `AgentReply` holds only the
record of handles, not the semantics handle.

**Failure.** A panic while the owner serves an agent's action answers that action first, then
re-arms the owner's wake if the inbox still holds a tail, then resumes the original panic, which
stays the one that escapes. The answer is `HandlerPanicked` (`may_have_run`) when the handler had
been invoked, and `ResolvePanicked` when the owner panicked before reaching it. A handler that
defers its work to a later frame, as a `GestureDetector` does, has already answered `Ok`; a panic
there belongs to that frame. A reply whose receiver is gone
is traced by element id and error code only, never a label or a value, and the drain goes on.
Pinned by `src/ui_runtime/tests/agent_semantics.rs`.

**Wiring.** Production reaches the agent through the development-agent hook
(`flui_view::dev_agent::DevAgentHook`, ADR-0095 §3). `UiRuntime::dev_agent_window` vends one
agent per presentation, keeps it on the `PresentationState`, and hands out
`flui_view::dev_agent::AgentWindow`s that hold it weakly through the hidden
`flui_view::__runtime::AgentPort`, so the hook never keeps a closed window alive: closing the
presentation drops the agent and every call on a window answers `gone` (kind `window`), at once
and before anything is enqueued, even while another thread's call still holds the port: the port
carries an open flag the presentation clears as it closes, and a call enqueues under the flag's
read lock while the close takes its write lock, so nothing is admitted for a closed window. The
wake runs after the read guard retires, including a full-inbox debt retry. A wake
may therefore re-enter close without deadlocking the admission fence; the
already admitted command keeps its place in the inbox. The bounded child rows
in `agent_port_admission_matrix` exercise both public reads and actions, then
verify that later calls answer `gone`. The windows hold the presentation's semantics handle strongly instead of the presentation, so the
cost lasts exactly as long as the hook keeps a window: a hook that does not serve is handed none,
and one that detaches or panics drops its windows, and collection stops on the next frame. `flui-app`'s desktop and iOS runners
and `flui_testing::HeadlessDevAgent` drive the hook through `dev_agent::DevAgentHost`; the
endpoint that serves it is `flui-devtools`' `agent` feature. Pinned by
`an_agent_for_a_closed_presentation_answers_gone`, `dev_agent_host_contains_its_hook` and
`flui-devtools`' `the_endpoint_contains_every_failure`.

### The development agent host lives in the runtime

**Rule.** `dev_agent::DevAgentHost` is the only code that calls an installed `DevAgentHook`:
attach once per loop (a second attach while attached is refused, and a hook whose `attach`
answers that it does not serve stays unattached and is never detached), hand over each window with
content, detach when the loop's `DevAgentAttachment` drops. The attachment is `!Send + !Sync`,
so that detach runs on the owner thread like every other call. Each call lends the hook out of its
slot with no lock held, so a hook that re-enters the host finds the slot empty, and a detach that
arrives meanwhile runs when the call returns; a panic drops the hook (its `Drop` contained too,
its payload forgotten, a deferred detach's included) and every later call does nothing; a hook
still held when the last host clone goes is dropped under the same containment; nothing
is vended while the hook is not attached, so a hook that does not serve or failed to attach costs
no semantics work, and a window's semantics work ends once the hook drops its `AgentWindow`.

**Why here.** Two hosts drive it, `flui-app`'s windowed runners and `flui-testing`'s headless
UI runtime, and the headless one is the only one CI executes (a windowed install creates a GPU
renderer first). Writing the containment once below both keeps the tested path and the shipped
path the same code. The devtools server cannot name the runtime (an official package depends on
`flui-sdk` and the contract crates only), so the hook trait is `flui-view`'s and reaches it
through the SDK; the runtime holds no transport. Flutter has no counterpart: its service
extensions are the VM's. Pinned by `dev_agent_host_contains_its_hook` (`src/dev_agent/tests.rs`).

### A font change reaches a UI runtime through its owner turn, on its next frame

**Rule.** `UiRuntime::fonts_changed` requests a redraw for every presentation the UI runtime hosts, and
nothing else: the marking is each pipeline's, at its next drain
(`PipelineOwner::apply_font_change`, flui-rendering), which compares the collection's
generation with the last one it applied. The host sends the notice: `flui-app`'s
`register_font` registers on the app's `FontCollection`, then dispatches `fonts_changed` to every
installed UI runtime as a frame task, so a UI runtime checked out for the task that registered gets it
queued behind that task. Bytes registered before are refused at that door
(`FontRegistrationError::AlreadyRegistered`) and notify nothing.

**Why.** The collection is shared by every UI runtime, but a UI runtime's state is touched only on its
owner turn. A notice that only wakes is idempotent: a second notice, or a pipeline that already
applied the change, lays nothing out. Pinned by `font_registration_matrix`
(`src/ui_runtime/tests/font_registration.rs`).

### Incarnation exhaustion cannot reissue a stale address

The incarnation counter issues every nonzero `u32` generation once, then keeps
zero as a permanent exhaustion sentinel. Catching an exhaustion panic cannot
restart identity allocation. `exhausted_incarnations_never_alias_previous_ui_runtimes`
in `ui_runtime_and_presentation_isolation_matrix` exercises the production allocator
with a local counter at its terminal boundary; exhausting the actual global
source is impractical and would invalidate unrelated UI runtime tests.

### A zero-capacity performance window is disabled

`PerformanceStats::new(0)` retains no duration samples, so both average frame
time and FPS stay zero. A nonzero window still records platform-clock intervals.
The consumer row `a_zero_capacity_performance_window_retains_no_frame_samples`
in flui-testing's `headless_frame_driver_matrix` distinguishes the two through
the public timing methods. Production overlays retain their default 120 samples.


### Deterministic execution preserves concurrent admission during compaction

Completed deterministic tasks retire under the task-list mutex; pending futures
stay in the same list, including tasks admitted by another producer before the
lock is acquired. Removed slots contain no future, so their retirement invokes
no user destructor under that mutex. Taking a snapshot and replacing the list
would discard concurrent admission. `compaction_preserves_concurrently_admitted_future_and_next_work`
in `execution_lane_matrix` uses a private before-lock seam to admit a real public
spawn exactly at that boundary, then proves both that task and the next task run.

### Refused first-frame counter operations preserve existing deferrals

An unmatched release or exhausted deferral count refuses its atomic update
before mutation. Catching that refusal therefore leaves later valid operations
usable. The public `unmatched_first_frame_release_preserves_the_next_deferral`
row in `flui-testing`'s `headless_frame_driver_matrix` proves real painted output
is withheld and then delivered after recovery. `draw_frame_returns_layer_tree_and_defers_when_gated`
also contains the terminal-count row; only its initial count is injected
privately because the public boundary requires billions of calls to reach.

### Frame failure delivery isolates diagnostics and opaque ownership

A segment failure is classified by borrowing its payload, then the opaque payload
is retained before recovery or diagnostics can run. The typed report owns only
framework values and strings. Diagnostics and the registered handler run under
separate boundaries: subscriber failure cannot suppress callback delivery, and
neither can replace the presentation's original failure or stop sibling frames.
A failed callback's owning envelope is retained before secondary payload handling;
ordinary successful-envelope retirement still runs under its own boundary. Rust
cannot contain two panicking fields in an aggregate's first ordinary destruction.

`frame_failure_containment_matrix` isolates aggregate payload and callback-capture
cases in subprocesses: producer alone, handler alone, diagnostics alone, competing
failures, and failed-handler retirement during UI runtime teardown. Each case asserts
one segment report, no partial submission and the next automatic retry presenting.
The existing private segment probe injects a failure that consumers cannot place
at this exact outer boundary; the registered report handler and real frame driver
are the production paths.

## Exceptional terminal presentation ownership

A presentation close that holds a failure, or runs inside an unwind, is in
preserving mode ([ADR-0123](../../docs/adr/ADR-0123-exceptional-presentation-close.md)).
The close first withdraws the presentation's authority: its lane targets and
cached routes, signal graph, writers, rebuild handles, local and UI runtime
GlobalKeys and agent ports refuse further use, while siblings stay live. It then
retains the values the presentation still owns (its trees and their callbacks)
instead of dropping them, and skips optional disposal
([ADR-0127](../../docs/adr/ADR-0127-exceptional-path-retention.md)). The platform
window and the accessibility bridge are framework-owned and are released in
either mode. A healthy close drops everything normally, and a lifecycle drain
still runs every eligible callback before propagating its first failure. Tested
by `presentation_close_retirement_failures_preserve_focus_ime_and_siblings`.

Accessibility input uses the whole-request translator of ADR-0124 before
presentation inbox admission, preserving numeric values and explicit
expand/collapse requests. Payload admission remains with the current semantics
owner at delivery.

`RenderingBinding` keeps its semantics-enabled listeners in their own storage,
and `set_semantics_enabled` calls an owned snapshot of them after releasing the
lock, so a callback may add or remove listeners or toggle the state again. Both
the storage (when the binding drops) and each snapshot retire their envelopes
in registration order. A panic from a callback or a capture destructor
propagates with the committed semantics state intact; while it unwinds, the
retiring container retains every envelope clone it holds, since a thread-shared
clone cannot be proven non-last (ADR-0127). Competing destructors inside one callback's capture, and the
binding's other fields, are outside this contract.
