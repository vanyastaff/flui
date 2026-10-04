# flui-devtools Architecture

The profiling, timeline and inspector modules observe through the SDK;
the optional development agent serves its host hook through a local endpoint.
The feature and production wiring map is in [README.md](README.md).

## Mapping decisions

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
