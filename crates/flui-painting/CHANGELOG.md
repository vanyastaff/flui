# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed — `get_word_boundary` uses UAX #29 word segmentation, not an ASCII-whitespace scan

- `TextLayout::get_word_boundary` now segments with
  `unicode-segmentation`'s `split_word_bound_indices` instead of walking
  ASCII whitespace bytes: it no longer treats a straight apostrophe as a
  word break by accident, no longer returns the whole line for scripts
  with no ASCII whitespace at all (CJK), and treats a run of whitespace as
  one segment rather than a sequence of one-byte gaps. New direct
  dependency on `unicode-segmentation` (already resolved transitively
  through `cosmic-text`; this is a direct edge onto the same copy, not a
  new one). Clusters-only, not dictionary-based — Thai/Lao/Khmer remain a
  known limitation; see `flui-widgets/ARCHITECTURE.md`'s Mapping decision
  for this feature.

### Changed — glyphs cross to the engine, not the buffer (ADR-0067, 2026-09-18)

- `TextLayout::placed_glyphs(origin, scale)` yields `PlacedGlyph { key:
  GlyphKey, x, y, color }` in device pixels; `SharedFontSystem::rasterize(
  GlyphKey) -> Option<GlyphImage>` is the fourth door of the font system.
  `GlyphKey`, `PlacedGlyph`, `GlyphImage`, `GlyphContent` are exported.
- `TextLayout::buffer()` is deleted, `Shaper::font_system()` is
  crate-private, and the `FontSystem` re-export is gone: `Family` (on
  `ResolvedFont`) is the one cosmic-text type on the surface.

### Changed — the command is `{ transform, op }` (ADR-0066, 2026-09-18)

- `DrawCommand` is now `struct { transform: Matrix4, op: DrawOp }`; the
  operations live in `DrawOp` without the `Draw` prefix (`Rect`, `Path`,
  `Paragraph`, …) and without per-variant transform fields. `Save`,
  `Restore`, and `RestoreLayer` are unit variants: they scope clips and
  nothing else. `DrawCommand::untransformed(op)` builds one under the
  identity.
- `DrawGradient`/`DrawGradientRRect` and `Canvas::draw_gradient`/
  `draw_gradient_rrect` are gone: a gradient is a `Shader` on a fill
  `Paint` drawn through `draw_rect`/`draw_rrect`/`draw_circle`.
  `paint_box_decoration` records one shader paint for all three
  silhouettes, so a rounded gradient keeps its per-corner radii.
- `Canvas::draw_picture(&DisplayList)` is back: every command replays as
  `ctm * command.transform`.
- `flui_types::painting::Path` is copy-on-write (`Arc<Vec<PathCommand>>`);
  a clone is a refcount bump, and `Path::shares_commands_with` observes it.
- `DrawOp` is at most 128 bytes and `DrawCommand` 192
  (`draw_command_fits_its_budget`); the previous command was 560 bytes.
- New criterion bench `display_list_record`.

### Changed — text crosses the display list shaped (ADR-0065 part 2, 2026-09-18)

- `DrawCommand::DrawText` and `DrawTextSpan` are replaced by
  `Paragraph { layout: Arc<TextLayout>, offset, color }`, recorded by
  `Canvas::draw_paragraph`; `Canvas::draw_text`/`draw_text_span` are gone.
  `TextPainter::paint` records the very layout it measured, so `max_lines`,
  the ellipsis, and per-span faces paint as measured.
- `TextLayout::buffer()`, `text()`, `describe_runs()` added; a span's colour
  is baked into its shaped attrs when it differs from the root's, and
  `TextPainter::set_text` reports a span recolour as `Invalidation::Layout`.
- `TextPainter::get_offset_for_caret` takes `&self`.
- serde derives are off `DrawCommand` and `DisplayList` (nothing enabled
  them; a shaped layout is not serialisable).

### Changed — the font system has three doors (ADR-0065 part 1, 2026-09-18)

- `SharedFontSystem::with_mut`, `resolve_font`, and `resolve_fonts` are
  replaced by `shape(|Shaper| …)` (resolve + shape under one lock, no
  generation bump), `register_font` (the only mutation, append-only, bumps
  the generation), and `generation()`. `TextPainter`'s layout cache and the
  engine's buffer caches key on the generation, so a face registered after
  layout is picked up at the next layout.
- The embedded Roboto / Material Icons / Cupertino Icons faces moved here
  (`flui_painting::fonts`, feature `bundled-fonts`, on by default) and are
  installed at font-system construction; `flui_engine::fonts` and the
  engine's `ensure_fonts_available` are gone, and a headless test measures
  an icon in the face the app paints.
- `TextLayout::get_offset_for_caret` takes `&self` and walks the laid-out
  runs (it no longer takes the font lock, and it answers on wrapped lines,
  where the previous implementation stopped at the first run).

