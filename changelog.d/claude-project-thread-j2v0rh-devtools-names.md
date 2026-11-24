### Changed

- `Timeline::get_events` is `Timeline::events`, and `Timeline::get_events_by_kind` is `Timeline::events_of`.
- `Timeline::get_events_in_range(start_ms, end_ms)` is `Timeline::events_between(start, end)`, which takes `Duration`s measured from the timeline's start.
- `Timeline::with_capacity(max_events)` is `Timeline::bounded(max_events)`: the number is a retention bound, and nothing is allocated up front.
- `Capture::with_capacity(max_frames, max_spans)` is `Capture::bounded(CaptureLimits { frames, spans })`; `CaptureLimits::default()` is 600 frames and 20,000 spans, what `Capture::new` keeps.
- `GestureInspector::with_capacity(arena, capacity)` is `GestureInspector::attach_bounded(arena, max_contests)`, and `DEFAULT_CONTEST_CAPACITY` is `DEFAULT_MAX_CONTESTS`.
- `PhaseInfo` is `PhaseTiming`.
- `Timeline::event_count` and the filtered readers (`events_of`, `events_between`) no longer copy every event: they count or filter first and copy only what they return. A `Timeline` with a bound drops its oldest event without shifting the others, and a `Profiler` no longer allocates its whole frame history when it is created.

### Removed

- `Timeline::print_summary`: a library does not print to stdout; read `Timeline::events_of` or the exported trace.
