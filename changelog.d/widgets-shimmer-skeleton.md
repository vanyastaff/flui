### Added

- **`flui-widgets`**: `Shimmer` and `Skeleton` loading placeholders. A `Shimmer` sweeps one
  highlight band across its subtree every 1.5 s by default (`period`, `direction`,
  `highlight`, `band_width`); `Skeleton::rect`, `line`, `circle` and `text` each paint
  their own window of that band, so a screen of placeholders reads as one sweep.
  Reduced motion, `enabled(false)`, a disabled `TickerMode` and a missing clock paint
  every skeleton in its flat colour. Assistive technology hears one progress indicator
  ("Loading" by default) for the region and none of the placeholders.
- **`flui-widgets`**: `Effect::Shimmer(ShimmerBand)` and `Effects::shimmer` sweep a band
  once over arbitrary content through a shader mask that tints only what the child
  painted.
- **`flui-objects`**: `RenderSkeleton`, `RenderShimmerMask` and `RenderPaintAnchor`, with
  the `ShimmerBand` and `ShimmerSweep` values they share.
- **`flui-rendering`**: `RenderObject::is_paint_anchor` and `PaintCx::paint_anchor`: a node
  learns its offset from the nearest anchor at paint time, across repaint boundaries.
