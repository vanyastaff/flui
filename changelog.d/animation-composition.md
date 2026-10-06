### Added

- **`flui-animation`**: `Keyframes<T>`, a value as a pure function of time: segments timed by
  `Duration` and laid end to end (`to` with the curve that eases into the value, `cubic` on a
  Catmull-Rom spline through the keyframe times, `hold`, `jump`), exact values at boundaries,
  no published non-finite sample, typed `KeyframesError`s from `build`, clamped (`value_at`),
  looped (`value_at_looped`) and progress (`Animatable`) reads. `Stagger` gives per-index delays
  with a `First`/`Last`/`Center`/`Index` origin.
- **`flui-animation`**: `Steps` and `JumpAt`, CSS `steps()`; `Curve::slope`, the curve's
  derivative.
- **`flui-widgets`**: `ActivityIndicator`, an indeterminate circular indicator.
  `RefreshIndicator` now shows it while refreshing instead of a static coloured bar.
- **`flui-material`**: `LinearProgressIndicator`, determinate or indeterminate.
- **`flui-cupertino`**: `CupertinoActivityIndicator`.
- **`flui-testing`**: `LaidOut::draw_ops`, the last scene's draw operations.

### Changed

- **`flui-animation`**: `#[derive(Animatable)]` is `#[derive(TwoWayConverter)]` and also derives
  `Lerp`, so a derived type can be tweened and used as a keyframe value. `Lerp` is re-exported.

### Removed

- **`flui-animation`**: `TweenSequence` and `TweenSequenceItem`: build a `Keyframes` track.
  `CatmullRomCurve`, `CatmullRomSpline`, `Curve2D`, `Curve2DSample` and `ParametricCurve`
  (the spline ignored its points' x): use `cubic` keyframe segments.
- **`flui-animation`**: `AnimationExt` and `CurveExt`; `AnimatableExt` keeps `animate` only. Use
  `CurvedAnimation::new`, `ReverseAnimation::new`, `CurveTween`, `ChainedTween` and
  `ReverseTween::new`.
