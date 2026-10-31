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

- **A pump owns its selected timestamp.** The host clock is read once. Every
  controller in a presentation's parent and child registries samples that
  timestamp through the presentation motion clock, even if the host clock
  advances during the pump or an earlier animation callback.
  `one_pump_shares_its_selected_time_across_parent_and_child` uses the public
  runtime pump and distinguishes its selected time from later host readings.

- **Motion follows one host observation.** Presentation clocks resolve the
  runtime's application override against accepted `SystemPreferences` before
  publication callbacks. Both current and later presentations inherit it.
  Normal runs use the duration-scale timeline and settle under Reduce; Preserve
  keeps authored timing. Changed motion wakes parked work, while duplicate host
  observations do not republish inherited data (ADR-0195).
  `system_motion_change_reaches_media_query_and_the_clock` exercises actual
  frame sampling, inherited publication and late presentation seeds.

- **Sampling failure preserves independent progress.** Each presentation owns
  its sampling provenance, first failure and independent publication blocker
  ([ADR-0199](../../docs/adr/ADR-0199-presentation-sampling-recovery.md)). All
  presentations sample before any builds. Fresh admission polls the frame clock
  once before diagnostics. Delivery-only failure preserves committed samples
  and can continue; computation or unknown failure rejects an admitted fresh
  or retained attempt. Independent controllers, input deadlines and sibling
  presentations still progress. The public pump tables
  `presentation_delivery_failure_keeps_healthy_sampling_build_and_completion`
  and `presentation_computation_failure_rejects_its_frame_and_preserves_healthy_work`
  distinguish displayed values, build, completion, addressed reports and the
  next healthy operation.

- **Scheduling reads invoke no foreign code.** Host wake and pending
  getters read committed deadline snapshots. Guard-free owner execution refreshes
  changed observations; scheduling reports wait until runtime checkout returns.
  Explicit deadline update authority commits each revision before waking.
  A failed pure query receives one automatic probe after 100 ms, then parks;
  an unknown poll cannot automatically replay the same revision. Healthy earlier
  deadlines remain eligible. `owner_sampling_contract` exercises the restored
  owner boundary, inert reads, competing diagnostics and live publisher updates;
  interaction's `owner_deadline_recovery_matrix` pins quarantine and revision
  identity. The interval is FLUI recovery policy, not an OS execution guarantee.
  Deadline-query reentry shares the enclosing owner opportunity; nested inbox
  work does not recursively rediscover its active source or restart a poll
  budget. The table's
  `deadline_query_reentry_does_not_rediscover_its_active_source` row distinguishes
  one admitted observation from a second nested query and checks inert wake
  reads afterwards.
  Armed query retries contribute to both the cached wake deadline and pending
  work: a native frame gate must admit their owner pump. The same mapping of
  observation state supplies both facts; parked/retired sources contribute
  neither. `query_retry_participates_in_the_pending_gate_and_then_settles`
  drives the public owner frame/drain/pending-gate path, distinguishes delivery
  on the due owner opportunity from discovery on lease return, and verifies
  that repeated failure parks without another wake or report.
  Successful explicit query observations remain cached through unrelated owner
  work, including after recovery. `registered_query_revision_is_observed_once`
  and `registered_query_reprobe_success_stays_cached` pin new refresh admission,
  unchanged numerical timing and cancellation. Query-derived due polls still
  resample changed timing; legacy arena sources keep owner-driven discovery.
  The pending-gate family also covers registered post-poll resampling and silence
  after the member clears its timing.

- **Completion belongs to a presentation.** Assembly installs that
  presentation's weak post-frame handle before mounting elements. An incomplete
  segment retains its callbacks; only a completed segment makes them eligible.
  Retained handles scheduled between segments create completion demand, which
  wakes a clean presentation without rebuilding its widget tree or submitting
  another scene. Withdrawal closes admission before retiring callback captures.
  `native_text_preparation_does_not_complete_the_presentation` and
  `clean_presentation_completion_keeps_the_tree_clean` pin the real pump paths.

- **Invalid authored text waits for changed input.** A typed text-layout error
  withholds scene submission and retains layout debt. The failure handler receives
  the ordinary error; unchanged invalid input does not request a continuous frame
  retry. A pending build or live layout invalidation resumes work. The public
  `invalid_authored_text_waits_for_changed_input_and_then_presents` drives the real
  pump, rebuild handle and sink through rejection and recovery (ADR-0181).

  A competing shared text-context loan also retains layout debt until an explicit
  input invalidation. It reports `TextContextBusy`, without poisoning the node,
  requesting native sizing or continuously retrying unchanged work. Releasing
  the loan alone does not create frame demand.
  `competing_text_measurement_waits_for_changed_input` pins refusal and subsequent
  changed-input recovery through the public runtime pump.

  An exact sizing source without a preparation owner likewise cannot progress
  by repeating unchanged measurements. Its first refusal retains layout and
  callback debt; changing the inherited provider resumes the presentation.
  `native_text_preparation_does_not_complete_the_presentation` includes the
  unavailable-source case and observes one callback after repaired geometry.

