### Added

- **Expanded hit areas** (`flui-widgets`, `flui-objects`, `flui-rendering`): `HitSlop` makes its
  child hittable beyond its bounds — grown by `slop` insets and to at least `min_target`, which
  defaults to the gesture settings' preset (44 px for iOS touch, 24 px for a desktop mouse).
  Competing areas resolve in order: a visual hit on another target, the nearest edge, then the
  topmost; an area never leaves its composited layer. Backed by `RenderHitSlop` and the new
  `RenderBox::hit_slop_area` hook, which `PipelineOwner::hit_test` resolves after the visual walk.
- **Shape hit-testing** (`flui-widgets`, `flui-objects`): `ContentShape` hit-tests its child by a
  `HitShape` (`Rect`, `RoundedRect`, `Oval`, `Path`) instead of its box, without clipping paint.
  Backed by `RenderContentShape`.
