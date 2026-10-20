### Fixed

- `RRect::contains` fits oversized radii the way paint does (`RRect::normalized`, the CSS/Skia
  factor), so a pill whose radius exceeds its height takes hits along its whole straight run
  instead of only its centre column; the engine draws (decorations included) and clips rounded
  rects with the same normalization.
- `RenderClip*` at `Clip::None` no longer clips hits: it paints no clip and reports no accessible
  clip, so its child takes the corners it paints.
- A render object whose hit test panics is a miss instead of aborting the walk: its siblings and
  ancestors are still tested, and the first panic resumes once the path is complete.
- `RenderFlow` hit-tests the children where it last painted them, without running its delegate
  or allocating per hit test.
- Hit testing no longer allocates per visited node: the walk borrows each node's children,
  `HitTestResult::new` allocates nothing, and expanded-area re-walks share one scratch result.

### Changed

- `RendererBinding::hit_test_in_view` returns `ViewHitTest`: `TreeBusy` when the render tree's
  layout is withdrawn or checked out (instead of an empty path), and `Answered` with the first
  contained render-object panic, which the runtime reports after delivering the event.
- A node's `hit_test` receives its position already mapped through `hit_test_transform`: the walk
  inverts the matrix once and refuses a degenerate one before `hit_test` runs.
  `RenderTransform`, `RenderContainer`, `RenderRotatedBox` and `RenderAnimatedTransform` no longer
  invert it themselves.
- The default `RenderBox::hit_test` depends on arity: a leaf is hit inside its size, and an
  object that can have children is hit only through a child, tested topmost first.

### Added

- `HitTestResult::with_inverse_transform`, a scope for an inverse the caller already computed,
  and `HitTestContext::child_count`.