- **Registered text preparation resumes the same presentation segment.** The
  saved segment owns its original tick, host instant, constraints, accepted DPR,
  numeric policy, multi-source cohort and sealed layout/build identity premises. Actual
  admission advances premises before deduplication; retained transport does not.
  Dirty-channel requests are applied before compatibility is checked. Premises
  are checked again immediately before a segment, after sampling and its
  diagnostics. An obsolete retained sample is withdrawn and retried with
  a fresh next-pump tick; it cannot discharge completion debt.
  The sampled retained identity remains authoritative even if diagnostics
  remove the saved segment or replace it. A later preparation report gets one
  passive validation of its own admissions, without recursively draining
  callbacks or reporting. Fresh input remains same-pump eligible when the
  sampled operation had no former retained attempt.
  `a_sibling_animation_admission_replaces_ready_text_before_geometry_publication`
  pins accepted sibling input, withheld completion and the repaired glyph size.
  Native service receives an owned frontier after Runtime checkout returns and settles
  through the exact addressed owner FIFO. Lost receipts retain bounded retry
  debt; held expired receipts park geometry and permit document edits, while
  matching late replies can heal. Waiting requires the installed producer's
  readiness or park cue. Incomplete geometry is unavailable to IME, hit-testing
  and scene submission. `registered_text_preparation_preserves_the_actual_presentation_attempt`
  pins fractional continuation and repeated already-pending rebuild admission;
  `owner_delivery_contract` pins released-checkout service and close fencing.
  Delivery-only sampling preserves a compatible frozen frontier. A publication
  blocker invalidates old receipt authority, withdraws geometry and restores
  runnable layout while the replacement barrier stays uncommitted. The retained
  table's `unknown_deadline_withdraws_the_old_receipt_and_recovers_fresh_geometry`
  and `a_sampling_report_replaces_ready_text_before_geometry_publication` rows
  distinguish rejected late settlement from valid replacement geometry.
  Withdrawn closed numeric cohorts release their answer pins outside borrows;
  preserving an incoming opaque failure does not keep obsolete numeric answers
  pinned indefinitely.

- **Inherited DPR agrees with the render pipeline.** Initial media data uses
  the pipeline's accepted ratio. Direct updates reject non-positive and
  non-finite ratios before mutation. Native metrics reject these observations
  before coalescing, preserving an earlier accepted resize and independent
  appearance observations. `initial_inherited_scale_matches_the_renderer`,
  `resize_and_surface_restore_reach_the_product_frame` and
  `queued_state_bursts_coalesce_between_observing_frames` pin these paths.

- **Preference projection is updated with the accepted snapshot.** Root media
  publication derives text scale, text-weight adjustment, contrast and ordered
  preferred locales together. Unknown text weight leaves authored weights
  unchanged; categorical Bold projects to a deliberate +300 adjustment, and
  numeric observations preserve their signed value (ADR-0174).
  Unknown contrast uses
  normal contrast without altering the raw host observation. The mounted row
  `resize_and_surface_restore_reach_the_product_frame` checks contrast changes,
  unknown fallback and the seed seen by a later runtime's first build.
  `preferred_locales_select_resources_and_direction` checks actual localized
  resources and direction after publication and on a late runtime's first build
  (ADR-0173).
  `bold_text_changes_the_painted_glyphs` checks actual painted font instances for
  Text, RichText and EditableText, live restoration, subtree overrides and late
  runtime seeds.

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
  names none of `flui-native`, `winit`, `android-activity`, `ndk`,
  `windows`, `objc2-app-kit`, `objc2-ui-kit`, `wgpu`, `flui-engine` or
  `flui-app`: tier K's forbid set in the root
  `[workspace.metadata.flui.reach]`, checked by `cargo xtask reach` over
  every root build. A seam that needs a
  host type (the frame sink, the platform window) crosses as a trait this
  crate defines or one from `flui-platform`; a window's accessibility
  bridge is `flui_semantics::platform::PlatformAccessibility` (ADR-0082 §2,
  amended), and the development reload tier is this crate's own
  `reload::ReloadTier`, which the host translates from its hot-reload driver.
- **A UI runtime is owner-affine.** `UiRuntime` is `!Send + !Sync` (a raw-pointer
  `PhantomData` marker; pinned by `assert_not_impl_any!` in the UI runtime tests).
  Everything that crosses a thread goes through a `UiCommandSender` into a
  bounded inbox that the owner drains only while the scheduler is idle.
- **The owner host is an ordinary owner-affine value.** The implemented
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
  A contained callback panic, including installation initialization, emits a
  typed report at its presentation address before returning the runtime lease.
  Reporting does not increment frame-failure streaks or replace the original
  panic; diagnostics retain their own containment boundary (ADR-0185).
  A runtime operation's own panic stops there (ADR-0258 §1): its payload is
  retained, the turn stays healthy and the FIFO tail behind it runs
  (`a_failed_frame_is_contained_and_the_fifo_tail_runs`). Only a terminal one
  leaves the turn: the close of the last presentation of the last running UI
  runtime, or an operation the host shut down in (§2). A host hook's own
  failure (`after_turn`, the frame-rate hook, text preparation) leaves the turn
  as before.
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
  frame time projected through each presentation's motion clock before `Vsync`
  controllers tick (see "`Vsync` controllers tick at the
  frame's timestamp" below).
  Scheduler execution runs through `OwnerFrame::drive_frame`. A wake with
  frames disabled runs `OwnerFrame::pump_background` instead: consume old
  demand, drain commands and service runtime gesture geometry, acknowledge the
  drained redraw request, then poll ready tasks once and advance due input
  deadlines without animation sampling or a presentation segment. Preparation runs
  inside owner admission; the host background dispatcher does no prior drain.
  Whether a wake
  becomes a pump is the host's per-backend wake gate (ADR-0058), not the
  UI runtime's. Pinned by `ui_runtime/tests/pump_transaction.rs`, each test failing
  against a pump that skips or reorders the phase it names; `flui-app`'s
  `runner_frame_ordering` scan pins that every runner goes through it.
  Outside this crate the invariant holds by type: the draw step
  (`render_frame`) is crate-private and the owner-local post-frame lane
  (`local_post_frame_lane`) is `testing` only, so a host has no way to
  draw a frame, or end one, except the pump. Tests of the draw step alone
  reach it through `render_frame_for_test` and `draw_frame`, both under
  `testing`. `trybuild_ui::ui_tests` checks the private draw-step diagnostic
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
- **A UI runtime reads time from one `ClockSource`.** `UiRuntime::try_new` takes the
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
  `SemanticsHost`, the commit epoch) or, for
  `ExecutionServices`, one host loop, constructed only by the host's
  composition root. The one static is `runtime_services`' incarnation counter,
  an identity counter with an explicit ADR-0097 counter grant. Its `try_update`
  helper preserves permanent exhaustion instead of wrapping into a previous UI runtime.
