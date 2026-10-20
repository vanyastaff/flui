### Fixed

- **`Color::lerp` toward a transparent colour drifted at vanishing alpha** (`flui-painting`): the
  premultiplied mix is now evaluated in `f64`, so a fade to transparent keeps the opaque end's
  colour to within `2e-5` per channel right down to the last alpha step, instead of shifting it
  by `f32` rounding in the cancelling `1 - t` weight.
