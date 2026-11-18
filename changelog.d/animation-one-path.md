### Changed

- **`flui-animation`**: each public item has one path, the crate root. The modules `curve` and
  `simulation` are private and the `prelude` module is gone: import `flui_animation::Cubic`,
  `flui_animation::SpringSimulation` and the rest from the root (`flui::animation::…`,
  `flui_sdk::animation::…`). `curves` stays public as the namespace of the easing presets. `Lerp`
  is named from `flui_foundation::geometry` (`flui::geometry::Lerp`), which owns it;
  `#[derive(TwoWayConverter)]` resolves it there.
- **`flui-animation`**: `AnimationController::forward_from` and `reverse_from` take the starting
  value as `f64`, and `fling_with` takes a `SpringDescription`; the `None` each accepted meant
  `forward`, `reverse` and `fling`.
- **`flui-animation`**: `IntTween::with_rounding` is `IntTween::rounding` and
  `CurvedAnimation::with_reverse_curve` is `CurvedAnimation::reverse_curve`.
- **`flui-animation`**: `AnimatedValue::with_debug_label(label, initial, motion, vsync)` is
  `AnimatedValue::builder(initial, motion).debug_label(label).build_on(vsync)`, the shape
  `AnimationController::builder` has.
- **`flui-animation`**: `AnimationStatus` is exhaustive: a `match` over its four phases needs no
  `_` arm.