- **The UI runtime's surface is the host's, not an embedder's.** `UiRuntime`,
  `PresentationState` and their methods are `pub` only where `flui-app`
  calls them; what only `flui-app`'s tests call is `pub` under
  `testing`; the rest is `pub(crate)`. `flui-app` re-exports none of
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
  `UiRuntime::try_new` takes the app's `FontCollection` (the host's shared engine
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
- **Test hooks stay behind `testing`.** Items that exist for tests, or
  that have no production caller yet (`HeldPointerQueue::append`/`len`,
  `SemanticsHost::ensure_semantics`, `outstanding_handles`,
  `ExecutionServices::with_limits`/`owns_default_pools`/`default_pools_started` and
  `platform_semantics_enabled`, the announce/event delivery, the UI runtime's
  `for_test` constructors and `*_for_test` probes such as
  `text_context_for_test`, the `testing` doubles),
  compile only under `cfg(test)` or the `testing` feature, which only
  dev edges and the headless test driver `flui-testing` enable (it is a
  test-only crate, reached by applications through dev edges or the facade's
  `testing` feature). Wiring one into production removes its gate in the same
  change. `UiRuntime::active_text_store` is one of them until a pull-model
  platform input method reads it. The exception is the `FrameClockSource`
  impl for `ManualClock`, which a trait impl cannot gate per caller.

## Mapping decisions

### Rejected execution restores enclosing text and clock transactions

The runtime maps owner execution refusals to an invariant failure: its live
driver owns scheduler and frame owner together, so terminal refusal is not an
ordinary frame that presented nothing. Preparation runs under owner admission,
before frame phases: it publishes time, drains commands, services geometry and
closes text commits. Rejection invokes no preparation, leaving accepted commands
pending. Timestamp and text-commit guards restore
their enclosing values on every exit. Rejected execution never reaches the
successful deferred-grant anchor. The internal frame helper returns only a
presentation boolean, so no opaque pipeline output survives into grant cleanup.

A drive that unwinds, or an anchor that fails, leaves the anchor owed
(ADR-0142 item 5). The next owner turn runs it first: the inbox drain, the
background pump, and the pump before its frame. A turn that ends with it still
owed asks for a wake, and only a completed anchor clears it. A failure owner
code parked in a presentation's gate after a grant the platform asked for
directly, outside any dispatch or frame, makes the anchor owed too, so it is
reported at that next turn.

While an owner lease holds the runtime, every presentation's commit gate is
checked out (ADR-0142 item 6): it admits only the `TextInputOwner`'s own calls
(`CommitGate::owner_entry`), so a text service reaching a store from a message
loop nested in a handler gets no synchronous lock and its asynchronous one
waits. The lease runs the commit anchor before the runtime returns to its
slot, and that grant runs there.

`rejected_nested_pump_preserves_outer_commits_and_animation_time` observes queued
store grants, synchronous lock exclusion, retained command delivery and animation
progress after refusal.
It uses the internal pump producer because safe public `pump(&mut self)` cannot
express native callback recursion. Guard restoration is private implementation;
the observations cross the text-store and animation interfaces.

### Native gesture geometry belongs to one accepted coordinate context

Each presentation queries its window when it is created, when accepted host
preferences change, and when its renderer accepts a different device pixel
ratio (ADR-0172). New contacts copy the complete checked projection; active
contacts retain their admission profile. Timing and wheel preferences publish
before inherited-data callbacks. A failed exact query retains usable geometry
only at the same ratio; a different ratio admits the consumer baseline until
recovery. Successful absence clears the cache. A successful but unrepresentable
projection acknowledges the query, retains that same-context fallback, and
waits for a real source/context change instead of repeating a deterministic
failure indefinitely.

Failed native reads retain a separate query obligation, paced by the injected
clock from 100 milliseconds to a one-second cap. Idle inbox drains, normal
pumps and background pumps service due obligations. One owner operation and
its nested host gates share a typed-presentation attempt budget, so slow
getters or host work cannot cause a second retry in the same operation.
Standalone pumps create their own turn. Deadlines join `next_wake`; querying
does not create redraw or synthetic-frame debt, and closing removes the
presentation's obligation. Strong barrier identities refuse stale query or
diagnostic continuations without a wrapping generation counter. Containment
preserves the first query, publication or pacing failure through recovery.
Background preparation failure leaves ready futures unpolled and restores their
wake delivery. Recovery adopts geometry before the next task batch, while
self-wakes remain pending for a subsequent background turn with frames disabled.

The public `owner_metrics_contract` rows
`native_geometry_controls_admission_and_retries_without_a_frame` and
`late_geometry_recovery_is_isolated_per_presentation` use actual mounted touch
contacts, the owner pump and an exact scripted window. They pin initial and
late admission, different-DPI fallback, active-contact continuity, independent
timing, capped repeated failure, slow getter and before-gate work, recovery
after a query panic, and isolated cancellation on close. They exercise the
runtime contract; native operating-system event translation requires its own
platform execution evidence.

