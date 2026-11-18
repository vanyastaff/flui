### Changed

- A decoration value paints itself: `BoxDecoration::paint(canvas, rect, options)`,
  `BoxDecoration::hit_test(rect, position)` and `TableBorder::paint(canvas, rect, rows,
  columns)`. `DecorationPaintOptions` is `flui_painting::styling::DecorationPaintOptions`.
- `FontWeight` holds any CSS weight from 1 to 1000 (`FontWeight::try_new`, `value`,
  `TryFrom<u16>`), so a variable font draws 350 and an animated weight lands on 550; the
  `W100`…`W900`, `NORMAL` and `BOLD` constants keep their names.
- `FontFeature` and `FontVariation` name their setting by an `OpenTypeTag`, four printable
  ASCII bytes checked when built (`OpenTypeTag::new(*b"smcp")`, `OpenTypeTag::try_new("wght")`);
  a feature value is `u16`. A tag shaping would drop no longer builds.
- `Canvas::clip_rect_ext`, `clip_rrect_ext` and `clip_path_ext` are `clip_rect_with`,
  `clip_rrect_with` and `clip_path_with`.
- `FontCollection::register_font` and `flui::register_font` take
  `impl Into<Cow<'static, [u8]>>`: an owned `Vec<u8>` moves in and a `&'static [u8]` is
  borrowed, where every registration copied the font.

### Removed

- The free painters `paint_box_decoration`, `box_decoration_hit_test` and
  `paint_table_border` (use the methods above).
- `FontWeight::from_css` (use `FontWeight::try_new`).
- `TextureId`'s `From<u64>`, `From<TextureId> for u64` and `is_null`: a texture registry
  issues the id.
