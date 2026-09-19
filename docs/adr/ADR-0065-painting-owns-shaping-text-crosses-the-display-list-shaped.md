# ADR-0065: Painting owns shaping; text crosses the display list shaped

*Text was shaped twice — once to measure in `flui-painting`, once to paint in
`flui-engine` — and only the first shaping knew about `maxLines` and the
ellipsis. One shaper, one font door, and a shaped paragraph on the wire make
"painted as measured" true by construction instead of by two copies agreeing.*

---

- **Status:** Accepted (parts 1 and 2 landed 2026-09-18)
- **Date:** 2026-09-18
- **Deciders:** @vanyastaff
- **Scope:** `flui-painting`'s font-system surface (`SharedFontSystem`,
  `Shaper`, `fonts`), its `TextLayout`/`TextPainter` paint path, the
  `DrawCommand` text vocabulary, and `flui-engine`'s `TextRenderer`. Amends
  [ADR-0016](ADR-0016-unified-font-system-registration.md): the shared
  `FontSystem` stays the boundary; what changes is who may mutate it and what
  the engine reads.

---

## Context

Three facts, each verified in the tree on 2026-09-18:

1. **Two shapers.** `TextPainter::paint` discarded its shaped `TextLayout`
   and recorded `DrawCommand::DrawTextSpan { span, size, wrap_width, .. }`;
   the engine re-shaped the span with its own copy of the style→attrs
   mapping, bound `size: _`, and had no notion of `max_lines` or an
   ellipsis. A paragraph measured as one line ending in "…" painted as the
   whole string. Issue #929 ("measured in one font, painted in another") was
   this defect surfacing once; `ResolvedFont` patched that instance.
2. **No invalidation.** `TextPainter`'s layout cache and the engine's two
   shaped-buffer caches never learned that `register_font` had run; text laid
   out in a fallback face stayed there until its constraints changed.
3. **The wrong door.** `SharedFontSystem::with_mut` handed out
   `&mut FontSystem` under a non-reentrant `parking_lot::Mutex`, so any text
   API called inside the closure deadlocked (documented, not enforced), and
   it bumped the database generation on every call — including the engine's
   per-frame shape — so `InstalledFamilies` rebuilt its family index over
   every host face once per paragraph per frame. And the embedded baseline
   faces (Roboto, Material Icons, Cupertino Icons) were loaded by the engine,
   so a headless widget test measured an `Icon` in whatever fallback the host
   offered while the live app painted it from the embedded font — the
   measure/paint split ADR-0016 exists to close, reopened in the test tier.