`native_fling_profile_controls_real_scroll_inertia` connects the exact native
touch fling range to observed scrolling pixels, including an active contact's
retained maximum and the next contact's changed minimum. Its raw-velocity
source inverse produces an excessive coast instead of the positive bounded
motion required by the admitted profile.

Two private rows join `frame_pacing_and_pump_matrix` because its required
failure boundaries are unreachable through the current public runtime
baseline and the non-reentrant owner lease. The default touch ratios are one;
`checked_geometry_refusal_acknowledges_the_query_and_keeps_safe_admission`
injects a baseline ratio of two to exercise overflow of a valid finite native
reading, then recognizes a real tap using the retained or fallback projection.
`newer_geometry_publication_survives_query_and_diagnostic_reentry` invokes a
same-presentation replacement inside the actual getter and diagnostic. Mounted
inherited values, tap recognition and a hold deadline distinguish the latest
geometry/timing publication from stale healthy or panicking continuations.

### Host input is one value on one timeline

A presentation accepts `HostInput::{Platform, Window, Frame { vsync }, Accessibility}`
and nothing else (ADR-0207 §1). The owner FIFO admits platform input, window
observations and frames through `PresentationDispatcher::dispatch` (frames also
through the frame driver's `FrameDispatcher::deliver`, which admits the same
`Frame` work), so they share one order. An AT request is produced on the
accessibility adapter's thread, so it crosses the runtime's bounded command inbox
stamped for its presentation, and `dispatch` queues an owner-thread caller's
request there too (ADR-0207, amendment): every AT request is applied at the next
owner turn's Idle drain, before that turn's frame begins, so the frame the wake
runs already reflects it. The runtime unwraps the input: platform input to the
gesture binding (`dispatch_pointer_on`, whose sequence id is traced), focus and
text owners, a window observation to metrics and lifecycle, a frame's vsync to the
next render's motion sampling, and an AT request — decoded on the owner thread,
after the adapter's thread only transported it — to the semantics owner with an
assistive sequence id the binding minted. The window payload is the runtime's
normalized `WindowObservation`, derived from the window's state snapshot, so
no raw platform event crosses into it. `owner_input_contract`'s
`accessibility_input_reaches_the_semantics_owner` pins the AT path.

### Recorded input is emitted where it is applied

While a tap is registered (`UiRuntime::input_taps`, ADR-0215 §1) the runtime
emits an `InputRecord` per applied input from the methods that apply it —
`handle_input_addressed`, `apply_window_observation` (and the merged pending
window state, one row per value), `note_frame_vsync` and `dispatch_accessibility`
— so the owner FIFO and a headless host record the same rows. Each row is
admitted after its dispatch committed (a dispatch panic is recorded, then
resumed), followed by the arena decisions a `GestureObserver` on each presentation's
arena reported during it, each stamped with the time its arena committed it
(`a_decision_row_carries_its_commit_time`); decisions made inside a frame
follow its frame row when the pump ends. Pointer input a presentation holds for
its committed tree carries its row with the held event (`HeldInputRow`): the
frame's replay admits it after the verdicts reached before that dispatch, with
the input's arrival and the dispatch's sequence; a dispatch that fails still
admits it, in no sequence, before the failure resumes. Dropped undispatched,
wherever the held queue drops it, it admits itself as `Queued` with no
sequence, which replay skips. An applied preference revision is a
`Preferences` row.
Without a tap nothing is cloned and no arena is observed. Delivery is a batched pass over owned rows
with no runtime or registry borrow held: rows a tap admits reentrantly wait for
the next pass, which an owner wake starts. Every call is contained; the first
failure is kept, a lost row becomes a pending `Gap` count delivered in a second
contained call or before the next row, and an outgoing tap is retired after the
failure is reported, so its destructor's panic never comes first. Finishing (the
handle's `finish`, or the registry's drop for a tap installed for the runtime's
life) delivers the pending gap and the `End` row, or reports
`RecordingIncomplete`; finishing inside a tap call counts the rows its pass has
not offered yet, and rows queued for a later pass, as lost in those final rows.
An AT request no handler ran for is recorded `Unhandled`, as a key is. Handles are
generation-bound and generations are never reused. `input_tap_delivery_contract`
pins each of these.

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

### Focus leaves a removed node between layout and paint

