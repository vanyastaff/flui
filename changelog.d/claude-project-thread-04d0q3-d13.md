### Fixed

- **`flui-painting`**: text shadows (`TextStyle::shadows`) and span backgrounds
  (`TextStyle::background_color`) now paint. They were accepted and interpolated but never reached
  the display list. A shadow is drawn as the casting span's glyphs, offset, filled with the shadow
  colour and blurred by its radius. A colour emoji casts a silhouette. A background fills each line
  box of its span, under an ellipsis too
  ([ADR-0309](/docs/adr/ADR-0309-every-text-paint-attribute-crosses-the-display-list.md)).
- **`flui-widgets`**: `IconThemeData::shadows` paints under the icon glyph, and
  `IconThemeData::opacity` scales the icon colour's alpha. Both were ignored before.

### Added

- **`flui-painting`**: `DrawOp::ParagraphShadow`, `Canvas::draw_paragraph_shadow` and `BlurSigma`,
  a blur in logical pixels bounded to `BlurSigma::MAX`.

### Changed

- **`flui-painting`**: `TextStyle::shadows` and `IconThemeData::shadows` hold `Shadow<f64>`.

### Removed

- **`flui-painting`**: `TextStyle::foreground`, `TextStyle::background` and `TextShadow`, which
  nothing painted. Use `TextStyle::color`, `TextStyle::background_color` and `Shadow<f64>`.
