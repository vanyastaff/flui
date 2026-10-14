# flui-devtools Architecture

The profiling, timeline and inspector modules observe through the SDK;
the optional development agent serves its host hook through a local endpoint.
The feature and production wiring map is in [README.md](README.md).

## Mapping decisions

### Animation work belongs to its nearest scheduler frame

Optional animation spans arrive through SDK-enabled tracing instrumentation,
without a framework-to-devtools dependency (ADR-0202). Tick, sampling and
listener timings are inclusive extents. Reentrant spans in the same phase do
not add a second duration for overlapping work. Animation span names are
accepted only from their declared target.

Scheduler frames are recognized by their producer target and span identity;
an application's same-named span is not a runtime boundary. All phases
and scheduler frames finish at their outermost exit, with depth accounting for
same-span reentry. A source retaining its current span also retains its parent
frame identity, but neither lifetime can delay timing into a later frame.
The application-frame and retained-sampling rows in
`developer_history_respects_capacity_and_clear` pin both boundaries through real
controller work, including late entry and release in the next idle frame.
Completed phase spans retain a closed marker until final destruction.
The same-frame closed-sampling row independently measures the producer's first
visit and reenters its span inside a real listener: that listener remains listener
work and cannot restart the earlier sampling phase.
The retained-rendering row drives actual build/layout/compositing/paint spans,
retains them through a frame boundary and releases them during a later idle
frame. All original phases are recorded before their frame ends, and their
late retirement cannot invent paint work in the idle frame.

The layer owns one scheduler frame by span identity. A nested foreign frame
and its phases are dropped whole; merely finding the owning frame somewhere
among a phase's ancestors would blend two runtimes. The nested-runtime row in
`developer_history_respects_capacity_and_clear` pumps an actual animation in
that foreign frame and checks the idle owning frame contains no sampling or
listener work. `a_real_animation_reaches_the_profiler_with_its_work` covers
time, spring and repeat producers, paint, successful completion and idle silence.

### Clearing history invalidates outstanding event guards

Timeline guards identify an event independently of its current buffer position.
Clearing the timeline advances the retained buffer's identity base to the next
issued event ID. A guard created before clear can therefore neither change a
new event nor prevent a new guard from recording its own duration.
`developer_history_respects_capacity_and_clear` exercises both directions
through the public API (with profiling and timeline enabled).

### Zero profiler history capacity retains only aggregate counters

`ProfilerConfig::max_frame_history` is a maximum, including zero. At zero,
completed frames still contribute to aggregate frame and jank counters, while
`frame_history` is empty and `frame_stats` returns `None`. The zero-capacity
row in `developer_history_respects_capacity_and_clear` pins both retention and
counter progress. Phase values are owned data without user destructors.

### Timeline admission evaluates consumer conversions outside the mutex

Event names accept consumer-defined `Into<String>` implementations. Conversion
runs before locking the timeline, so it may inspect or clear the same timeline.
The three admission paths use this ordering; isolated rows in
`developer_history_respects_capacity_and_clear` exercise each public method and
then admit another event. A child deadline distinguishes the old mutex deadlock
without blocking the remaining family rows. Converted names own plain strings.

### Timeline durations preserve their microsecond range

`TimelineEvent::duration` reconstructs seconds and subsecond nanoseconds from
its public `u128` microseconds instead of narrowing the entire value to `u64`.
Representable durations remain exact at microsecond precision; values beyond
`Duration::MAX` saturate. The large-duration row in
`developer_history_respects_capacity_and_clear` records a completed event through
the public timeline, reads its duration back, and exercises saturation of a
consumer-modified event value.

### The gesture inspector names members by number and inspects recordings by replay

`GestureInspector` numbers arena members and composition nodes in the order
it first sees them, by allocation identity, never by type name: two members
of one type stay apart, and two runs over one input stream number alike. It
builds the composition tree from each member's observed path, whose nodes
carry identity, so the arena keeps no downward child list for it. A recording
holds verdict and deadline rows but not joins or claims, so a recording is
inspected by replaying it with an inspector attached to the replaying host's
arena. `a_recording_replays_to_the_model_the_live_run_built` pins that the
replay builds the live run's model; `detaching_stops_observation` pins that a
detached inspector keeps its reports and receives nothing more.

### The gesture overlay rebuilds only its own layer and observes only while shown

`GestureInspectorOverlay` stacks a layer over the app with `StackFit::Expand`,
so wrapping the root leaves the app's constraints unchanged. The controller
notifies the layer alone: toggling never rebuilds the app, and the layer
registers its arena observer only while shown and mounted, so a hidden
overlay costs the input path nothing. Contests are drawn beside the Down
position the overlay's translucent `Listener` recorded for their pointer, and
a settled contest fades over `FADE` through post-frame rebuilds that stop
once nothing fades. `toggling_hides_the_overlay_without_rebuilding_the_app`,
`the_overlay_observes_only_while_mounted_and_shown` and
`the_overlay_draws_each_contest_by_its_pointer_then_fades_it` pin these.