### Changed — surface cut to what has a consumer (2026-09-18)

- **Deleted:** `PaintingBinding`, `ImageCache`, `CachedImage`, `ImageHandle`,
  `SystemFontsNotifier` (the live decode cache is
  `flui_widgets::image::decode_cache`; nothing listened to the notifier);
  `ClipContext` (no production implementor); the sealed
  `DisplayListCore`/`DisplayListExt` pair and `DisplayListStats`;
  `DisplayList`'s mutation and analysis surface (`iter_mut`, `IndexMut`,
  `AsMut`, `filter`, `map`, `to_opacity`, `apply_transform`, `stats`);
  `DrawCommand::{ShaderMask, BackdropFilter}` (no producer; masks and
  backdrop filters are layers) with `Canvas::draw_shader_mask` /
  `draw_backdrop_filter` and the engine's second lowering for them;
  `CommandKind`, `DrawCommand::{with_opacity, kind, is_*, paint, has_paint,
  transform, transform_mut, apply_transform}`; the `prelude`;
  `PaintingError` (five variants nothing produced) → `RegisterFontError`;
  `measure_text`, `measure_inline_span`, `detect_text_direction`, `LineInfo`,
  `TextLayout::{get_line_info, has_rtl_content, is_bidirectional,
  was_truncated}`; `TextPainter`'s accepted-and-ignored `strut_style` /
  `text_width_basis` / `text_height_behavior` / `placeholder_dimensions`
  and `did_layout`; the non-`dart:ui` `Canvas` extras with no consumer
  (`draw_point`, `draw_polyline`, `with_save`/`with_translate`/`with_rotate`/
  `with_rotate_around`/`with_scale`/`with_scale_xy`, `rotate_around`,
  `scale_uniform`, `clear_commands`, `local_clip_bounds`/
  `device_clip_bounds`/`would_be_clipped` — the last three answered for the
  last clip only and ignored `ClipOp::Difference`); `draw_picture` (it
  ignored the current transform; returns with the `DrawCommand` split);
  `CanvasState`/`ClipShape` from the public surface; `docs/{ARCHITECTURE,
  PERFORMANCE,README,MIGRATION}.md` and `CONTRIBUTING.md`.
- `shared_font_system()` is public and `register_font` lives on
  `SharedFontSystem`; `AppRuntime` installs the font system at realm install.
- `DisplayList::bounds()` and `Canvas::bounds()` return `Option<Rect>`;
  `commands()` returns a slice; `len`/`is_empty`/`bounds`/`commands` are
  inherent. `PictureLayer`/`CanvasLayer::bounds()` follow.
- `DrawCommand` is no longer `#[non_exhaustive]`; the engine's dispatch
  matches it exhaustively with no wildcard arm.
- `Canvas::scale(sx, sy)` replaces `scale_xy`; the four clip methods and
  their `_ext` forms share one private `push_clip`. Clip lifetime is carried
  by the recorded save/restore commands; the unused clip-depth counter is gone.
- `DisplayList` serialization contains commands only. Deserialization recomputes
  bounds from those commands instead of accepting an externally supplied cache.
- `DisplayList::append_isolated` adds a balanced run inside save/restore markers.
  The rendering composer uses it to keep canvas clips local to each paint run;
  `append` remains raw concatenation with shared replay state.
- `flui_types::painting::Paint` derives `PartialEq` (the hand-written impl
  skipped `shader`); `flui_geometry::RRect::contains` replaces three copies
  of point-in-rounded-rect across this crate and `flui-objects`.
- Docs: the front page is a compiled example; the `!Sync` claims on
  `Canvas`/`TextPainter` (false) and the "restore on panic" claim on the
  scoped helpers (false) are gone; `ARCHITECTURE.md` is a current-state
  document with the open items (double shaping, cache invalidation,
  `with_mut` reentrancy, command size).


### Removed

- **`text_layout::fallback` parallel `TextLayout`** (plan U8 / audit P-3) —
  the stub `TextLayout` and the `text` Cargo feature were both deleted;
  cosmic-text-backed layout is now the only path. The `--no-default-features`
  build shape changes because the optional `text` feature no longer exists,
  but no workspace consumer relied on disabling it. Drops the corresponding
  `tests/text_layout_fallback.rs` integration test.
- **`canvas::sugar` invented ergonomics** (plan U8 / audit P-4/P-14/P-15) —
  removed the entire `canvas/sugar/` directory plus the `Canvas::record` /
  `Canvas::build` static constructors. No Flutter analogue; sugar wrappers
  over the primary `draw_*` / `with_*` / composition surface.

### Added

- **Tracing Instrumentation**
  - Added `#[tracing::instrument]` to performance-critical operations
  - Canvas::finish(), extend_from(), save_layer()
  - DisplayList::append(), to_opacity()
  - Structured logging with spans and fields for production debugging

