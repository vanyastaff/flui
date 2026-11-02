### Changed

- **Tracing targets are `flui.<subsystem>`**: frame phase spans are under `flui.frame`,
  `element_rebuild` and the reconcile events under `flui.tree`, recognizer spans under
  `flui.input`, animation spans under `flui.animation`, signals and accessibility fallbacks under
  `flui.signals` and `flui.a11y`. The old
  spellings `flui::reconcile`, `flui_animation`, `flui::signals` and `flui::a11y` are gone from
  `RUST_LOG` directives; there is no alias.
  `flui_view::RECONCILE_TARGET` is now `"flui.tree.reconcile"`.
- **Frame spans carry primitives**: ids are `u64` and `element_rebuild` moves from INFO to TRACE
  with its reasons as a bitmask (`RebuildReasons::bits`). The `semantics`, `commit` and `submit`
  phases get DEBUG spans, relayout roots get a TRACE `layout_root` span, and the recognizer spans
  that were created but never entered are now entered TRACE spans.
- **`flui-devtools`' frame profiler recognizes a phase by target and name**, so an application
  span named `paint` is no longer counted as the paint phase.
- **`Timeline::export_chrome_trace` writes complete (`X`) events** with a process per runtime,
  thread names and, for a timeline over a capture, `frame_ms` and `jank` counters, in place of
  begin/end pairs on a fixed process.
- `TimelineEvent` gained `runtime` and `presentation`.

### Added

- **`flui-devtools` has one capture of the frame spans**: `Capture` and `CaptureLayer` record the
  frame, each presentation's segment and every `Phase`, partitioned by the runtime that drove the
  frame and the presentation a span ran for. `FrameTimingLayer::over(&capture, profiler)` and
  `Timeline::over(&capture)` read it instead of recording on their own. `Phase::of` is the one
  `(target, name)` table. `PresentationId::from_u64` and `GenId::from_u64` read an id back from a
  trace field.
- The scheduler's `frame` span carries `runtime`, and the runtime opens a `presentation` span
  around each presentation's segment of a frame.
- `Capture::on_frame` hands its sink the capture, so a sink never has to own a handle to the capture
  that owns it, and a sink that panics no longer takes the frame from the sinks after it. The
  runtime's presentation span now also covers a presentation's animation sampling, `submit` and
  `commit`, and the trace export writes a `presentation` argument on events that ran for one.
- Several `Capture`s can watch one registry, a nested frame's work no longer takes the enclosing
  frame's presentation, and the trace export writes `frame_ms` and `jank` counters for a frame
  that reported no display period (judged against 60 Hz).
- `OwnerFrame::set_trace_runtime` names the runtime a `frame` span reports from the moment it
  opens, so a frame whose callback panics still says whose frame it was.
  `FrameTimingLayer::over_runtime` profiles a named runtime, one pair per runtime, and a phase
  measured by hand with `Profiler::profile_phase` keeps its offset inside a captured frame.
