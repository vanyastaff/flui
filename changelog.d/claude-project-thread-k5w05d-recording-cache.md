### Changed

- The renderer keeps the draw recording of a chunk whose content, transform and surroundings are
  unchanged, or which only moved by whole device pixels from a position on a dyadic grid (whole
  or quarter pixels, as a list scrolled by whole pixels is), and replays it instead of recording
  it again: a full frame of 60 text-heavy chunks takes about half the CPU it took. A chunk moved
  by a fraction of a pixel, from a position whose fraction rounds differently at the new one, or
  under a fixed clip that cuts it, and chunks with a clip, a layer, a draw item or an external
  texture of their own, still record every time.

### Fixed

- Text placed left of or above the device origin lands on the pixel and subpixel bin a whole-pixel
  move from the right or below predicts, and text far from the origin no longer picks another
  bin or row: glyph placement floors with one rule for every sign instead of truncating, and
  `ShapedRun::placed_glyphs` takes the paragraph's whole pixels and the rest of its position
  apart, as `placed_glyphs(key, whole, offset, scale)`, so the whole pixels only move the glyphs.
  A glyph whose pen alone is past the `i32` range is omitted instead of drawn at a saturated
  column.
