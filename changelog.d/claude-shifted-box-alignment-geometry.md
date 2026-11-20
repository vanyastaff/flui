### Changed

- **`flui-widgets`**: `FittedBox`, `FractionallySizedBox`, `OverflowBox`, `SizedOverflowBox`,
  `UnconstrainedBox` and `Transform` take `impl Into<AlignmentGeometry>` for their alignment, as
  `Align` does: an `AlignmentDirectional` resolves against the ambient `Directionality` (start
  is the right edge in RTL), and an absolute `Alignment` works as before and takes no
  `Directionality` dependency.

### Fixed

- **`flui-objects`**: `RenderFittedBox::set_alignment`, and `set_fit` when only the fitted
  transform moves, report the re-placing of the child's semantics as well as paint, so a moved
  child's accessibility bounds follow its pixels.
