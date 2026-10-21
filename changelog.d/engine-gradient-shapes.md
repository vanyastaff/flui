### Fixed

- **Gradient shaders on shapes** (`flui-engine`): a `Paint` with a linear, radial, sweep or solid
  shader paints the shader instead of `Paint::color` on stroked rects, rounded rects and circles,
  on filled and stroked ovals and arcs, on `draw_drrect` rings and on `draw_line`. These shapes
  draw their tessellated outline through the gradient pipelines, with aliased edges like
  gradient paths.
