# flui-painting

Records 2D drawing into a `DisplayList` and shapes text with Parley.
Nothing is rasterised here — `flui-engine` replays the list on the GPU.

```rust
use flui_painting::{Canvas, Paint};
use flui_foundation::geometry::Rect;
use flui_painting::styling::Color;

let mut canvas = Canvas::new();
canvas.save();
canvas.translate(10.0, 10.0);
canvas.draw_rect(
    Rect::from_ltrb(0.0, 0.0, 40.0, 40.0),
    &Paint::fill(Color::RED),
);
canvas.restore();
let list = canvas.finish(); // Save, DrawRect, Restore
```

- `Canvas` — `dart:ui`'s `Canvas`: save/restore, transforms, clips, `draw_*`.
  A render object or a `CustomPaint` painter draws against it.
- `DisplayList` / `DrawCommand` — the recorded commands with their transform
  baked in; the closed vocabulary `flui-engine` matches exhaustively.
- `TextPainter` — lays an inline span out against a width constraint,
  answers caret / hit-test / line queries, paints it. Measurement, paint and
  the queries read one Parley layout, shaped through the UI runtime's
  `TextContext`. A face an app registers (`flui::register_font`, through the
  app's `FontCollection`) reaches all three alike.
- `paint_box_decoration`, `paint_table_border` — the
  decoration painters.

The crate owns the paint, style and text values:

- `paint` — `Paint`, `Path` (a kurbo `BezPath` inside, with exact winding and
  tight bounds), `Shader`, `BlendMode`, `Clip`, images and effects;
- `styling` — `Color` (`Color::lerp` interpolates in premultiplied Oklab), borders,
  `BorderRadius`, `BoxDecoration`, gradients and shadows;
- `typography` — `TextStyle`, `FontWeight`, spans, alignment and metrics;
- at the root, `Alignment`, `BoxFit`, `BoxShape` and `TextBaseline`.

Geometry values (`Point`, `Offset`, `Size`, `Rect`, `RRect`, `Matrix4`) come
from `flui_foundation::geometry`; lengths are `f64` logical pixels.

## Tests

```bash
cargo nextest run -p flui-painting
```

`flui_painting::testing::record` (the `testing` feature) records a closure's
drawing into a `DisplayList` — see `docs/testing.md`.

[`ARCHITECTURE.md`](ARCHITECTURE.md) has the module map, the recorder and
text contracts, the mapping decisions, and the open items.
