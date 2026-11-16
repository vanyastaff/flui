### Added

- **`flui-runtime`**: `FrameSink::in_flight_limit`, how many of a presentation's frames a sink
  may hold unsettled (one by default, two for a threaded raster side). A presentation at the
  limit produces no frame and keeps its demand until a settlement frees a place, which wakes the
  loop; while frames are in flight `UiRuntime::next_wake` stays within 50 ms
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md) §5).
  `ScriptedSink::holding` gives the testing sink a limit; without it the sink holds any number.

### Changed

- **`flui-scheduler`**: `FrameClock::poll` takes the presentation's `FramesInFlight` and answers
  `Skip(Backpressure)` while the sink holds as many frames as it may; `FramesInFlight::NONE`
  keeps the old decision.
