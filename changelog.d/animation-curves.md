### Changed

- **`flui-animation`**: curve parameters are validated however a curve is made. `Cubic`,
  `ThreePointCubic`, `Interval` and the elastic curves have private fields, a `const fn new` that
  panics on invalid input (a compile error in a `const`), a `try_new` returning the new
  `CurveError`, and serde decoding that rejects what `try_new` rejects; the serialized field
  names are unchanged. `Cubic::new` requires `x1` and `x2` in `[0, 1]` (CSS Easing 2) and finite
  `y1`, `y2`; `ElasticInCurve::new(0.0)` and other non-positive periods are refused instead of
  producing NaN. `Split` reports an invalid split as a `CurveError` message.
- **`flui-animation`**: one input policy for every curve, stated on the `Curve` trait:
  `transform(NaN)` returns NaN (`Cubic`, `ThreePointCubic`, `Interval` and `Split` returned 0 or
  1), `t` outside `[0, 1]` clamps to the nearest end, and the ends are exact.
- **`flui-animation`**: the cubic-bezier solver bounds its output error below `1e-7`; it bounded
  only the x residual before, and `Curves::EaseInOutExpo` was off by up to `9e-3` next to its
  vertical tangent. The elastic curves no longer jump by `2^-10` onto their end values.

### Removed

- **`flui-animation`**: `ReverseCurve` and `Curve::reversed` (they mapped 0 to 1, breaking the
  curve contract): reverse the driving animation (`ReverseAnimation`, `AnimationExt::reversed`)
  or use `.flipped()` to turn an ease-in into an ease-out. `SawTooth` and `Threshold`: use
  `Interval::linear(t, t)` for a step at `t`.
