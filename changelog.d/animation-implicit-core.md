### Added

- `MotionSpec::with_duration`, `MotionSpec::with_curve`, `MotionSpec::DEFAULT_DURATION` and `Default for MotionSpec` (200 ms ease-in-out) state the duration/curve merge rule once.
- `MotionBuilder` (in the `flui_widgets` prelude) gives every implicit widget the same `duration`, `curve`, `spring` and new `motion` builders.

### Changed

- `AnimatedOpacity`, `AnimatedPadding`, `AnimatedRotation`, `AnimatedAlign` and `AnimatedContainer` take `duration`/`curve`/`spring` from `MotionBuilder` instead of inherent methods: import `flui_widgets::MotionBuilder` or the prelude.
