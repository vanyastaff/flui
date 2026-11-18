### Changed

- **`flui-scheduler`**: frames are labelled with the display's refresh period through
  `UpdateScheduler::set_display_period(Duration)` (read back with `display_period`); the `frame`
  span carries it as `period_us`, as before.

### Removed

- **`flui-scheduler`**: the stats-only frame budget and its unit types: `FrameBudget`,
  `FrameBudgetBuilder`, `SharedBudget`, `BudgetPolicy`, `PhaseStats`, `AllPhaseStats`, the `budget`
  and `duration` modules (`FrameDuration`, `Milliseconds`, `Microseconds`, `Seconds`, `BudgetPercentage`,
  `InvalidDurationConfig`), the `prelude`, `UpdateScheduler`'s budget and jank queries
  (`is_over_budget`, `is_deadline_near`, `remaining_budget`, `remaining_budget_ms`,
  `budget_snapshot`, `avg_fps`, `set_budget_frame_duration`, `is_janky`, `janky_frame_count`,
  `jank_rate`, `clear_jank_stats`) and `SchedulerBuilder::target_fps`/`frame_duration`. Nothing
  read them to decide anything; per-phase durations of a frame come from `FrameClock`'s
  `FrameSnapshot`, and a profiler judges a late frame against the span's `period_us`.
- **`flui-scheduler`**: `FrameTiming`'s budget fields and queries (it keeps `id` and
  `start_time`); `SchedulerPhase::next`, `is_animating`, `is_rendering`, `can_transition_to`, `try_from_u8` and
  `TryFrom<u8>`; `FramePhase::next`, `prev`, `is_rendering`, `as_index` and `TryFrom<u8>`;
  `AppLifecycleState::can_transition_to` (it allowed every transition), `description`,
  `should_animate`, `can_animate`, `should_save_state`, `should_release_resources`,
  `should_render` (the same answer as `is_visible`), `try_from_u8` and `TryFrom<u8>`.
- **`flui-view`**: the re-exports of the scheduler's duration types.
