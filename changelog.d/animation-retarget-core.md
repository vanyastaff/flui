### Added

- `MotionSpec` (`Curve { duration, curve }` or `Spring(..)`) and
  `AnimatedValue::with_motion`, `set_motion` and `velocity`: an interrupted
  `AnimatedValue` keeps its value and velocity in both spring and curve mode,
  a curve segment still lands exactly on time and at rest, retargeting to the
  current target keeps a curve on schedule, and reversing a curve shortens
  it by the eased fraction already travelled (CSS Transitions).

### Fixed

- A dismissed `Dismissible` card leaves at the finger's speed on any width
  under tight constraints (at the drag's speed under loose ones);
  the fling velocity was a fixed 1/300 of the px/s speed.
- A flung back gesture settles from the finger's speed instead of a fixed
  350 ms curve.
- `AnimatedValue::advance` ignores zero, negative and non-finite steps, and
  steps whose sum overflows saturate instead of publishing NaN.
