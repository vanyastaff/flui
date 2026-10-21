### Added

- **Rounded polygons and morphing** (`flui-painting`, `paint::shapes`): `RoundedPolygon` with
  per-corner `CornerRounding` (radius and smoothing) and `star`, `circle` and `rectangle`
  factories, all fallible with `ShapeError`; `normalized()` fits a shape to the unit square.
  `Morph::new(&a, &b).path(t)` interpolates between two polygons with matching corners kept
  together. `material_shapes` holds the Material 3 Expressive shapes (`circle`, `oval`,
  `sunny`, `cookie_9`, `cookie_4`, `pentagon`, `pill`, `soft_burst`) the loading indicators
  morph between.
