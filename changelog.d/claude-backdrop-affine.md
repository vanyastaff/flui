### Fixed

- A backdrop filter's `ImageFilter::Transform` draws through any affine map, rotation and shear included, resampled in a shader pass; before, a map that was not axis-aligned in device space failed the frame with `UnsupportedBackdropFilter`.
