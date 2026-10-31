# flui-testing Architecture

The workspace's test support, placed above the frame runtime and the widget
catalog (tier K, `order = 6`) so its driver can run the product frame
transaction ([ADR-0083](../../docs/adr/ADR-0083-one-frame-transaction-in-flui-runtime.md) §4).
Two drivers live here while that move is in progress: `HeadlessHost`, which
pumps a `flui_runtime::ui_runtime::UiRuntime`, and `HeadlessBinding`, the
substrate driver over raw owners, which the raw-owner suites still use.

## Invariants

- **Raw-owner animation work runs inside its scheduler frame.**
  `HeadlessBinding::pump_frame` projects time, then ticks Vsync in the persistent
  pipeline slot before build/layout/paint. This matches runtime sampling and
  gives the work the owning frame's tracing extent (ADR-0202).
  Devtools' `a_real_animation_reaches_the_profiler_with_its_work` pins actual
  animation progress, same-frame paint and CPU phase attribution.

- **Adopted widget clocks have one runtime parent.** Re-adopting the same registry
  is a no-op. A foreign, closed or otherwise refused replacement returns its typed
  admission error and keeps the current driver; outgoing registry custody drops
  after releasing the adoption borrow. The public
  `widget_registry_adoption_preserves_its_driver_on_refusal` observes old and new
  run completion across refusal and corrected adoption (ADR-0191).

- **Application motion preference uses the runtime's publication path.**
  `HeadlessHost::set_motion_override` and the widget harness's matching method
  call `UiRuntime::set_motion_override` inside the owner entry. The runtime
  updates presentation clocks and inherited data; the harness owns no second
  policy resolver. Material's `press_highlight_lasts_its_delay_under_reduced_motion`
  and `snack_bar_keeps_its_display_duration_under_reduced_motion` drive real
  mounted timers through this entry and observe both retained duration and expiry.

- **Extra presentation motion clocks remain independent.** An owning controller
  registers through `build_on` and remains alive for the scripted run. The
  harness has no manual controller-registration shortcut. Editing an extra
  presentation clock stays within its borrow; a step marks that presentation's
  demand after the borrow is released. A paused clock contributes no continuous
  animation demand. `presentation_rates_pause_and_step_are_independent` pins
  distinct rates, sibling progress, no paused produce and exactly one step
  produce. The runtime and development-agent projection is still separate.

