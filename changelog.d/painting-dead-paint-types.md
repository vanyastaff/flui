### Removed

- `flui::painting::paint::{PaintingStyle, PathOperation, VertexMode, BlurMode, BlurQuality,
  PathPaintMode, StrokeOptions, ImageConfiguration, MaskFilter, BlurStyle, ClipBehavior}`: nothing
  drew with them. A paint's fill or stroke is `PaintStyle`, its stroke settings are `Paint`'s own
  fields, and a clip's quality is `Clip`.

### Fixed

- Recording a canvas no longer opens a tracing span at the default `info` filter on every
  `Canvas::finish`, `Canvas::take_recording` and `Canvas::save_layer`, which ran on every repaint.
