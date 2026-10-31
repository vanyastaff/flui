### Added

- **`flui-painting`**: `ImageFilter::Transform` holds a general 2D affine map (`ImageTransform`,
  refused with `InvalidImageTransform` when it is not finite or not invertible) and its sampling
  (`ImageSampling`). `ImageFilter::try_magnify` builds the magnifier: the backdrop scaled about a
  focal point.
- **`flui-engine`**: a backdrop filter with an axis-aligned transform draws the backdrop magnified
  or shrunk. A rotated or sheared transform, nearest sampling, or a transform on a layer's own
  content fails the frame with `UnsupportedBackdropFilter` or the new `UnsupportedImageFilter`.
