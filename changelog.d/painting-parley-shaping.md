### Added

- **`flui-painting`**: `GlyphRasterizer`, a key type plus `rasterize(&mut self, key)`, the seam
  the engine's glyph atlas draws through; `SharedFontSystem` implements it with `GlyphKey`. The
  doc of `SharedFontSystem::rasterize` now says what an atlas does with `None` (does not place the
  glyph, asks again next use); behaviour is unchanged.
- **`flui-painting`**: behind the new, off-by-default `parley` feature (a direct edge onto the
  swash cosmic-text already builds), `parley_text::{ParleyGlyphKey, FaceKey, SubpixelBin,
  Synthesis, VariationId, FontRegistry, FontBytes, RegisterFaceError, SwashRasterizer}`.
  `FontRegistry::register_face` refuses a face key already registered over different bytes
  (`RegisterFaceError::Conflict`); a `VariationId` names the registry that minted it, and another
  registry does not resolve it. The swash rasterizer draws bit-identical bitmaps to the
  cosmic-text path for the same face, glyph, size and bin
  ([`parley_oracle.rs`](/crates/flui-painting/tests/parley_oracle.rs)), refuses a key it cannot
  draw (unregistered face, unknown variation, a size that is not finite and positive, a skew past
  `Synthesis::MAX_SKEW_DEGREES`), and emboldens by an interpolated stroke width with no checked
  Flutter reference (mapping decision 10 in
  [`flui-painting`'s ARCHITECTURE.md](/crates/flui-painting/ARCHITECTURE.md#10-synthetic-bold-uses-an-interpolated-stroke-width)).
  No production caller yet: the migration series in
  [ADR-0092](/docs/adr/ADR-0092-per-realm-text-over-parley.md) wires it.
- **`flui-painting`**: `text_layout::font_system_initialized()`, test support only.

### Changed

- **`flui-engine`**: the crate-private glyph atlas is generic over its rasterizer
  (`GlyphAtlas<R: GlyphRasterizer = SharedFontSystem>`); the default keeps the cosmic-text path,
  and every production site names it. The atlas no longer uploads a bitmap wgpu would reject: an
  image whose data does not match its size is not placed, and a grow skips a re-rasterized glyph
  whose size or content changed and drops it from the cache, so its next use asks again (both
  warned). The cosmic-text path never produces either, so its output is unchanged.
