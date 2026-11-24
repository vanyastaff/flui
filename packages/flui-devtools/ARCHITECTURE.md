# flui-devtools Architecture

`flui-devtools` is an official package: it depends on `flui-sdk` alone
([ADR-0088](../../docs/adr/ADR-0088-official-packages-sdk-and-facade.md)), so the framework
never calls it. It reads what the framework already emits (`tracing` spans and events, the
`TreeObserver` and `GestureObserver` seams) and owns no endpoint, no file watcher and no tree
walk. Modules, features and the public surface are in [README.md](README.md) and the rustdoc.
This document states the invariants each module holds and the test that pins each.

## The capture is the one recorder

`Capture` is the only reader of the framework's frame spans and frame telemetry; `Profiler` and
`Timeline::over` are views over it. `Capture::layer` is one `tracing` layer with its own
per-layer filter. It admits:

- the `frame`, `presentation` and phase spans of target `flui.frame`;
- the animation controller spans of their declared target `flui.animation`, and nothing a
  span of the same name opens elsewhere;
- of the events, only `frame_telemetry` and `frame_gpu_time` at `TRACE`, plus the `enabled!`
  gate the runtime asks before it builds them.

Because the filter is the layer's own, FLUI's default `INFO` log filter does not starve it and
attaching it does not turn `DEBUG` or `TRACE` on for the rest of the process.
`the_filter_admits_the_telemetry_gate_and_no_other_trace_callsite` pins the event side;
`an_application_span_named_like_a_phase_is_not_a_phase` and
`every_phase_is_found_by_its_own_target_and_name_and_by_nothing_else` pin that a span is a
phase by `(target, name)`, never by name.

A capture's state in a span's extensions is filed under its `CaptureId`, minted from a
never-wrapping counter, so two captures on one registry see every frame apart and a capture
created after another was freed cannot read what the freed one left in an open span
(`two_captures_on_one_registry_each_see_every_frame`,
`a_new_capture_does_not_read_a_dropped_captures_state`).

## Frames belong to their runtime's outermost scheduler frame

A frame is the scheduler's `frame` span, recognized by its producer target and span identity.
It belongs to the runtime named by its `runtime` field, and a span inside it to the presentation
whose segment it ran in (`Partition`); a span with no known owner is kept apart
(`spans_are_partitioned_by_the_runtime_and_presentation_they_ran_for`,
`a_frame_with_no_runtime_is_kept_apart`, `concurrent_runtimes_are_captured_apart`).

- Only spans inside a frame are captured; a phase outside any frame is not a frame
  (`only_the_frameworks_phases_inside_a_frame_are_captured`).
- A frame opened inside another frame on its thread, such as a nested runtime's, is captured
  but is not outermost, and its phases are dropped from the enclosing frame whole: finding the
  owning frame somewhere among a phase's ancestors would blend two runtimes
  (`a_nested_frame_is_not_outermost`,
  `a_phase_of_a_nested_frame_is_not_covered_by_the_enclosing_frames_phase`,
  `a_nested_frames_phase_does_not_inherit_the_enclosing_presentation`,
  `a_nested_runtime_cannot_blend_its_animation_into_the_owning_frame`).
- Frames and phases end at their outermost exit, and reentry into the same phase adds no second
  duration. A clone of a span retained by application code extends neither the phase nor its
  frame into a later one, and a closed span cannot restart a measurement
  (`a_reentrant_phase_is_counted_once`,
  `retained_rendering_spans_cannot_charge_a_later_frame`,
  `retained_sampling_spans_end_at_exit_and_do_not_charge_a_later_frame`,
  `a_closed_sampling_span_cannot_charge_listener_work_in_the_same_frame`,
  `application_frame_names_do_not_hide_real_animation_work`).
- Animation tick, sampling and listener timings are inclusive extents, so they do not add up to
  a frame CPU total (`animation_phase_names_and_inclusive_extents_are_respected`,
  `a_real_animation_reaches_the_profiler_with_its_work`).
- A frame whose task panics before the pipeline runs still names its runtime
  (`a_frame_whose_task_panics_still_names_its_runtime`).

## Frame sinks

`Capture::on_frame` registers a sink and returns a `CaptureSubscription`; the sink is removed
when the handle drops and not before. A sink runs on the thread that closed the frame, after the
frame is stored and with no lock held, in registration order. A sink that is removed during a
delivery finishes that delivery and misses the next.

Delivery runs under one `FirstPanic` and never unwinds out of the layer, since that would stop
the other layers from seeing the span exit. The first panic of a frame is reported as one
`ERROR` event on target `flui.devtools`, and the remaining sinks still run; a failing report
does not unwind either. A sink's identity is a minted id, so removal takes exactly that sink.
Pinned by `a_frame_sink_sees_each_frame_after_it_is_stored`,
`ending_a_sink_handle_removes_exactly_that_sink`,
`a_sink_removed_during_a_delivery_finishes_that_delivery_and_misses_the_next`,
`a_panicking_sink_is_reported_once_per_frame_and_the_others_still_run`,
`a_panicking_sink_does_not_stop_another_capture_on_the_registry`,
`a_failing_failure_report_does_not_unwind_out_of_the_layer` and
`a_capture_with_sinks_is_freed_with_its_last_handle`.

## Produced frames and the bound per runtime

A `ProducedFrame` is read from the `frame_telemetry` event of its runtime; its GPU time is
joined later from a `frame_gpu_time` event that names the same runtime, presentation and frame
number. An event naming no valid id is skipped.
`CaptureLimits` bounds frames, produced frames and spans per runtime, so one runtime never
evicts another's (`telemetry_events_are_produced_frames_with_their_gpu_time`,
`telemetry_naming_no_valid_id_is_skipped`, `produced_frames_are_bounded_per_runtime`,
`a_runtime_never_evicts_another_runtimes_frames`,
`the_span_capacity_bounds_the_retained_frames_contents`,
`a_timed_frame_carries_its_gpu_time_into_the_trace`).