glyphon (the engine's rasteriser) consumes a `&cosmic_text::Buffer`; it
cannot take glyph runs. So "glyph runs on the wire" is not a wire change but
an engine-owned rasteriser, which is its own project.

## Decision

### Part 1 — the font system has three doors

`SharedFontSystem` exposes exactly:

- `shape(|shaper: &mut Shaper| …)`: holds the lock, exposes
  `Shaper::resolve_font` and `Shaper::font_system`, never bumps the
  generation. Resolution and shaping happen under one acquisition, so the
  "resolve before `with_mut`, never inside" deadlock has no reachable shape.
- `register_font(&[u8])`: the only mutation of the database, append-only,
  bumps `generation()`. A face is never removed, so a font id recorded
  anywhere stays valid for the life of the process.
- `generation()`: what every cache of shaped text keys on.
  `TextLayoutCache` re-lays-out when it moves; the engine drops its buffer
  caches at the next frame boundary when it moves.

`with_mut`, `resolve_font`, and `resolve_fonts` are gone. The embedded
faces move to `flui_painting::fonts` behind the default-on `bundled-fonts`
feature and are installed at font-system construction — Roboto when the host
database is empty, each icon family when absent — before the generic
families are bound. The engine never touches `db_mut`.

### Part 2 — one `Paragraph` op on the wire

`DrawCommand::DrawText` and `DrawTextSpan` are replaced by
`Paragraph { layout: Arc<TextLayout>, offset, color }`, recorded by
`Canvas::draw_paragraph`. `TextPainter::paint` records the very `Arc` it
measured (`Arc::ptr_eq` is the contract). `TextLayout` is immutable after
construction (its caret query walks the laid-out runs, lock-free). The
engine reads `TextLayout::buffer()` and hands it to glyphon; its shaper, its
two caches, and its style→attrs copy are deleted. The root colour rides on
the op, so a root-only recolour is `Invalidation::Paint` with the same
shaped buffer; a span colour is baked into the layout's attrs, so a span
recolour is `Invalidation::Layout` and shapes once at the next layout
(`TextPainter::set_text` classifies by comparing the per-run colours).
`WgpuPainter::draw_text` (the hand-driven painter's plain-text
convenience) shapes through `TextLayout` and records a paragraph.

## Consequences

- `max_lines`, the ellipsis, per-span styling: painted as measured, by
  construction. The engine loses its shaper and both buffer caches (~600
  lines); a mechanical guard pins that no `set_text`/`set_rich_text`/
  `shape_until_scroll` exists in `flui-engine`.
- A paragraph's recorded bounds are its laid-out lines' box (offset +
  longest line × height), tighter than Flutter's `Offset.zero & size` box
  of the render object; the ink is inside either.
- The structural snapshots print what reached the shaper per run (family,
  weight, style, size, span colour) instead of the span tree's requested
  styles — so a pinned face set that carries no bold face now shows
  `weight=400`, which is what is painted.
- `cosmic_text::Buffer` was reachable through one painting accessor
  (`TextLayout::buffer`), inside the ADR-0016 boundary and named as the
  accessor a future engine-owned rasteriser deletes — ADR-0067 deleted it
  the same day.
- A headless test measures icons in the face the app paints
  (`icon_fonts_measure_before_any_engine_exists`).
- **Named gap, not closed:** a registration invalidates caches, but nothing
  marks text render objects dirty on registration (Flutter's
  `PaintingBinding.systemFonts` listener). That is a realm-level broadcast
  owned by flui-app.
- **Deferred with a trigger:** a `FontContext` handle threaded through
  layout instead of the `OnceLock` static. Its cost is every capability
  context constructor in flui-rendering plus the test bootstraps, and it
  buys nothing behaviourally while all realms share one font collection
  (as Flutter's do). It returns when a second font source (per-realm fonts,
  sharded databases) or closing the ambient-reach ratchet becomes a
  requirement (ADR-0027 follow-up 6).

## Alternatives rejected

- **Glyph runs on the wire now.** Requires an engine-owned rasteriser
  (swash + atlas + mask pipeline); glyphon's internals are `pub(crate)`.
  It is the end state (Skia `SkTextBlob`, Impeller `TextFrame`, GPUI
  `ShapedRun`), and the `Arc<TextLayout>` handle survives it — the accessor
  changes, not the wire type.
- **Unshaped text plus `max_lines`/`ellipsis`/`ResolvedFont` fields.**
  Keeps two shapers; every shaper feature (alignment, decorations, shadows)
  stays a two-crate change — the "manage the divergence" trap ADR-0016
  already rejected.
- **Keep `with_mut`, add `generation()`.** The deadlock stays representable
  and every engine shape keeps rebuilding the family index.

## Replacement tests

Part 1: `shaping_never_bumps_the_generation`,
`register_font_bumps_the_generation_once`,
`register_font_invalidates_a_laid_out_painter`,
`icon_fonts_measure_before_any_engine_exists`
(`crates/flui-painting/tests/font_registration.rs`);
`an_empty_host_database_gets_roboto_and_both_icon_faces` and its two
siblings (`crates/flui-painting/src/fonts.rs`).

Part 2: `a_truncated_paragraph_paints_exactly_the_lines_it_measured`
(records the measured `Arc`, one line, ellipsis in the text) and
`root_recolor_keeps_the_shaped_buffer_and_span_recolor_reshapes_once`
(`crates/flui-painting/tests/text_overflow_unit.rs`);
`max_lines_one_reaches_the_composited_picture_as_one_line` (widgets parity
tier — the `Text` widget wires `max_lines` but not `TextOverflow::Ellipsis`,
a gap this ADR names and does not close);
`truncated_paragraph_leaves_no_ink_below_its_line` (engine readback) and
the mechanical `the_engine_does_not_shape`
(`crates/flui-engine/src/paragraph_readback_tests.rs`).
Flutter's `text_painter_test.dart` maxLines/intrinsics block is
`skip: true` upstream (#13512); the paint half the reference covers in
`rendering/paragraph_test.dart` is what these replace.
