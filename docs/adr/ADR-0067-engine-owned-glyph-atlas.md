# ADR-0067: Engine-owned glyph atlas

*Text is a batch of its draw segment. The engine rasterises glyphs through
a painting door and owns the atlas; glyphon and the shaped buffer on the
wire are gone.*

---

- **Status:** Accepted (landed 2026-09-18)
- **Date:** 2026-09-18
- **Deciders:** @vanyastaff
- **Scope:** `flui_painting::{TextLayout::placed_glyphs, SharedFontSystem::rasterize, GlyphKey, PlacedGlyph, GlyphImage}`;
  `flui_engine::{glyph_atlas, batches::text, GlyphInstance, DrawSegment::glyph_batch, Phase::Glyph}`.
  Completes [ADR-0065](ADR-0065-painting-owns-shaping-text-crosses-the-display-list-shaped.md)
  (which named this as its end state) and amends
  [ADR-0016](ADR-0016-unified-font-system-registration.md) (the glyphon↔cosmic-text
  version coupling is gone).

---

## Context

After ADR-0065 the engine rasterised through glyphon, and glyphon consumes
a `cosmic_text::Buffer`. Four costs followed, each verified in the tree on
2026-09-18:

1. **Text lived outside the Command IR.** Every other primitive is an
   instance in a `DrawSegment`; a paragraph was an entry in a side batch,
   and each segment carried a `text_start..text_end` range into it. Replay
   drew those ranges in a separate render pass per segment, tracked
   "claimed" ranges through opacity-layer recursion, and drew whatever was
   left — text captured by a filter's input or an advanced shape — in
   trailing "gap passes" over everything. The recorder sealed a segment at
   the head of every geometry method (`seal_text_tail`) to keep order.
2. **Text got less paint state than a rect.** glyphon's `TextArea` takes a
   scissor rectangle and a uniform scale: no per-instance SDF clip (a
   `ClipRRect` let labels through its rounded corners), no opacity on the
   record path, no transform beyond the origin.
3. **Colour was wrong on mid-tones.** glyphon's `ColorMode::Accurate`
   converts the text colour sRGB→linear in its vertex shader, assuming an
   sRGB surface. The engine draws in gamma space onto `Unorm` surfaces
   (`Renderer::select_surface_format`, Impeller parity), so `#808080` text
   rendered as `#373737`; black and white, being fixed points, hid it.
