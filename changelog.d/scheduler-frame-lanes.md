### Changed

- **`flui-scheduler`**: a frame has three phases, `SchedulerPhase::{Idle, Frame, PostFrame}`
  (ADR-0368): `Frame` polls the owner's ready async tasks once, then runs the pipeline, and
  `PostFrame` runs the post-frame callbacks. The frame drive is
  `DriveFrame::drive_frame(timestamp, prepare, pipeline)`, with no idle deadline.
- **`flui-runtime`**: `FrameClockSource` no longer supplies an idle deadline; a frame request's
  `target_present` and presentation report are recorded with the frame's input, and no step of the
  frame reads them.

### Removed

- **`flui-scheduler`**: the frame-callback lanes nothing filled: `schedule_frame_callback`,
  `cancel_frame_callback`, `add_persistent_frame_callback`, `OneShotFrameCallback`,
  `RecurringFrameCallback`, `schedule_microtask`, the priority task queue (`add_task`, `Priority`,
  `Task`, `TaskQueue`, `PriorityCount`, `MAX_BUILD_REENTRY_PASSES`, `SchedulerBuilder::task_queue_capacity`),
  the idle deadline (`IdleDeadline`), the timings lane (`add_timings_callback`,
  `remove_timings_callback`, `report_timings`, `time_since_last_timings_report`,
  `TimingsCallback`), `set_frames_enabled` (the lifecycle state alone enables frames),
  `should_schedule_frame`, `schedule_frame_if_enabled`, `schedule_forced_frame`,
  `current_system_frame_time_stamp`, `current_vsync_time`, `transient_callback_count`,
  `debug_assert_no_transient_callbacks`, `UpdateScheduler::is_in_frame` (`phase().is_in_frame()`
  answers it), and `FrameClock::record_presentation` and `next_presentation_after`. Animation is a
  `Vsync` controller of the runtime, deferred work an async task (`AsyncDriver::spawn_local`), and
  per-frame cost `FrameClock`'s `FrameSnapshot`.
- **`flui-foundation`**: `TaskId`, which only the priority task queue minted.
