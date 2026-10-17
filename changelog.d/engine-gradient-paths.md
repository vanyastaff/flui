### Fixed

- **Gradient shaders on paths** (`flui-engine`): a `Paint` with a linear, radial, sweep or solid
  shader drawn with `draw_path` paints the shader instead of silently falling back to
  `Paint::color`. Fills, strokes and dashed strokes all take the gradient pipelines, with the
  paint's blend mode, the current transform and every clip kind; edges are aliased like other
  tessellated paths outside supersampling.
