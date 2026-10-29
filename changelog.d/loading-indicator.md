### Added

- **Loading indicators** (`flui-material`): `LoadingIndicator` and `ContainedLoadingIndicator`,
  the Material 3 Expressive indicators that morph through seven Material shapes on a spring as
  they turn, or morph a circle into the soft burst with a determinate value. Reduced motion
  shows one still shape.
- **Expressive theme choice** (`flui-material`): `MaterialStyle` (`Baseline`, `Expressive`) on
  `ThemeData::style`; under `MaterialStyle::Expressive`, `MaterialApp` gives every
  `RefreshIndicator` a `LoadingIndicator`.
- **Shapes in the SDK** (`flui-sdk`): `painting::{Morph, RoundedPolygon, PathCommand,
  material_shapes}` for packages that draw Material shapes.