4. **A version pin and a leaked type.** `TextLayout::buffer()` put
   `cosmic_text::Buffer` on the crate surface, and glyphon's own cosmic-text
   dependency had to equal flui-painting's or the shared `FontSystem` type
   split (ADR-0016's accepted coupling).

The market: Skia (`SkTextBlob` → `GrTextBlobCache`), Impeller (`TextFrame`
→ `GlyphAtlas`), vello (`draw_glyphs`), GPUI (`ShapedRun` → its own atlas)
all put glyph runs on the wire and own the atlas in the renderer. Iced
carries `Arc<Buffer>` for the same reason FLUI did: glyphon.

## Decision

### Painting exposes glyphs and one rasterisation door

- `TextLayout::placed_glyphs(origin, scale) -> impl Iterator<Item = PlacedGlyph>`:
  each glyph in device pixels (`x` the raster origin column, `y` the
  baseline row, the paragraph's origin folded in, fractional position
  binned into the key), with its span colour when it has one.
- `GlyphKey`: opaque newtype over cosmic-text's cache key. `Copy + Eq +
  Hash`; stable for the process because the font database is append-only.
- `SharedFontSystem::rasterize(GlyphKey) -> Option<GlyphImage>`: the bitmap
  (bearings, size, `Mask` or `Color` content, texels), rasterised under the
  font lock with a scaler kept beside the database. Not cached there — the
  engine's atlas is the cache.
- `TextLayout::buffer()` is deleted; `Shaper::font_system()` is
  crate-private; the `FontSystem` re-export is gone. The engine names no
  cosmic-text type, and `the_engine_does_not_shape` pins both the source and
  the manifest.

Rasterisation sits in painting rather than the engine because it is an
operation on the font database that the font lock already guards, and
because keeping cosmic-text a dependency of one crate is the boundary
ADR-0016 wanted and could not have while glyphon existed.

### The engine owns the atlas; glyphs are a batch of the segment

- `GlyphAtlas`: two pages — `R8Unorm` coverage masks, `Rgba8Unorm` colour
  bitmaps — packed by `etagere`'s bucketed shelf allocator, keyed by
  `GlyphKey` in an `FxHashMap`. A slot is stamped with the frame it was last
  used in; on a full page every slot not used this frame is evicted, and
  only then does the page grow (doubling to the device limit) with live
  glyphs re-uploaded at the positions they hold. `end_frame` advances the
  counter once per presented frame.
- `DrawBatcher::draw_paragraph` records a `GlyphInstance` per inked glyph
  into `DrawSegment::glyph_batch` under the active scissor run, the SDF clip
  (`apply_active_clip`), and the layer opacity — what every other instance
  gets. Glyphs fully outside the scissor are not recorded.
- `Phase::Glyph` is the last phase, so a label on its background never
  seals; `flush_segment` draws the batch at the end of the instanced pass
  when nothing sits between the arcs and the glyphs, and in its own pass
  otherwise. The glyph shader samples the page named per instance; no
  colour conversion.
- Deleted: `text.rs` (`TextRenderer`, `TextPlacement`), the glyph ranges on
  `DrawSegment`, `seal_text_tail`/`claim_text_entry`, the claimed-text
  bookkeeping and gap passes in `replay` and `layer_offscreen`,
  `EngineError::{TextPrepare, TextRender}`, the glyphon dependency, and the
  `RUSTSEC-2026-0253` (`lru` via glyphon) ignore in `deny.toml`.

## Consequences

- **Faster.** `text_throughput` (40 shaped rows over striped backgrounds,
  record → encode → submit → wait, 1 950 glyphs): steady state 470 → 400 µs
  (−15%), cold rows 762 → 645 µs (−15%), measured on the reference machine
  against the glyphon build of the same bench. The win is one render pass
  per text-bearing segment instead of two; a first cut that opened its own
  glyph pass and `discard`ed transparent texels measured +25% and was
  fixed before landing.
- **Correct where glyphon was not:** a rounded clip rounds text
  (`text_is_clipped_by_a_rounded_clip`, red without the SDF slot); mid-tone
  colours land as recorded (`glyph_colour_lands_as_recorded`, red under a
  linear conversion); text inside a filter input or advanced shape renders
  in that op's frame with the rest of its segment instead of over
  everything.
- A text-only opacity layer is non-empty by the ordinary `is_empty`
  predicate; `is_geometry_empty` is gone.
- **Transforms reach text.** Glyphs are rasterised at the CTM's
  `max_scale`; under a uniform CTM they snap to the device grid exactly as
  before, and under a rotated or anisotropic CTM each quad carries the
  CTM's linear part over that scale and is resampled (bilinear) into the
  transformed shape. The old "stretched uniformly by the larger axis" limit
  is gone: `anisotropic_scale_squashes_glyphs_on_one_axis` and
  `a_quarter_turn_rotates_the_label` replace the pin that recorded it.
- The engine gains one dependency (`etagere`, already in the lock through
  glyphon) and `rustc-hash`; it loses glyphon and, transitively, `lru 0.16`.

## Alternatives rejected

- **Rasterise in the engine with cosmic-text as an engine dependency.**
  Puts cosmic-text on two crates again and keeps a `&mut FontSystem` on the
  painting surface; the boundary argument above.
- **Keep glyphon and fix colour with `ColorMode::Web`.** Fixes fact 3 alone;
  leaves text outside the IR, without the SDF clip, with the version pin.
- **A per-glyph `Vec<u8>` on the wire (pre-rasterised).** The recorder would
  rasterise every frame it paints; the atlas exists so a glyph is
  rasterised once per (face, size, subpixel bin) for the life of the
  process.
- **A separate glyph pass always.** Simpler flush; measured as the 25%
  regression.

## Replacement tests

`a_slot_is_shared_by_equal_keys_and_an_empty_glyph_takes_no_space`,
`eviction_reclaims_slots_before_the_page_grows`,
`a_page_grows_within_a_frame_and_earlier_slots_keep_their_place`
(`crates/flui-engine/src/glyph_atlas.rs`);
`glyph_colour_lands_as_recorded`, `text_is_clipped_by_a_rounded_clip`, and
the strengthened `the_engine_does_not_shape`
(`crates/flui-engine/src/paragraph_readback_tests.rs`);
`anisotropic_scale_squashes_glyphs_on_one_axis` and
`a_quarter_turn_rotates_the_label` (`aa_oracle_tests.rs`, both red when the
affine path is disabled);
`a_quad_touching_the_scissor_edge_is_inside_and_one_past_it_is_outside`
(`crates/flui-engine/src/batches/text.rs`). The existing text readback
suites (`text_is_clipped_by_the_active_clip_rect`,
`text_ordering_across_an_opacity_layer_boundary`,
`truncated_paragraph_leaves_no_ink_below_its_line`) pass unchanged;
`anisotropic_scale_stretches_glyphs_uniformly`, which pinned the old limit,
is replaced. The
benchmark is `crates/flui-engine/benches/text_throughput.rs`.