- **A harness frame is `UiRuntime::pump`.** `host::HeadlessHost::pump` is the
  only way the widget harness (`widgets::lay_out`, `widgets::harness::mount`)
  draws a frame, the mount included: apply commands, begin frame, the
  pipeline, end frame and the text-store commit anchor all run inside the
  UI runtime's transaction. Nothing here re-implements a phase; the tests in
  `tests/runtime_driver.rs` fail against a harness that drives the pipeline
  itself (the root `MediaQuery`, the commit gate, the owner inbox, the
  window's cursor).
- **A scale change has one runtime entry.** `HeadlessHost::set_scale_factor`
  updates its native-window stand-in and surface dimensions through
  `UiRuntime::apply_window_observation` with a `Metrics` observation, the
  runner's path, so a recording holds the change as a row
  (`a_scale_change_mid_recording_replays_to_the_same_decisions`). The runtime
  owns render/semantics and inherited-data publication; the driver cannot
  mutate its internal source. `a_secondary_window_publishes_its_own_media_query`
  and the two addressed semantics scale cases in `headless_frame_driver_matrix`
  pin this path.
- **One `ManualClock` drives the UI runtime.** The UI runtime takes it as its
  `ClockSource`, so the frame-time origin, the gesture arena's deadlines and
  each presentation's `FrameClock` read it, and the pump reads a clone of it
  as the frame's timestamp. Time moves only when a caller advances it:
  `HeadlessHost::pump(dt)` before the frame, or a pointer helper's sample
  interval.
- **A contained failure is raised after the pump, and the first one wins.**
  The UI runtime contains a segment panic or pipeline error as a dropped frame
  (ADR-0048) and reports it to the handler `HeadlessHost` installs, with
  text retained verbatim. `pump` raises the first report once the pump has
  returned; when a later panic unwinds out of the same pump, the report is
  raised and the later payload is leaked (its destructor could panic),
  logged at error level. A lifecycle panic the tree recovered from (an
  `ErrorView`) is not raised: its frame completed. A report the UI runtime makes
  between pumps is raised by the next pump before it frames, not erased.
  After any raise, including a pump that unwound past its commit anchor, the
  next pump frames and runs the grants the unwound one queued. Pinned by
  `tests/headless_host.rs`, `host::tests` and the failure tests in
  `tests/runtime_driver.rs`.
- **Presentation completion requires coherent geometry.** A scoped callback
  accepted through `HeadlessHost::post_frame_handle` waits across a failed frame,
  runs after repair and remains consumed if that callback itself unwinds.
  `an_incomplete_frame_retains_callbacks_until_repair_and_recovers_after_their_unwind`
  pins all three boundaries. The scheduler's unscoped host-frame lane remains
  deliverable during a presentation failure; the invocation count in
  `a_contained_frame_failure_stays_authoritative_over_a_later_post_frame_panic`
  proves its unwind competes with the contained failure in the same pump.
- **The UI runtime root is attached once.** The widget harness attaches one
  harness root that builds whatever tree its slot holds; a root swap
  replaces the slot and rebuilds that root, so the UI runtime's root scopes stay
  mounted and a root of the same type updates in place.
- **Input travels the UI runtime's input path.** Pointer events go through
  `UiRuntime::handle_input_addressed` inside the UI runtime's entry; the moves it
  queues are flushed before the helper returns, so a synthetic move is
  observable at once. IME events go the same way, into the presentation's
  own text-input owner.
- **`HeadlessBinding` is the substrate driver until the second half of §4.**
  It drives raw owners (`flui-view`, scheduler, animation and interaction
  suites, the `perf` target, the facade's `tests/*.rs`) and the
  configurations a UI runtime cannot express. Its `pump_frame`, `run_pipeline` and
  `pump_presentation`/`pump_all` go when those suites move to the pump
  (ADR-0083 `## Migration`, move 6b).
  Its raw-owner frame uses the same complete `OwnerFrame::drive_frame` operation
  as the production runtime, preserving mid-frame task polling and post-frame
  observation of committed layout. An admission refusal is a driver invariant
  failure; it cannot be reported as a successful empty frame.
- **Only `log_capture` crosses the dev cycles.** `flui-widgets` and
  `flui-runtime` name this crate on dev edges while it depends on both, so
  their unit-test builds compile a second copy of themselves. Their unit
  tests use only `log_capture`, which names no type of either crate. The
  compiler does not refuse more: a `flui-widgets` unit test could hand
  `widgets::lay_out` a view (the `View` trait lives in `flui-view`, of which
  there is one copy), and its lookups of `MediaQuery`, `FocusRoot` or
  `VsyncScope` would then silently miss the scopes the UI runtime installed from
  the other copy. Review keeps harness tests in `tests/`.
- **The UI runtime stays behind its host.** `HeadlessHost::UI runtime` and
  `HeadlessHost::enter` are crate-private, and `LaidOut` and `Harness` hand
  out narrow accessors (the window's cursor, the accessibility action
  listener, the post-frame handle, the scheduler) rather than the UI runtime:
  `flui-runtime` is not an embedder API, and `flui::testing` re-exports
  `widgets`, so a public path to `UiRuntime` here would open the UI runtime's frame
  entry points (ADR-0083 §2) to every application with the `testing`
  feature.
- **A recording crosses the wire only through private mirrors.**
  `recording` serialises `InputRecord` rows through DTOs with pinned
  spellings; the vocabulary carries no serde. Unknown fields are skipped, an
  unknown variant (or key, code or action spelling) is
  `RecordingError::UnknownVariant`, never a skipped row. The DTO cannot match the
  `#[non_exhaustive]` vocabulary exhaustively, so `cargo xtask
  recording-vocabulary` generates `tests/generated/recording_variants.rs` from
  the sources and `every_vocabulary_variant_round_trips_through_the_recording_dto`
  needs a sample per entry that survives a write and a read. Text (key text,
  IME strings, assistive values, window titles, device names) is redacted
  unless the header keeps it, and a row that lost text is marked so replay
  refuses it (`an_assistive_set_value_on_a_password_field_records_no_plaintext`).
  The tap only converts rows on the owner thread; a worker thread writes them
  (inline on wasm).
- **Replay validates before it dispatches.** `ReplayableRecording::read`
  reads through the `End` row and refuses a `Gap`, a redacted row or a missing
  end (`a_truncated_recording_dispatches_nothing`) before
  `HeadlessHost::replay` can apply a row; the replay advances the clock to each
  row's arrival, applies preference rows at their positions and returns the
  decisions it made (`a_recording_replays_to_the_decisions_it_recorded`).
- **A headless frame is a delivered `HostInput::Frame`.** `HeadlessHost::pump`
  notes the clock's reading as every window's vsync before it frames, so
  recordings see frames where deadlines fire, and its wake sets the redraw
  flag, as a runner's does, so `pump_until_settled` observes a running
  animation's demand.

## Mapping decisions

### Registry drivers revoke their frame authority on replacement

Default, adopted and extra presentation registries request samples through the
binding's weak scheduler wake capability. Each driver also carries weak authority
owned by its binding or clock entry. Replacement commits the new registry and
revokes the old authority before invoking callbacks or retiring outgoing state.
Installation failure still finishes outgoing retirement, retaining the first
failure when both compete. A standalone old registry remains manually drivable;
its observer clones cannot wake the binding that replaced it.

`starting_an_idle_bound_controller_requests_its_first_frame`,
`adopting_a_registry_revokes_the_previous_driver_binding` and
`driver_replacement_preserves_first_failure_and_recovers` exercise public handles
in `headless_frame_driver_matrix`. The public
`extra_registry_drivers_request_frames_and_retire_on_replacement` row checks
extra clock installation and replacement while preserving a sibling's driver.

### Unique accessibility queries materialize only failure diagnostics

`A11yTree::find` and `find_by_label` inspect their matching iterator directly.
One matching node returns without collecting a temporary node vector. A second
match establishes ambiguity, but the iterator is then exhausted to preserve
every matching description in tree order. Missing subjects retain the complete
reachable-tree diagnostic. The public
`unique_queries_preserve_subjects_and_complete_failure_diagnostics` row in
`headless_frame_driver_matrix` covers both selectors, unique and missing
subjects, and three distinguishable duplicates emitted in a different order.

These queries inspect the raw root-reachable AccessKit update; they do not
implement an OS adapter's hidden-node and transparent-container filters. The
development agent's wire projection performs the shared consumer filtering
under ADR-0095. Keeping the distinction explicit avoids treating a raw payload
query as proof that a screen reader can reach the subject.

### The harness pumps the UI runtime

The harness calls `HeadlessHost::pump(dt)`, which
advances the manual clock and runs `UiRuntime::pump`, the one frame
transaction every runner drives. `lay_out`'s mount is a frame:
post-frame callbacks registered in `init_state` run at its
end.

Two behaviors follow from the UI runtime, not from the harness. The UI runtime's
`Vsync` registry ticks in the persistent phase with the pipeline, not among
the transient callbacks (recorded in `flui-runtime`'s `ARCHITECTURE.md`,
"`Vsync` ticks in the persistent phase, not among the transient
callbacks"); a caller's own registry passed to `lay_out_animated` is ticked
in the same phase at the same time. And the UI runtime coalesces pointer moves
until the next frame; the harness flushes the queue after each event, through
the same dispatch code the frame would run, so a test observes a move
immediately.

### Logical render roots follow reconciliation

`LaidOut::root`, `current_root` and render-type traversal resolve the caller's
live render subtree after every frame. A composition root remains mounted while
its child changes render type or a contained build failure installs an error
view; a cached render ID would then point at the detached child. The
`logical_render_root_tracks_replacement_and_build_recovery` row in
`headless_frame_driver_matrix` checks healthy replacement, the rendered error
slot and recovery through the root's own rebuild handle.

### The conformance kit drops caught opaque panic payloads

`Case::run` borrows its fixture and converts a caught panic into a named
`CaseFailure`. After extracting string diagnostics, it drops the opaque
payload (ADR-0281). The `kit_drops_opaque_failure_payloads_and_continues` row
in `text_store_kit_matrix` injects two independent aggregate payloads through
public fixture reset, checks both named failures and that their destructors
ran, then runs the kit successfully again.

The kit's deliberately panicking-grant case also confirms that the supplied
grant actually ran. A store panic before invocation is reported through the
outer case boundary. The
`a_store_failure_before_the_grant_is_reported_and_next_grant_progresses` row
exercises that distinction and then issues a successful grant on the same
consumer store. Store ownership remains with the fixture in these regressions;
normal arbitrary store destructor behavior is governed by Rust's unwind rules.

### A reused signal probe follows its current mount

Each build refreshes the probe's observed signal and reactive graph together.
A sequential remount creates a new signal, so retaining the first pair would
read a released slot while the new mount remained live. Old graph retirement
occurs outside the observation cell's borrow.
`a_signal_probe_reads_and_writes_its_current_mount_after_remount` in
`headless_frame_driver_matrix` writes through actual pointer callbacks before
and after remount, and reads the new initial value between them.

### The gesture-alternatives check mounts afresh for every path

`assert_gesture_alternatives` builds a new copy of its subject for each path,
so one path's effect cannot be read as another's, and compares each path's
model with a reference path: the gesture for the first command, assistive
technology's custom action for the rest. A path that leaves the model
unchanged fails even when it matches, so a broken widget cannot pass by doing
nothing on every path. It reads the menu's items from the semantics tree, not
from the presenter, so a presenter whose items assistive technology cannot
reach fails; it never hovers, so what it reaches is reachable without hover.
`the_check_passes_a_conforming_row_and_names_each_broken_path` pins a
gesture with an effect of its own, an unanswered intent and a silent menu.
