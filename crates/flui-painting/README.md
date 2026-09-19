# flui-painting

Records 2D drawing into a `DisplayList` and shapes text with cosmic-text.
Nothing is rasterised here — `flui-engine` replays the list on the GPU.

```rust
use flui_painting::{Canvas, Paint};
use flui_types::{Rect, geometry::px, styling::Color};

let mut canvas = Canvas::new();
canvas.save();
canvas.translate(10.0, 10.0);
canvas.draw_rect(
    Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
    &Paint::fill(Color::RED),
);
canvas.restore();
let list = canvas.finish(); // Save, DrawRect, Restore
```

- `Canvas` — `dart:ui`'s `Canvas`: save/restore, transforms, clips, `draw_*`.
  A render object or a `CustomPaint` painter draws against it.
- `DisplayList` / `DrawCommand` — the recorded commands with their transform
  baked in; the closed vocabulary `flui-engine` matches exhaustively.
- `TextPainter` / `TextLayout` — lay an inline span out against a width
  constraint, query caret / hit-test / lines, paint it. Shaping goes through
  the process-wide font system that the engine shares
  (`shared_font_system()`), so a face registered through
  `SharedFontSystem::register_font` measures and paints alike.
- `paint_box_decoration`, `paint_table_border` — the Flutter-shaped
  decoration painters.

The paint vocabulary (`Paint`, `Shader`, `BlendMode`, `Path`, geometry) is
defined in `flui-types` and re-exported.

## Tests

```bash
cargo nextest run -p flui-painting
```

`flui_painting::testing::record` (the `testing` feature) records a closure's
drawing into a `DisplayList` — see `docs/TESTING.md`.

[`ARCHITECTURE.md`](ARCHITECTURE.md) has the module map, the recorder and
text contracts, the mapping decisions, and the open items.