- **Canvas Composition Methods**
  - `Canvas::extend(impl IntoIterator<Item = Canvas>)` - extends from multiple canvases (follows std pattern)
  - `Canvas::merge(self, other) -> Self` - functional-style merge of two canvases
  - Both methods use zero-copy move semantics

- **Common Traits (C-COMMON-TRAITS)**
  - Added `Clone` implementation for `Canvas` for better ergonomics

- **Optional Serde Support (C-SERDE)**
  - Added optional `serde` feature for serialization
  - `DisplayList`, `DrawCommand`, `DisplayListStats` support Serde
  - HitRegion skipped during serialization (contains function pointers)

- **API Guidelines Compliance Audit**
  - Comprehensive audit document (API_GUIDELINES_AUDIT.md)
  - 98% compliance with Rust API Guidelines (47/48 applicable)
  - Production-ready status confirmed

### Changed

- **API Naming Consistency (RFC 430)**
  - Renamed error variants for consistent word order:
    - `DecorationFailed` → `PaintDecorationFailed`
    - `TextPaintingFailed` → `PaintTextFailed`
    - `ImageFailed` → `PaintImageFailed`
  - Updated helper methods to match: `paint_decoration_failed()`, `paint_text_failed()`, `paint_image_failed()`
  - Follows Rust API Guidelines C-WORD-ORDER for error types

- **API Method Naming (C-CONV, C-METHOD)**
  - Renamed `DisplayList::with_opacity()` → `to_opacity()`
    - Uses `to_` prefix for expensive borrowed-to-owned conversions
  - Renamed `Canvas::append_canvas()` → `extend_from()`
    - Uses `extend_from` for consuming ownership (vs. `append(&mut)` which drains)
    - Follows Rust standard library conventions
  - All methods follow Rust API Guidelines naming conventions

## [0.1.0] - 2024-11-28

### Added

- **Extension Traits Pattern**
  - Split DisplayList API into `DisplayListCore` (sealed) and `DisplayListExt` (public)
  - Enables future API additions without breaking changes
  - Users can add custom extension traits

- **Iterator Methods**
  - Added `DisplayList::iter()` and `iter_mut()` for clippy convention
  - Implemented `IntoIterator` for `&DisplayList` and `&mut DisplayList`
  - Enables ergonomic iteration: `for cmd in &display_list { }`

- **Debug Trait**
  - Added `Debug` implementation for `Canvas`
  - All public types now implement Debug

- **Safe restore() Behavior**
  - `Canvas::restore()` no longer panics when called without matching `save()`
  - Now a safe no-op if save stack is empty
  - Matches behavior of common 2D graphics APIs

- **Comprehensive Documentation**
  - Added detailed crate-level documentation with examples
  - Added `docs/` directory with guides:
    - Architecture Guide (internal design and patterns)
    - Performance Guide (optimization techniques)
    - Migration Guide (version upgrade instructions)
    - Contributing Guide (contributor guidelines)
  - Enhanced prelude documentation
  - Improved trait documentation with examples

- **Modern Rust Patterns**
  - Sealed traits for API stability
  - Extension traits for modularity
  - Smart pointer support (Arc, Box, &)
  - Standard traits (AsRef, AsMut, Index, IndexMut)
  - `#[must_use]` on filtering methods
  - `#[non_exhaustive]` on error types
  - Const constructors where applicable

- **Quality Control**
  - Enabled comprehensive clippy lints (all, pedantic)
  - Added documentation lints (broken links, missing docs)
  - Zero clippy warnings with strict checks
  - Added missing Debug implementations

### Changed

- **Canvas::restore()** - No longer panics, now a no-op when called without save()
- **DisplayList API** - Methods split into core and extension traits (requires trait import)
- **Documentation** - Removed all Flutter-specific references, focusing on FLUI's own design

### Fixed

- Corrected documentation links
- Fixed clippy warnings for IntoIterator convention
- Updated test for safe restore() behavior

### Performance

- No performance regressions
- All optimizations preserved (zero-copy composition, transform baking, etc.)

## [0.0.1] - Initial Development

### Added

- Core Canvas API for recording 2D drawing commands
- DisplayList for immutable command sequences
- DrawCommand enum with all primitive types
- Transform stack with save/restore
- Clipping support (rect, rounded rect, path)
- Paint configuration (fill, stroke, blend modes)
- Path drawing with Bezier curves
- Image drawing (simple, repeat, nine-slice, filtered)
- Text rendering support
- Layer composition with effects
- Shader masks and backdrop filters
- Thread-safe design (Send for Canvas, Send + Clone for DisplayList)
- Zero-copy canvas composition
- Batch drawing methods
- Scoped operations (closure-based API)
- Chaining API (builder pattern)
- Debug helpers

[Unreleased]: https://github.com/flui-org/flui/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/flui-org/flui/releases/tag/v0.1.0
[0.0.1]: https://github.com/flui-org/flui/releases/tag/v0.0.1