A presentation segment runs build, then the build-during-layout fixpoint
(`WidgetsBinding::settle_layout_with_builders`), then
`FocusManager::settle_orphaned_focus` with no tree borrow held, then paint
(`PipelineCell::run_frame`). Removal records an orphan marker inside reconciliation
and runs no user code, so the one focus edge to the successor is published
here, in the frame that removed the node, and never through `None` while a
candidate remains. When that edge moved the primary, the segment rebuilds
(`rebuild_within_frame`, which keeps the frame's recovered-panic records) and
settles layout exactly once more before paint; what the listeners dirty beyond
that waits for the next frame. When that repeat itself removed the new focus,
the segment asks for the next frame (`FocusManager::has_orphaned_focus`), which
moves it on; nothing else may ask for one. Settling after paint would show a frame with
focus on a node that is gone; settling inside the fixpoint would run listeners
under the binding's write lock. `focus_survives_rebuild_and_moves_on_removal`
pins the edges, and its row
`the_successor_rebuilds_in_the_frame_that_moved_focus` the single repeat (a loop
would publish the second edge in the same frame) and the frame it asks for.

### A pointer press is focus input

`dispatch_pointer_event_entered` — live and replayed held input alike — calls
`FocusManager::note_pointer_input` on a button press (a `Down` or a
`ButtonChange::Pressed`) before routing it, so focus a click moves is not shown.
After routing, the back button (X1) going down or up is delivered to the same
presentation's focus as the `BrowserBack` key through
`FocusManager::dispatch_pointer_back`, which holds it apart from every keyboard
key. The three steps are contained separately and the first panic resumes.
A window that loses activation (`update_window_focus`, a window snapshot, or a
sibling taking focus) forgets its held keys: their releases go elsewhere. The
`addressed_input_routing` rows `the_back_button_is_a_back_key`,
`the_back_button_is_held_apart_from_the_back_key`,
`a_pointer_press_hides_keyboard_focus` and `deactivation_forgets_held_keys`
pin each.

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

Playback/debug progression is [ADR-0176](../../docs/adr/ADR-0176-presentation-animation-playback.md).
Host/application Normal/Preserve policy and rebasing are
[ADR-0195](../../docs/adr/ADR-0195-presentation-motion-policy.md). Their loading
feedback consumer is [ADR-0192](../../docs/adr/ADR-0192-accessible-loading-motion.md).
Registry ownership/topology and notification custody retain their separate
animation/foundation authorities; clock projection does not create a second driver.

The UI runtime's `Vsync` registry ticks at the timestamp the
pump's `FrameClockSource` returned (`raw_frame_time` reads it for the
frame's duration, relative to the UI runtime's start), so a controller advances by
frame time, not by whenever the tick happened to read the wall clock. A frame
driven outside a pump (a bare `draw_frame`/`render_frame` in a test) falls
back to the wall clock, and a test can still override it with
`set_now_secs_for_test`. The public
`one_pump_shares_its_selected_time_across_parent_and_child` row distinguishes
this selected timestamp from a host clock moved by an earlier callback.

Each presentation maps that one raw time through its own
`flui_animation::MotionClock` before ticking its registry, so the time a
registry sees is finite and never runs backwards: an override that is not a
duration, or a raw time earlier than the last, holds the animation
(`an_invalid_or_backwards_frame_time_holds_the_animation`). Each presentation
can change its own rate, pause or step through its exact agent window's owner
inbox ([ADR-0176](../../docs/adr/ADR-0176-presentation-animation-playback.md)).
There is no process-wide time multiplier or scheduler ticker controller path.

The presentation keeps one driver-bound registry for its lifetime. A bound
controller requests its first frame through its live seat, including a restart
whose directional status is unchanged. The frame capability holds only the
scheduler's weak wake edge, a weak window and the presentation's revocable
lifetime. A retained registry cannot wake surviving windows after its owner
closes (`closed_presentation_animation_cannot_wake_a_surviving_window`).
Registry callbacks and outgoing
capture retirement occur outside borrows. The public pump row
`starting_an_idle_presentation_animation_requests_its_first_frame` asserts
the request before pumping and then observes completion through that driver.

`agent_playback_drives_independent_windows_and_one_paused_step_frame` drives
two real runtime presentation registries at different rates, then observes
paused frame production and one addressed step frame without sibling demand.
`agent_motion_sets_rate_and_steps_a_paused_window` pins whole-request validation
and closed-window refusal through the public agent handle.

Hidden presentations and disabled frames do not invoke controllers or create
animation continuation demand. Each pump withdraws stale animation demand when
the presentation cannot advance a running animation. Explicit agent steps use
host demand, so an accepted inspection frame survives pausing or completion;
independent widget-build demand remains deliverable.
Their clocks still observe raw time, so the first visible frame catches up
instead of replaying hidden frames.
`gated_presentations_hold_samples_then_catch_up_when_visible` pins both gates
through the runtime frame producer for tween, repeat and spring, including
exactly-once terminal status and future delivery. The addressed step row above
also checks a completed run's inspection frame and an independent root rebuild.

Playback requests admitted by an animation listener stay in the owner inbox
until the next pump's idle boundary. The current traversal retains its selected
time for every presentation. `step_during_a_tick_applies_next_frame` drives the
public pump and addressed agent handle with a manual clock: a compound pause
and step remains unanswered during the current frame, advances only its target
on the next frame and leaves sibling continuation independent. Moving inbox
delivery to the end of the current frame fails the pending-answer assertion.

Stopping is terminal for frame eligibility even if the host later reports
`Resumed`. `a_stopping_realm_ticks_no_presentation` retains active registry
owners in two presentations, stops through the public runtime entry and pumps
past both deadlines. Source evaluation and displayed values stay held; removing
the stop entry's lifecycle commit fails the same scenario.

### `Vsync` ticks in the persistent phase, not among the transient callbacks

Scheduler tickers are retired under ADR-0175. Transient frame callbacks run
in begin frame, before the microtask flush and before any persistent callback.
The UI runtime's `Vsync` registry is ticked by `draw_frame_entered` before a new
presentation segment in the scheduler's persistent phase: after the transient
callbacks and microtasks, and after any persistent callback registered before
the pipeline. A controller's listener that schedules a microtask therefore
sees it flushed at the next frame's begin, not this one's. The two sets are
separate from the animation registry, so a controller has no second scheduler
ticker admission. The registry tick precedes that segment's build. Registered text preparation retains its
original tick and resumes only its layout/fixpoint tail; an intervening pump does
not advance that controller or reenter its widget build. Healthy presentations
can enter their own new segments. The public row
`an_animated_size_resumes_its_original_tick_after_host_service_yields` pins the
retained tick and resulting fractional glyph/IME geometry. Relative ordering
against arbitrary scheduler transient callbacks and microtasks remains unasserted.
Moving the registry tick into begin frame is a separate change.

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

This is continuation safety, not rollback. Arbitrary external effects remain
the application's responsibility. Pinned by the panicking secondary-presentation
signal command, command-capture destructor, and addressed keyboard/IME tests.

