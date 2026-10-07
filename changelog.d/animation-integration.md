### Added

- **`flui-objects`**: `RenderAnimatedTransform` with `TransformMotion::{Slide, Scale, Rotation}`:
  a transform that follows an animation without rebuilding the element tree.

### Changed

- **`flui-widgets`**: `SlideTransition`, `ScaleTransition` and `RotationTransition` update a
  transform layer per frame instead of rebuilding; `RefreshIndicator` no longer rebuilds its
  subtree on every scroll pixel or pull-distance change, only when refreshing starts or ends.

### Removed

- **`flui-testing`**: `LaidOut::transform_scale` and `transform_rotation`; read a child's
  matrix with `PipelineOwner::transform_to`.
