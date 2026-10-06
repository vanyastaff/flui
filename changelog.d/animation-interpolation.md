### Changed

- **`Color::lerp`** (`flui-painting`), and so `Tween<Color>`/`ColorTween` and every colour lerp in
  borders, shadows, decorations and gradients, interpolates in Oklab with premultiplied alpha
  ([ADR-0149](/docs/adr/ADR-0149-interpolation-contracts.md)). Endpoints are unchanged; frames
  in between differ: a black-to-white fade is `rgb(99, 99, 99)` half way instead of
  `rgb(128, 128, 128)`, and red half way to black is `rgb(99, 0, 0)` instead of
  `rgb(128, 0, 0)`. A NaN `t` now returns the beginning colour.
- **`Matrix4::lerp`** (`flui-foundation`) decomposes like CSS transforms: it keeps skew and
  perspective, stays finite when an endpoint has a zero scale axis (that endpoint takes the
  other's rotation), switches at `t = 0.5` between matrices that cannot be decomposed, and
  returns the endpoints exactly.
- **`IntTween`/`StepTween`** (`flui-animation`) return `begin` for a NaN `t` instead of `0`.

### Added

- **`Angle`** (`flui-foundation`): a plane angle that keeps whole turns, with numeric `Lerp`,
  `nearest_equivalent` for the shorter arc, and `From<QuarterTurns>`.
- **`AnimatedRotation`** with **`RotationPath::{Numeric, Shorter}`** and
  **`AnimatedContainer::transform`** (`flui-widgets`).
- **`Color::to_premultiplied_oklab`/`Color::from_premultiplied_oklab`** and
  **`PremultipliedOklab`** (`flui-painting`): the space colours interpolate in.

### Removed

- **`Color::lerp_oklab`** (`flui-painting`) and **`OklabColorTween`** (`flui-animation`):
  `Color::lerp` and `ColorTween` interpolate in Oklab, premultiplied.

### Fixed

- **`AnimatedContainer`** with an overshooting curve (`Curves::EaseOutBack` from 16 to 0, say) no
  longer hands a negative padding or margin to layout, which panicked in debug builds; width and
  height clamp at zero too.
- **Hero flights** along an overshooting `Hero::curve` keep a non-negative shuttle size.