Today the first panic still leaves the dispatch boundary after the continuation
above. ADR-0258 (accepted, not implemented) ends that: each owner operation is
contained on its own and reported once as `FrameFailureKind::CallbackPanic`
through `UiRuntime::report_contained_panic`, the runtime goes back to its slot
and the FIFO tail runs; only a terminal failure (the last runtime closing, or
loop exit) leaves, and it goes to the host's terminal slot, to resume with 101
after the exit flush. The continuation rules here stay as they are; what
changes is that the panic stops after them instead of resuming.

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
effect happened: a `GestureDetector` runs a semantics tap synchronously inside the drain, so the
callback has run when the reply arrives, and what it changed reads in the tree committed by the
next frame. No lock guards per-node state: the owner reads its own tree on its
own thread, and the agent's side holds only its channel ends and the record of handles its reads
reported, which tells `gone` from `unknown_handle`. The record keeps the newest generation
reported per render slot, so it is bounded by the slots the presentation has used, not by how
many elements came and went. Element handles are render identities, scoped to the one
presentation; a server that spans windows keeps its own table over them (ADR-0095 §3).

**Locate and actionability.** `locate` and `actionability` are owner jobs like `read`
(`UiCommand::SemanticsLocate`, `UiCommand::ElementActionability`). A locate resolves the locator
over the committed tree (`SemanticsOwner::locate_wire`) and records the handles it reports like
a read. An actionability job runs three steps, none inside another's borrow: the semantics owner
says where the pointer would land (`actionability_point`), the render tree's pointer hit-test runs
there (render objects are user code, so never under a semantics borrow), and the owner reads the
facts against the path found (`actionability_wire`). A hit-test the render tree cannot serve
(`TreeBusy`) answers `RenderTreeBusy` (`busy`, retry `soon`); a render object that panics in it
answers `HitTestPanicked` (`platform`, nothing ran) first and its panic then continues on the
owner, as an action's does. `stable` needs bounds from an earlier call: `AgentStability`, on the
presentation, keeps the bounds of the last 64 elements read, and a comparison counts once a
terminal frame has run since (the presentation's tree revision moved) or while the presentation
has no work pending that could still move the element. The first call for an element, and one
made in the frame of the call before it with work pending, answer unstable. Pinned by
`src/ui_runtime/tests/agent_locate.rs` and `agent_stability.rs`.

**Act and read.** `act_and_read` (`UiCommand::SemanticsAgentActionRead`) takes the committed tree
as the before-snapshot (`SemanticsOwner::snapshot_wire`), serves the action as `act` does, parks
the reply on the presentation (`agent_reads`, at most 256) and requests a redraw. The reply
completes in `drive_frame`, right after the frame the effect landed in (before the commit anchor), by diffing
the new tree against the snapshot (`TreeSnapshot::delta_to`): appeared, gone and changed nodes
(name, value, description, state, actions; bounds only when asked), a role change counting as
appeared, capped at 256 entries with `truncated`. A presentation that closes first answers each
owed reading `readback_failed` with the effect `ran`, since the action was delivered. Pinned by
`src/ui_runtime/tests/agent_act_read.rs` and `crates/flui-semantics/tests/delta.rs`.

**Waits.** `wait` (`UiCommand::SemanticsAgentWait`) parks a condition on the presentation
(`agent_waits`) instead of polling it. The owner evaluates it when it arrives, so one that holds
already answers at the drain; after each frame `drive_frame` ran, which also counts that frame
for `Frames`; and at every owner wake that ran none (`drain_owner_inbox`), so a deadline or a
timer that ran out is seen. The tree part of a condition is a pure function of the committed tree
(`SemanticsOwner::evaluate_wait`); `Settled` and the unsettled causes come from the runtime's own
predicates (`needs_redraw`, a scheduled frame, pending work, transient callbacks, the next timer),
which `UiRuntime::unsettled_causes` reads without the agent's own waits, so a parked wait is never
why the window is busy. A condition that counts frames asks for a redraw while it is parked,
because an idle window presents none. The deadline is on the presentation's clock (a manual clock
expires it deterministically) and joins `next_wake`, so a window that draws nothing still times
out; a timeout is an answer, `satisfied: false` with the last observation and the causes, and the
window closing answers `gone`. A presentation holds 256 waits: the next is refused with
`TooManyWaits` (`busy`, retry `soon`), never dropped and never in place of an older one. A
condition on an event holds once the window emitted an event of that kind after the wait began
(see Events). Image loads are not among the causes: the runtime sits below the layer that loads
images. Pinned by `src/ui_runtime/tests/agent_wait.rs` and
`crates/flui-semantics/tests/wait.rs`.

