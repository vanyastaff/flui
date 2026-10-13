### Changed

- Animation items use canonical crate-root imports and a common `animation::prelude`, also available through `flui_sdk::animation::prelude`. Material consumers use it directly, with no SDK wrappers or duplicate types.
- Curve construction errors identify arguments with `CurveParameter`; three-point normalization failures report `CurveError::RescalingOutOfRange` separately from out-of-range input.
- `TweenAnimation` accepts mappings and output values without `Debug` implementations; its mapping only needs `Clone` when cloning the observer itself.
- Curved, proxy and constant observations accept curves/output values without `Debug`; an owned curve needs `Clone` only when cloning its curved observer.

### Removed

- Animation implementation module paths, the public `AnimationDirection`, `AnimatableExt` and the free `animate` helper. Import canonical items from the root; use `Animatable::animate` or `TweenAnimation::new`.
- Per-type aliases `FloatTween`, `ColorTween`, `SizeTween`, `RectTween`, `OffsetTween`, `AlignmentTween`, `EdgeInsetsTween`, `BorderRadiusTween` and `Matrix4Tween`; use `Tween<T>` instead. Integers use `IntTween` with an explicit `Rounding`.
- Redundant constant-animation phase constructors; use `ConstantAnimation::with_status`.
- Unused `ConstantTween`, `ReverseTween` and `CurveTween`. Fixed observations use `ConstantAnimation`, current reversed endpoint mappings use `Tween`, and live easing uses `CurvedAnimation`; these are migration choices for existing consumers, not equivalent replacements for every custom mapping.

### Fixed

- Constant animations return an inert listener token outside core notifiers' allocated namespace, preventing accidental withdrawal of a live callback and removing their process-global counter.
