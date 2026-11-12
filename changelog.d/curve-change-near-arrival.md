### Fixed

- `AnimatedValue::set_motion` (and `animate_to`/`retarget` with an unchanged target) that changes
  only the curve less than a millisecond before the segment ends runs the remaining time instead
  of jumping to the target, so value and velocity stay continuous.
- `Cubic::slope(1.0)` of a curve whose second control point is the end point (`curves::EASE_IN`,
  `cubic-bezier(x1, y1, 1, 1)`) is the limit slope instead of zero, so an ease-in segment arrives
  at rest instead of stopping from full speed.
