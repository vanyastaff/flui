### Changed

- **`flui-animation`**: the trait `TwoWayConverter` is `MotionValue`, the name its derive already
  had; one `use flui_animation::MotionValue` brings the trait and the derive.
- **`flui-animation`**: `#[derive(MotionValue)]` writes only the motion vector. Interpolation is the
  separate `#[derive(Lerp)]`, so a type that interpolates its own way derives its vector and keeps
  its own `impl Lerp`.

### Added

- **`flui-foundation`**: `#[derive(Lerp)]` in `flui_foundation::geometry` (`flui::geometry`),
  field by field, with exact endpoints; a generic struct interpolates whenever its fields do.
