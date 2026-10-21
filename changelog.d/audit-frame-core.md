### Added

- **Per-phase frame telemetry** (`flui-scheduler`, `flui-runtime`): `FrameSnapshot::phases`
  (`FramePhaseDurations`) records each segment's build, layout and paint;
  `FrameSnapshot::phase_duration` derives the submit, and `over_budget_phase` names the phase that
  made a late frame late. Its `clock_timestamp` is now the pump's frame time. `FrameClock::record_frame`
  takes the frame's stages as one `FrameTimes`.
- **`frame_pace`** (`flui-scheduler`): frame rate and frame work over a snapshot slice, ignoring
  idle gaps longer than `FRAME_RUN_GAP`.
- **`SchedulerBuilder::clock`** (`flui-scheduler`): frame durations, jank and the Idle deadline are
  measured on the clock frame times come from; a UI runtime passes its own.

### Changed

- **`FrameId` is the scheduler's own 64-bit id** (`flui-scheduler`), numbered per scheduler (every
  scheduler's first frame is frame 1) instead of a process-wide `usize` counter that panicked on
  wasm32 once it wrapped.
- **`FrameTiming` is issued by the scheduler** (`flui-scheduler`): it measures against an instant
  you pass (`elapsed_at`, `is_janky_at`, …) instead of the wall clock; `phase`,
  `phase_duration` and `FrameTimingBuilder` are removed.
- **`FrameBudget`** (`flui-scheduler`): fps parameters are `NonZeroU32`, live-frame queries take
  the current instant, `record_frame_duration` refuses non-finite and negative samples, and the raw
  `f64` recorders are removed.
- **`TaskQueue` drains per-priority FIFO lanes** (`flui-scheduler`): a task queued during a drain
  waits for the next one, so a re-queueing high-priority task cannot starve older work.

### Removed

- **`FrameClock`'s in-flight gate** (`flui-scheduler`): `set_max_in_flight`, `record_submit`,
  `record_retire` and `in_flight` were never fed; `Backpressure` now means the produce throttle.
- **`flui_runtime::performance_stats`**: the performance overlay reads the frame history instead.

### Fixed

- **Late demand reaches the next frame** (`flui-runtime`): a redraw requested after a
  presentation's segment, or a state change made in the frame's tail (a hover exit, a paint-time
  mark), no longer waits for unrelated input.
- **Work queued between frames gets a frame** (`flui-scheduler`): microtasks and tasks request
  one, a frame's run budget bounds a fan-out (a task queueing two more ran about 2^32 times in one
  frame), Idle tasks stop at the deadline one at a time, and callbacks, tasks and post-frame
  callbacks queued behind a panic re-arm the next frame.
- **A task's waker never panics** (`flui-scheduler`): a panicking frame wake hook no longer unwinds
  into the thread that completed the future; the demand stays pending.
- **One window poke per frame** (`flui-runtime`): many state changes before a frame make one
  platform redraw request.
- **Secondary windows** (`flui-runtime`): a window-state batch redraws the window it came from,
  and each presentation is laid out at its own window's size.
- **Performance overlay** (`flui-runtime`): frame time is the frame's work on the frame clock, not
  the gap between composites.
- **`Vsync::has_running`** (`flui-animation`) remembers an idle registry until a run can start, so
  repeated queries between frames scan no controller.
