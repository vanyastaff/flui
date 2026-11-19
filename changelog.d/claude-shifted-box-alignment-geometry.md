### Changed

- **`flui-widgets`**: `FittedBox`, `FractionallySizedBox`, `OverflowBox`, `SizedOverflowBox`,
  `UnconstrainedBox` and `Transform` take `impl Into<AlignmentGeometry>` for their alignment, as
  `Align` does: an `AlignmentDirectional` resolves against the ambient `Directionality` (start
  is the right edge in RTL), and an absolute `Alignment` works as before and takes no
  `Directionality` dependency.
