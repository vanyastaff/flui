### Changed

- **`AnimationController::repeat` takes a `RepeatSpec`** (`flui-animation`): `repeat(reverse: bool)`
  and `repeat_with(min, max, reverse, period, count)` become one `repeat(impl Into<RepeatSpec>)`.
  A `RepeatMode` alone repeats forever over the controller's bounds (`repeat(RepeatMode::Restart)`,
  `repeat(RepeatMode::Alternate)` for the former `repeat(false)`/`repeat(true)`); a
  `RepeatSpec::new(mode)` adds `with_range(ValueRange)`, `with_period(Duration)` and
  `with_iterations(Iterations::Finite(n) | Iterations::Infinite)`. The range is a validated
  `ValueRange`, so a NaN, equal or span-overflowing pair is refused when the range is built;
  `InvalidRepeatRange` now reports only a range that collapses once clamped into the controller's
  bounds, or a range-less repeat on an unbounded controller.
