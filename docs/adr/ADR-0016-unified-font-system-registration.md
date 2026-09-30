# ADR-0016: One shared `FontSystem` for layout and render

- **Status:** Superseded by ADR-0092 (2026-09-30), which absorbs ADR-0077: one font
  collection shared by per-realm contexts replaced the shared `FontSystem`, and the
  process-wide font system is gone
- **Date:** 2026-07-02
- **Superseded by:** [ADR-0092](ADR-0092-per-realm-text-over-parley.md), which absorbs ADR-0077

## Context

Text was measured and painted against two different `cosmic_text::FontSystem`
instances: a private singleton in `flui-painting` used by layout, and one per
`TextRenderer` in `flui-engine` used by glyph rasterization. They agreed only by
coincidence (both loaded system fonts plus Roboto). A font registered into one
was invisible to the other, so text could measure against one face and paint
with another — overflow, clipping, misplaced carets — and the layout instance
had no registration API at all, so icon and application fonts rendered as tofu.

Flutter has one font collection shared by paragraph layout and paint;
`loadFontFromList` and `FontLoader` add to it.

## Decision

**One font system, owned by `flui-painting`, is the only source of fonts for
both measuring and painting.**

- `flui_painting::shared_font_system()` returns a `SharedFontSystem` handle to
  it. `flui-painting` is the lowest crate that needs fonts, and `flui-engine`
  depends on it, so the engine reaches down rather than owning a copy.
- The font database changes after construction only through
  `FontCollection::register_font`, which loads the face here through a
  crate-internal door (`add_face`) as it adds it to the collection. It is
  append-only and bumps a generation that every shaped-text cache keys on.
- The baseline faces load at construction: host fonts plus the embedded faces
  behind `flui-painting`'s default-on `bundled-fonts` feature (Roboto, Material
  Icons, Cupertino Icons), so `Icon` renders a glyph out of the box and
  size-sensitive targets can opt out.
- The engine reads glyphs only through painting's doors
  (`TextLayout::placed_glyphs`, `SharedFontSystem::rasterize`; ADR-0065,
  ADR-0067). It names no cosmic-text type.

Registration anywhere is visible to layout and to every engine instance, in any
order — the measure/paint mismatch is excluded by construction rather than by
keeping two databases in sync.

**No longer holds for measurement or paint (ADR-0092 §10 step 4).** Layout
measures on Parley over the realm's `FontCollection` and paint draws the runs of
that layout, rasterized by the engine's `SwashRasterizer` from the faces the
runs carry, and since step 5 carets and selection read that same layout: this
font system shapes nothing and the engine reads no glyphs through it. It is
the host discovery the app's collection is fed from. The one registration
door, `FontCollection::register_font`, loads a face into the collection alone
(flui-painting `ARCHITECTURE.md`, mapping decisions 11, 15, 16 and 18). The rest of
this record stands until ADR-0092 supersedes it.

## What is changing

Today the font system is a process-wide static (`FONT_SYSTEM` in
`flui-painting`'s `text_layout`), eagerly constructed by
`SharedEngineServices::resolve()` at realm install and read ambiently on layout
paths. Apps register through the app's `FontCollection` (`flui::register_font`),
which measures, paints and places carets with the face; every realm lays out
again, on its next frame, the text it measured before the face existed
(ADR-0092 §2). Nothing adds a face to this font system after it is built.
The target is a font system per realm rather than a
process global; ADR-0077 records that together with the move to parley, and
supersedes this record when accepted. The part that carries over unchanged is
the decision above: one font source for measuring and painting.

## Alternatives rejected

- **Two font systems kept in sync.** A font registered after engine
  initialization stays invisible to paint unless every frame resyncs — new
  machinery to maintain what one instance gives for free.
- **A registry in `flui-assets` consulted by both.** Pulls an async, tokio-backed
  crate into the synchronous layout crate.
- **The shared instance in `flui-engine`.** Layout sits below the engine and
  cannot depend upward.
