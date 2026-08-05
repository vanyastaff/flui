# Changelog

All notable changes to `flui-scheduler` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`Scheduler::set_on_frame_scheduled`** — platform wake hook fired on the
  `frame_scheduled` false→true transition (Flutter parity:
  `SchedulerBinding.scheduleFrame` → `platformDispatcher.scheduleFrame`).
  `request_frame`, `schedule_frame` and `schedule_frame_callback` now route
  through the transition so registering an animation ticker actually wakes an
  idle event loop; previously they only set an atomic flag nobody read while
  the platform slept, and animations starved after the first frame.
- **Safe conversion methods**:
  - `SchedulerPhase::try_from_u8()` - fallible conversion from u8
  - `AppLifecycleState::try_from_u8()` - fallible conversion from u8
  - `FrameSkipPolicy::try_from_u8()` - fallible conversion from u8

### Changed

- **`Ticker::start`, `Ticker::start_default`, and `Ticker::start_typed`**
  now return a `TickerFuture` for the active run. `Ticker::stop()` completes
  that future normally; `Ticker::dispose()`, `Drop`, and `reset()` cancel it.
  A second `start` while active no longer mutates the active run in release
  builds (debug builds already assert).
- **`TickerFuture::when_complete_or_cancel`** now uses `event-listener` crate instead of busy-wait thread spawn, eliminating potential resource leaks
- **`SchedulerBindingState`** is now per-`Scheduler` instance instead of global static, enabling proper test isolation and multiple scheduler support

### Breaking (issue #556 — `UpdateScheduler` reshape)

- **`Scheduler` → `UpdateScheduler`, `WeakScheduler` → `WeakUpdateScheduler`**
  — hard rename, no alias, workspace-wide.
- **`UpdateScheduler::drive_frame`** gained a `deadline: IdleDeadline`
  parameter: `drive_frame(vsync_time, deadline, pipeline)`. `IdleDeadline`
  is a newtype over `Instant` — not a bare second `Instant` — so it cannot
  be silently swapped with `vsync_time` at a call site. `deadline` bounds
  `Priority::Idle` task execution alone — `Priority::Animation` and
  `Priority::Build` always run to completion regardless of it. A deadline
  can defer low-priority work; it can never skip a frame or starve logical
  work. A panicking task never leaks a stale deadline into a later frame
  (an RAII guard clears it, including during an unwind).
- **`UpdateScheduler::budget()`** (a `parking_lot::MutexGuard<FrameBudget>`
  accessor) is replaced by **`UpdateScheduler::budget_snapshot()`**, which
  returns an owned `FrameBudget` value (now `Clone`) instead of a live lock
  guard.
- **`UpdateScheduler::new()`** no longer defaults its internal frame-budget
  stats via a named `FrameDuration::FPS_60` constant — that constant is
  deleted (`flui-scheduler` ships no fixed frame-rate default anywhere).
  `UpdateScheduler` carries no `frame_duration`/`target_fps` field or
  accessor at all: `set_target_fps`, `target_fps`, `set_frame_duration`,
  `frame_duration`, `with_target_fps`, and `with_frame_duration` are all
  deleted. `SchedulerBuilder::target_fps`/`frame_duration` still configure
  the internal stats budget's label (read back only through
  `budget_snapshot()`), never a scheduler-wide rate.
- **`FrameSkipPolicy`, `UpdateScheduler::{set_frame_skip_policy,
  frame_skip_policy, set_max_frame_skip, max_frame_skip,
  skipped_frame_count, should_skip_frames, check_and_skip_frame,
  clear_skip_stats, skip_rate}` are deleted.** `should_skip_frames`/
  `check_and_skip_frame` made a live frame-skip decision off the deleted
  fixed-rate assumption — dead surface with zero production consumers
  (tests only, no strategy ever installed).
- **`SchedulingStrategy`, `default_scheduling_strategy` are deleted**
  (`config.rs`) for the same reason: zero production consumers, and
  `default_scheduling_strategy` read `is_over_budget()` to make exactly the
  kind of live gating decision `UpdateScheduler` no longer makes.
- **`VsyncScheduler`, `VsyncMode`, `VsyncStats`, `VsyncCallback`, and
  `Scheduler::{set_vsync, has_vsync}`/`SchedulerBuilder::vsync_refresh_rate`
  are deleted.** This was a fixed-rate vsync *simulator* with zero
  production consumers; real frame pacing lives in the blocking Fifo
  present (ADR-0029), and per-presentation physical pacing is a future
  `FrameClock` (a later #556 slice), not this crate.

### Fixed

- `TickerFuture` is now wired to the `Ticker` lifecycle instead of existing as
  an unconnected async helper.
- Thread spawn leak in `TickerFuture::when_complete_or_cancel` - threads no longer spin in a busy loop
- Test interference caused by global `BINDING_STATE` - each `Scheduler` now has isolated binding state

### Documentation

- Updated ticker documentation to describe the single canonical `Ticker`
  lifecycle and removed stale typestate/prelude examples from the public README.
- Added cross-references to `try_from_u8` in `from_u8` panic documentation

### Dependencies

- Added `event-listener = "5.3"` for efficient async event notification

---

## [0.1.1] - Previous Unreleased

### Added

- **Type-system features**:
  - Type-safe duration wrappers (`Milliseconds`, `Seconds`, `Microseconds`, `Percentage`)
  - Type-safe IDs with marker traits (`TypedId<M>`, `FrameId`, `TaskId`, `TickerId`)
  - `FrameDuration` for frame budget calculations

- **Collection traits for `TickerGroup`**:
  - `iter()` and `iter_mut()` methods
  - `IntoIterator` implementation for owned, shared, and mutable references
  - `FromIterator<Ticker>` for collecting into `TickerGroup`
  - `Extend<Ticker>` for adding multiple tickers

- **Builder patterns**:
  - `FrameBudgetBuilder` for constructing `FrameBudget`
  - `FrameTimingBuilder` for constructing `FrameTiming`
  - `SchedulerBuilder` for constructing `Scheduler`

- **Conversion traits**:
  - `From<Microseconds>` for `Milliseconds`
  - `From<Seconds>` for `std::time::Duration`
  - `From<f64>` for `Seconds`
  - `From<i64>` for `Microseconds`

- **Optional `serde` feature**:
  - Serialization support for `Milliseconds`, `Seconds`, `Microseconds`, `Percentage`
  - Serialization support for `Priority`, `PriorityCount`, `FramePhase`
  - Serialization support for `BudgetPolicy`, `PhaseStats`, `AllPhaseStats`
  - Serialization support for `VsyncMode`, `VsyncStats`, `TickerState`

- **Documentation improvements**:
  - `# Panics` sections for functions that may panic
  - `# Examples` for all major types
  - Sealed trait documentation
  - Hyperlinks between related types

### Changed

- Input validation added to `FrameDuration::from_fps()` (panics if fps is 0)
- Input validation added to `VsyncScheduler::new()` (panics if refresh_rate is 0)

## [0.1.0] - 2024-XX-XX

### Added

- Initial release with core scheduling functionality
- `Scheduler` for frame orchestration
- `TaskQueue` with priority-based execution
- `Ticker` and `TickerProvider` for animation coordination
- `FrameBudget` for time management
- `VsyncScheduler` for vsync synchronization
