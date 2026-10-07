### Added

- **`flui-animation`**: `MotionClock`, a per-presentation animation clock that maps raw frame
  time to a monotonic, finite `AnimationTime` with a `PlaybackRate` (rebased on a rate change, so
  time never jumps) and exact `step`s; it alone mints a `FrameTick`.
- **`flui-testing`**: `HeadlessBinding::motion_clock_mut` sets the rate of, or steps, the
  animation time `pump_frame` ticks its `Vsync` with.

### Fixed

- **`flui-runtime`**: each presentation ticks its `Vsync` through its own `MotionClock`, so a
  frame time that runs backwards, or a non-finite or negative test override, holds animations
  instead of freezing a run or moving it backwards.
