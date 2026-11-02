### Fixed

- A backdrop filter's `ImageFilter::Transform` with `ImageSampling::Nearest` draws, each output pixel taking the backdrop texel it falls in; before, the frame failed with `UnsupportedBackdropFilter`.
