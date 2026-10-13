### Added

- **Expanded hit areas** (`flui-widgets`, `flui-objects`, `flui-rendering`): `HitSlop` makes its
  child hittable beyond its bounds — grown by `slop` insets and to at least `min_target`, which
  defaults to the gesture settings' preset (44 px for iOS touch, 24 px for a desktop mouse).
  Competing areas resolve in order: a visual hit on another target, the nearest edge, then the
  topmost; an area never takes a tap that landed on content in another composited layer or on a
  pointer barrier (`AbsorbPointer`, the modal barrier). The default `min_target` follows live
  host settings updates without a rebuild. Backed by `RenderHitSlop` and the new
  `RenderBox::hit_slop_area` and `RenderBox::blocks_hit_slop` hooks, which
  `PipelineOwner::hit_test` resolves after the visual walk; `GestureSettingsProvider::min_target`
  reads the live value.
- **Shape hit-testing** (`flui-widgets`, `flui-objects`): `ContentShape` hit-tests its child by a
  `HitShape` (`Rect`, `RoundedRect`, `Oval`, `Path`) instead of its box, without clipping paint.
  Backed by `RenderContentShape`.