**Events.** `subscribe` (`UiCommand::SemanticsAgentSubscribe`) registers a subscriber on the
presentation (`agent_events`, an `EventHub`) and answers with an `EventStream`; dropping the
stream ends the subscription (the hub sees that it holds the queue's last `Arc`). After each
frame `drive_frame` ran, the presentation emits what the frame left: a `frame` event per frame
presented or dropped since the last report, and a `semantics` event when the tree's revision
changed. Closing emits a last `window { opened: false }` and ends every stream. Each emission
also notes the event's kind on the waits parked on the window, which is what a `Condition::Event`
reads, so a wait and a subscription see one stream. A window with nothing to draw presents no
frame and emits nothing. Each subscriber's queue holds 256 events: one that falls behind has the
overflow counted, and the next event that fits is preceded by `gap { missed }`; a stream that
ends owing a gap queues it past the bound, so a stream never ends shorter than it says. A
presentation admits 64 live subscriptions and refuses the next with `TooManySubscriptions`
(`busy`, retry `soon`); a subscription with no filter is `invalid_argument`. Only events after
the registration are delivered, never a replay. `input` and `announcement` are accepted as
filters and not emitted yet (they come from the input journal and the announcement fan-out);
`frame.time_us` is not filled yet. Pinned by `src/ui_runtime/tests/agent_events.rs`.

**Inspect.** `inspect` (`UiCommand::SemanticsAgentInspect`) answers at the owner with the typed
snapshot a tree describes itself with (`flui_foundation::inspect`): `inspect_elements` for
`Widgets`, `inspect_render` for `Render`. The snapshot becomes the protocol's owned
`InspectNode` tree in `agent_inspect.rs` (the protocol crate has no foundation edge), with an
iterative walk so a deep chain costs no Rust stack; at most 4096 nodes are reported whatever the
caller asks. Ids on the wire are the tree's own handles (`v3.2`, `r12`); a `root` may be a
`NodeRef` (`p3/v3.2`), which must name this presentation: a reference to another or a closed
window is `UnknownPresentation` (`unknown_handle`, kind window) and is never looked up here, and
a handle of another tree is `invalid_argument`. A root that left its tree is `NodeGone`
(`gone`); a tree a frame holds is `TreeBusy` (`busy`, retry `soon`). `Layers` answers
`LayersNotKept` (`not_supported`): the sink takes the scene a frame paints, and the window keeps
no copy to read between frames. Pinned by `src/ui_runtime/tests/agent_inspect.rs` and, for the
reference's text, `a_node_reference_prints_and_parses_as_one_text` in `flui-foundation`. A render object or view that panics while the tree is read (`debug_name`, `inspect`) answers `InspectPanicked` (`platform`) first and then continues on the owner thread, as an action does; pinned by `a_render_object_that_panics_while_read_answers_platform_before_the_owner_resumes`.

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

### The performance overlay reads the frame history

The overlay's frame rate and frame time come from `flui_scheduler::frame_pace`
over the presentation clock's `FrameSnapshot` ring, the window its latency
percentiles already use: frame time is segment start to submit, on the frame
clock, and an idle gap longer than `FRAME_RUN_GAP` starts a new run instead of
counting as one slow frame. There is no second, wall-clock frame window.


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

A sampling operation records the first cause and any independent publication
blocker before recovery or diagnostics. Its one aggregate `SamplingPanic`
report is dropped only for an admitted attempt with a blocker. An outside-frame
query or deadline fault without retained work remains contained and does not
invent a frame-failure streak. A later preparation failure after diagnostics is
a separate operation and reports its own cause; it never repeats the earlier
sampling payload or changes that sealed disposition.

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

A dropped frame that retries backs off (ADR-0258 §6). After the `k`-th consecutive
drop the presentation waits `T · 2^min(k − 1, 10)`, `T` its display's refresh period,
capped at `FAILURE_RETRY_CAP` (1 s); the exponent is bounded before the shift. The
wait is a deadline on the presentation that `UiRuntime::next_wake` reports, not a
frame asked for: until it ends the presentation's segment is not admitted, its
frame work does not count as pending, and a late redraw request does not wake the
host, so a host that sleeps until `next_wake` makes one attempt per deadline. A key
press or a pointer button press ends the wait once per interval; motion, signal
writes, tickers and the frame's own invalidations keep their demand for the
attempt. A text failure that waits for input arms nothing. A clean segment clears
the streak and the wait. `headless_frame_driver_matrix` in `flui-testing` pins the
cadence at 60, 120 and 240 Hz, the single-failure redraw, the press bypass and a
streak of forty.

Sampling diagnostics run after every presentation has sampled and established
recovery, before the build walk. Their mutations are reconciled before any
retained geometry can publish. Outside-frame scheduling reports use an owned
batch taken before checkout returns and delivered afterwards, without a runtime
storage guard. Each report uses the current handler, so replacement can affect
later reports without reentering the original checkout. `owner_sampling_contract`
pins this public owner path with query, tracing, handler and earlier-operation
failures in competition.

### Frame-aligned haptics resolve with a painted frame

Each presentation owns a `HapticsHost` (ADR-0210 §4) on its gesture clock,
playing on its window's actuator through a weak reference, with the platform
row's deadline factor and the window's current refresh period. A frame the sink
reports `Presented` plays the queued requests whose deadline has not passed; a
frame that paints nothing, one the backend found no damage in (`NoPresent`: the
screen keeps the previous frame) and one it could not show (`NotShown`) present
nothing, so their requests wait for a later presented frame or resolve to
`Suppressed` at their deadline, as the ADR wants feedback only with the change it
confirms. Resolution runs no user
code: outcomes wait in the mailbox, and the observer runs in the
`HapticDelivery` sampling stage, under the presentation's failure custody, on
frame and background owner turns alike. The host's next wake (the first time
after the earliest deadline, or now while a delivery pass is due) joins
`next_wake`, so recovery and expiry need no unrelated input. Hiding, occluding or
suspending the window drains the queue; the accepted haptics preference reaches
every presentation, including one opened later, and turning haptics off drains
its queue the same way; closing runs one final delivery
pass, or, after a failure, withdraws the observers without running them. Any
number of observers register through `HapticsHandle::observe`; each outcome
reaches every one of them once, and leaves the mailbox when all have it. **Tests:** `frame_presented_widget_haptics_follow_the_presentation`,
`haptics_preference_suppresses_both_tiers`,
`frame_aligned_haptics_play_only_with_a_presented_frame`, and the queue's own rows in
`flui-interaction` (`frame_presented_haptics_play_with_the_frame` and the
mailbox rows ADR-0210 names, with `several_observers_each_receive_every_outcome_once`
and `removing_an_observer_releases_the_head_it_held`).

