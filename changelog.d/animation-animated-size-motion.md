### Changed

- `AnimatedSize::new()` takes no duration: its motion comes from `MotionBuilder` (`duration`, `curve`, `spring`, `motion`) and defaults to a critically damped spring of stiffness 400 instead of a linear curve. `reverse_duration` is removed; it never affected a resize.
- `RenderAnimatedSize::new` takes a `MotionSpec`, and `set_curve`/`set_duration`/`set_reverse_duration` become `set_motion`.
- `AnimatedCrossFade` resizes with the same motion as its fade.
