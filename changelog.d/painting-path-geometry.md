### Added

- **Path geometry** (`flui-painting`): `Path::length`, `Path::trim(start, end)` (fractions of
  the length; `start > end` wraps through the start), `Path::transform(&Matrix4)` (affine only;
  perspective is refused with `None`) and `Path::lerp(a, b, t)` for paths with the same element
  kinds. They are the geometry the icon effects, M3 Expressive shapes and loading indicators
  build on.
