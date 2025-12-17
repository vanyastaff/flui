# Changelog

All notable changes to `flui-scheduler` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Safe conversion methods**:
  - `SchedulerPhase::try_from_u8()` - fallible conversion from u8
  - `AppLifecycleState::try_from_u8()` - fallible conversion from u8
  - `FrameSkipPolicy::try_from_u8()` - fallible conversion from u8

### Changed

- **`TickerFuture::when_complete_or_cancel`** now uses `event-listener` crate instead of busy-wait thread spawn, eliminating potential resource leaks
- **`SchedulerBindingState`** is now per-`Scheduler` instance instead of global static, enabling proper test isolation and multiple scheduler support
- **`TickerProvider::schedule_tick`** now passes `0.0` as elapsed time, matching Flutter semantics where individual tickers track their own start times

### Fixed

- Thread spawn leak in `TickerFuture::when_complete_or_cancel` - threads no longer spin in a busy loop
- Test interference caused by global `BINDING_STATE` - each `Scheduler` now has isolated binding state
- Confusing elapsed time semantics in `TickerProvider::schedule_tick`

### Documentation

- Added "Why ScheduledTicker doesn't implement Clone" section explaining design rationale
- Updated `TickerProvider::schedule_tick` documentation to clarify elapsed time semantics
- Added cross-references to `try_from_u8` in `from_u8` panic documentation

### Dependencies

- Added `event-listener = "5.3"` for efficient async event notification

---

## [0.1.1] - Previous Unreleased

### Added

- **Advanced type system features**:
  - Typestate pattern for tickers (`TypestateTicker<Idle>`, `TypestateTicker<Active>`, etc.)
  - Type-safe duration wrappers (`Milliseconds`, `Seconds`, `Microseconds`, `Percentage`)
  - Type-safe IDs with marker traits (`TypedId<M>`, `FrameId`, `TaskId`, `TickerId`)
  - Typed tasks with compile-time priority checking (`TypedTask<P>`)
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

- **Extension traits**:
  - `ToMilliseconds` and `ToSeconds` for duration conversions
  - `FrameBudgetExt` for additional frame budget operations
  - `FrameTimingExt` for frame timing utilities
  - `PriorityExt` for priority operations

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
