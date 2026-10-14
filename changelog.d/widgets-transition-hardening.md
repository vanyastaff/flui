### Fixed

- **`flui-objects`**: `RenderAnimatedInset` clips its child's accessible bounds where it clips the
  child's paint, and a clip change updates semantics; its live and dry baselines saturate instead
  of overflowing.
- **`flui-objects`**: an align's child offset and baseline saturate at `±f64::MAX` for an extreme
  finite alignment instead of publishing infinite geometry; every aligning box (center, align,
  animated size, constraints-transform and overflow boxes) saturates its dry baseline alike.
- **`flui-objects`**: an animated transition sample whose invalidation wake unwinds stays owed, so
  the next notification of the same sample requests delivery again, also when the wake reenters
  the animation and a newer sample's own wake fails.
- **`flui-widgets`**: a transition whose animation replacement unwinds before the swap retries it
  on the next rebuild.
