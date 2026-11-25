### Changed

- **`flui-objects`**: `has_visual_overflow()` on `RenderStack`, `RenderIndexedStack`,
  `RenderWrap`, `RenderFittedBox`, `RenderAnimatedSize` and `RenderConstraintsTransformBox` is
  `true` before the first layout and after a layout that returned an error or panicked, since the
  failed pass has moved children; the box clips to the size it keeps until a layout succeeds.
  `RenderIndexedStack` answers for its displayed child only, so a hidden child that overflows no
  longer makes it clip.
- **`flui-objects`**: `RenderFittedBox::set_fit` reports a relayout, because whether the box
  crops is decided in layout; `RenderWrap::set_clip_behavior` and
  `RenderAnimatedSize::set_clip_behavior` also report a semantics update.

### Fixed

- **`flui-objects`**: a child that `RenderStack`, `RenderIndexedStack`, `RenderWrap`,
  `RenderFittedBox`, `RenderAnimatedSize` or `RenderConstraintsTransformBox` clips at its overflow
  now has its semantics rectangle and its input regions cut where its paint is cut, as under a
  `RenderClipRect`; a child wholly outside such a box reports no input region
  ([mapping decisions](/crates/flui-objects/ARCHITECTURE.md)).