### A presentation stores its display; every pacing reader reads that one value

The host derives which display a window is on (and its refresh period) and delivers
it as `WindowObservation::Display` ([ADR-0236](../../docs/adr/ADR-0236-display-snapshot-and-refresh-period.md)).
Pending updates merge as a whole `DisplayUpdate`, so a newer "unknown" replaces an
older known display instead of losing to it
(`newer_unknown_display_replaces_a_pending_known_one`). A presentation applies a
changed display by moving everything that follows it (the pointer resampler's
window, the gesture-geometry barrier, a redraw) and stores the value last, so a step
that fails part-way leaves the previous value and the next delivery, even of an equal
value, applies it again; an unchanged value does nothing.

`UiRuntime::refresh_interval` is the one reader's answer: the display's period, or
`DEFAULT_REFRESH_PERIOD` (60 Hz, a policy of this crate, never a value the snapshot
carries) while it is unknown, and it falls back when the window moves to a display that
reports nothing. The scheduler's frame budget is set to the shortest interval among the
runtime's presentations in the pump's preparation, before the frame begins, because the
frame copies the budget's target when it starts
(`jank_is_judged_against_the_current_period`,
`shared_frame_budget_uses_the_shortest_period`). The presentation shares its stored
`WindowDisplay` with its haptics host, whose frame deadline is ADR-0212's metric:
`flui_interaction::haptic_frame_deadline` multiplies the display's period by the
preset's factor, and a period that is unknown or whose product overflows uses the 60 Hz
fallback with a diagnostic (`unknown_refresh_period_uses_the_60_hz_fallback`). No tick or
media-query field carries the period: nothing in animation reads it from there.

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

Animation authority is withdrawn independently of retained tree ownership.
Each presentation prepares terminal registry closure before other teardown
callouts, then publishes cancellation after its capabilities are withdrawn.
Whole-runtime close prepares every presentation before delivering the first
cancellation; addressed close leaves sibling registries live. Saved animation
observers refuse new runs even if exceptional retention keeps their widget
owner allocated. The retirement receipt borrows the enclosing failure policy
and completes healthy cancellation tails without invoking opaque destructors
after a failure. `stopping_the_realm_mid_animation_cancels_every_run` pins these
ordering, outcome, reentry and competing-failure contracts (ADR-0175).

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

### A dropping UI runtime destroys its application values before it raises a failure

**Rule:** `UiRuntime::drop` raises its first failure only once no field it still owns holds an
application value. A failure raised from `Drop::drop` unwinds through the field drop glue that
follows; a capture whose destructor panics there is a second panic in one unwind, and aborts
(#1165).

**Choice:** after the presentations and the owner-local frame work retire, the drop takes the
queued commands (a cross-thread signal write carries the application's closure) and the
frame-failure handler, closes the command inbox, and only then destroys them under the same catch,
first failure kept: a destructor that sends through a sender it kept is refused with `OwnerGone`
rather than queueing a capture only the drop glue would reach. Once a failure
is held, or the thread is already unwinding, it retains them instead (ADR-0127). Every other field
holds framework values, or values already contained where they run: the input tap's calls and
retirement, and the presentations' trees, whose `dispose` and pending futures are contained per
element so the rest of the tree still disposes; the first of those is then the presentation's
close failure, since no frame follows to report it (ADR-0258 §2). Tested by
`a_queued_command_outliving_a_teardown_failure_does_not_abort`,
`a_handler_capture_sending_on_drop_is_refused`, `a_failing_dispose_and_future_drop_do_not_abort`
and `user_drop_panic_inside_an_unwind_is_retained`,
each run in a child process because the broken shape aborts.

### A presentation's frame-rate demand is decided here and delivered as an owner effect

**Rule:** the runtime has no platform edge (ADR-0083), so it decides what a
presentation wants of its display's refresh rate and publishes the change; the host
turns it into a window request.

**Choice:** a presentation is `FrameRateDemand::Active` while an animation runs
(`vsync` has a running controller, the motion clock is not paused, frames are
enabled and the presentation is shown) or a pointer is down, and `Idle` after
neither has held for 250 ms (`FRAME_RATE_HOLD`). The demand is read live at the end
of every runtime work item, next to the text-sizing frontiers: the clock's
animation bit cannot be read there, because `poll` clears it after each produced
frame. The host opts in through `OwnerEffects::tracks_frame_rate_demand` (off by
default; Android and iOS answer `true`): without it no demand is observed, no hold
is armed and no deadline joins the host's wake sources. The owner keeps, per
presentation, the demand it last delivered, and each work item offers
`OwnerEffects::frame_rate_demand_changed` whatever differs from it, after the
runtime has returned to the owner and not for a presentation that retired. A
demand counts as delivered only once the hook returned, so a hook that panics is
offered the same change again at the next work item. The hold is a wall-clock
deadline: it joins `UiRuntime::next_wake`, and `has_pending_work` reports it once
due, so a static screen, which produces no frames, still reaches `Idle` and the
frame closure of a host does not skip a due wake and spin. A hidden or closing
presentation draws nothing: it is released at once, with no hold, and reports no
hold deadline, so a hold that comes due while it is hidden cannot leave
`has_pending_work` true with no work item to clear it.

**Alternatives considered:**
- Calling the window from the runtime — it would give the runtime a platform edge.
- Handing the request to the application — the owner decided the runtime asks, by
  activity.
- Releasing without a hold — the rate would flutter between the frames of an
  animation.

**Trade-off:** a host that tracks demand pays one extra wake per interaction, 250 ms
after it ends; the host re-applies its stored demand when a native surface is
replaced.