## The profiler follows one runtime

A `Profiler` is a capture sink with one stream of frames: the runtime
`ProfilerConfig::runtime` names, or the first runtime whose frame names itself. Frames of other
runtimes stay in the capture, and a nested frame is not profiled
(`a_profiler_follows_one_runtime_and_the_capture_keeps_both`,
`a_profiler_over_a_named_runtime_ignores_the_others`,
`another_runtimes_frame_closing_does_not_disturb_the_followed_frame`,
`a_nested_frame_is_not_profiled`). It is held weakly by the capture and removes its sink when
its last handle drops (`dropping_the_last_profiler_removes_its_sink`).

- `frame_rate` is `(n - 1)` over the span from the first retained frame's start to the last's,
  so it counts frames the scheduler produced, not how fast their work ran
  (`frame_rate_counts_produced_frames_per_second_from_frame_starts`).
- A frame is jank against `JankLine`: by default the display period its `frame` span reports,
  and a zero period counts as none (`a_fixed_jank_line_judges_each_frame_by_its_duration`,
  `a_zero_display_period_is_judged_as_no_period`,
  `a_real_frame_is_judged_against_the_period_its_scheduler_reports`).
- `ProfilerConfig::max_frame_history` is a maximum, zero included: at zero the aggregate frame
  and jank counters still progress while `frame_history` is empty
  (`zero_history_capacity_keeps_counters_without_frames`).

## The timeline

- **Guards survive trims and clears.** An `EventGuard` names its event by mint order through a
  base id, not by its position. A trim of the oldest events advances the base so surviving
  guards still resolve, a guard of a trimmed event closes nothing, and `clear` moves the base to
  the next issued id so a guard from before the clear can neither change a new event nor block
  a new guard (`a_bounded_timeline_keeps_the_newest_events_and_a_trimmed_guard_closes_nothing`,
  `a_guard_before_clear_cannot_update_a_new_event`,
  `a_guard_after_clear_updates_its_own_event`,
  `developer_history_respects_capacity_and_clear`).
- **Conversion runs before the lock.** Event names take `Into<String>`, which is consumer code
  and may read or clear the same timeline. All three admission paths convert before locking
  (`guard_name_conversion_can_reenter_the_timeline`,
  `instant_name_conversion_can_reenter_the_timeline`,
  `completed_name_conversion_can_reenter_the_timeline`).
- **Durations saturate.** `TimelineEvent::duration` rebuilds seconds and nanoseconds from the
  public `u128` microseconds and returns `Duration::MAX` beyond its range; the trace export
  saturates its `u64` fields the same way (`completed_event_duration_preserves_large_values`).
- A JSON event naming no valid runtime is refused
  (`a_json_event_naming_no_valid_runtime_is_refused`), and the export reads back with each
  event's kind and ids (`the_json_export_reads_back_with_each_events_kind_and_ids`).

## The inspector is installed by the application

`InspectorCounters` is installed through `AppConfig::dev_tree_observer`, whose factory every UI
runtime the runner builds calls once with its id. The factory is `Send + Sync` and the counters
are shared with the application's threads, so they are private atomics: tallies are `Relaxed`,
and the end-of-stream flag is Release/Acquire so a snapshot that sees the stream end sees every
count before it. A panicking factory is logged and its runtime runs without an observer
(`an_apps_dev_tree_observer_counts_its_runtimes_mounts`,
`a_panicking_dev_tree_observer_factory_leaves_the_runtime_running`). Counts are exact over mount,
move, rebuild and unmount, and a panicking observer is detached without harming the tree
(`inspector_counts_mounts_moves_rebuilds_and_unmounts_exactly`,
`every_rebuild_cause_is_counted`, `panicking_observer_is_detached_and_the_tree_survives`).

## The gesture inspector numbers by identity and inspects recordings by replay

`GestureInspector` numbers arena members and composition nodes from 1 in the order it first
sees them, by allocation identity and never by type name: two members of one type stay apart,
two runs over one input stream number alike, and at exhaustion nothing new is numbered. The
composition tree is built from each member's observed path. A recording holds verdict and
deadline rows but not joins or claims, so it is inspected by replaying it with an inspector
attached to the replaying host's arena. `a_recording_replays_to_the_model_the_live_run_built`
pins that the replay builds the live run's model, and `detaching_stops_observation` pins that a
detached inspector keeps its reports and receives nothing more.

## The gesture overlay rebuilds only its own layer

`GestureInspectorOverlay` stacks one layer over the app with `StackFit::Expand`, so wrapping the
root leaves the app's constraints unchanged. The overlay's state is born bound in
`create_state`. The controller notifies the layer alone, so toggling never rebuilds the app,
and the layer registers its arena observer only while shown and mounted, so a hidden overlay
costs the input path nothing. A replaced controller hands the overlay over: the old
controller's observer is released and the new one drives it. Contests are drawn beside the Down
position the overlay's `Listener` recorded for their pointer, and a settled contest fades over
`CONTEST_FADE` through post-frame rebuilds that stop once nothing fades.
Pinned by `toggling_hides_the_overlay_without_rebuilding_the_app`,
`the_overlay_observes_only_while_mounted_and_shown`,
`a_replaced_controller_hands_the_overlay_to_the_new_one` and
`the_overlay_draws_each_contest_by_its_pointer_then_fades_it`.
