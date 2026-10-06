### Added

- `MotionSpec` (`Curve { duration, curve }` or `Spring(..)`) and
  `AnimatedValue::with_motion`, `set_motion` and `velocity`: an interrupted
  `AnimatedValue` keeps its value and velocity in both spring and curve mode,
  a curve segment still lands exactly on time, and reversing a curve shortens
  it by the eased fraction already travelled (CSS Transitions).

### Fixed

- A dismissed `Dismissible` card leaves at the finger's speed on any width;
  the fling velocity was a fixed 1/300 of the px/s speed.
- A flung back gesture settles from the finger's speed instead of a fixed
  350 ms curve.
- `AnimatedValue::advance` ignores zero, negative and non-finite steps.
