### Fixed

- **`flui-objects`**: `RenderAnimatedInset` clips its child's accessible bounds where it clips the
  child's paint, and a clip change updates semantics; its live and dry baselines saturate instead
  of overflowing.
- **`flui-objects`**: an aligning box (align, center, animated align and size, constraints-transform
  and overflow boxes) bounds its child offset to `f64::MAX / 2^20`, less the child's extent in the
  forward direction, and saturates its dry baseline, so an extreme finite alignment no longer
  publishes infinite geometry. The immediate child's far edge and same-sign nesting up to about
  2^20 levels stay finite; a descendant overflowing past its parent's reported extent and mixed-sign
  compositions of extreme offsets are outside that range.
- **`flui-objects`**: an animated transition sample whose invalidation wake unwinds stays owed, and
  the next notification retries it before reading the animation, whether that notification carries
  the same sample, a refused value, a failing read or the exhausted ticket counter. A wake that
  changes the animation again neither recurses nor requests twice.
- **`flui-widgets`**: a transition whose animation replacement unwinds before the swap retries it
  on the next rebuild.
