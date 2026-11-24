### Changed

- `flui_devtools::Profiler` reads a `Capture`. Install `capture.layer()` on your subscriber and make the profiler with `Profiler::over(&capture, ProfilerConfig::default())`; the same capture can feed a `Timeline::over(&capture)`. `ProfilerConfig::runtime` names the UI runtime to profile, and `None` follows the first runtime whose frame names itself.
- `Profiler::frame_rate()` replaces `Profiler::average_fps()` and `FrameStats::fps`: it is the number of frames the scheduler produced per second over the retained history, from the frames' start times. The old number divided a second by each frame's work time, so a 2 ms frame on a 60 Hz display reported 500 FPS. `None` with fewer than two frames.
- `JankLine::Fixed` takes a `Duration` (`JankLine::Fixed(Duration::from_millis(10))`) instead of milliseconds as `f64`, so the line can no longer be NaN or negative.
- Profiler phases are `flui_devtools::Phase`: `FrameStats::phase(Phase::Compositing)` and `phase_duration_ms(Phase)` read any of the ten captured phases, and `PhaseTiming::phase()` returns a `Phase`.
- `FrameStats` and `PhaseTiming` fields are private: read them with `number()`, `start()`, `duration()` (was `total_time`), `duration_ms()` (was `total_time_ms()`), `phases()`, `phase()`, `is_jank()`, and `PhaseTiming::phase()`, `duration()`, `start_offset()`. `FrameStats` implements `Display` as a one-line frame summary followed by one line per phase.
- A `TimelineEvent` carries `kind: TimelineEventKind` (`Frame`, `Segment`, `Phase(Phase)`, `Custom`) instead of `category: EventCategory`. `Timeline::record_event(name)`, `record_instant(name)` and `record_completed_event(name, start, duration, args)` drop the category argument and record `Custom` events; `Timeline::events_of` replaces `get_events_by_category`. The Chrome trace's `cat` and `args.category` are the kind's label: `Frame`, `Segment`, `Custom`, or the phase's own label (`Layout`, `Animation tick`). `export_json` writes `kind` (`"Custom"`, `{"Phase": "Layout"}`) where it wrote `category`.

### Removed

- `FrameTimingLayer` (`new`, `over`, `over_runtime`): install `Capture::layer()` and use `Profiler::over`, with `ProfilerConfig::runtime` for one runtime.
- `FramePhase`: use `Phase`. Time application work with `Timeline::record_event` instead of a custom profiler phase.
- The manual frame API: `Profiler::begin_frame`, `end_frame`, `set_frame_period`, `profile_phase` and `PhaseGuard`. The profiler records the frames the framework drives.
- `Profiler::new`, `Profiler::with_config` and `Default for Profiler`: `Profiler::over` is the only constructor.
- `Profiler::print_frame_summary`: format a `FrameStats` with `{}` instead.
- `EventCategory`, with its `name` and `color`: use `TimelineEventKind` and its `label`.

### Fixed

- A frame whose span reported a zero display period when it opened was judged against a zero jank line, so every such frame was jank. A zero period now counts as none, as it already did when recorded later, and the frame is judged against 60 Hz.
