# Changelog

All notable changes to `flui-scheduler` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
