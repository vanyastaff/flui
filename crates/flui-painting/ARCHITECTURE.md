# flui-painting Architecture

`flui-painting` is two things under one name: the recorder that turns a
render object's (or a `CustomPaint` painter's) drawing into a
[`DisplayList`](src/display_list/mod.rs), and the text stack that shapes an
inline span on Parley, answers layout queries on the result and hands the
engine a rasterizer for its glyphs. Nothing is rasterised here — `flui-engine`
replays the list.

The paint, style and text values (`paint`, `styling`, `typography`, plus
`Alignment`, `BoxFit` and `TextBaseline`) are owned here too (ADR-0098 §8), and
geometry comes from `flui_foundation::geometry`.
Design decisions are recorded under [Mapping decisions](#mapping-decisions).

---

## Module map

| Concern | Files | What lives there |
|---|---|---|
| Recorder | `canvas/{mod,state,transform,clipping,drawing,scoped}.rs` | `Canvas`: the `dart:ui` surface, save/restore, transforms, clips, `draw_*`, and the `with_*` helpers that pair a save with its restore |
| Wire vocabulary | `display_list/{mod,command,command_ops,paragraph}.rs` | `DisplayList` (commands + cached bounds), `DrawCommand` (the closed enum `flui-engine` matches exhaustively), `DrawCommand::bounds`, `ShapedParagraph` (the shaped text `DrawOp::Paragraph` carries, decision 18) |
| Text | `text_layout/{host,fallback_chain,fallback_tables,font_resolve}.rs`, `text_painter/{mod,measure,paint,baseline}.rs` | `TextPainter`; `HostFonts` (the one host font scan a collection is fed from), the per-platform fallback lists and the order a fed collection falls back in, family resolution |
| Per-realm text context | `text_layout/context.rs` | `FontCollection` (the app's shared, add-only fontique collection), `HostFontFeed` (the host's faces, added off the owner thread) and `TextContext` (one realm's Parley font and layout contexts over it, used through `&mut`); constructed by the runtime, one context per realm; every `TextPainter` measurement shapes on it |
| Parley shaping | `parley_text/{shape,caret}.rs` | `TextContext::shape`: a `ParagraphSpec` (styled spans, width, line height, direction, `max_lines`, ellipsis) to a `ParagraphLayout` whose `metrics()` read the laid-out lines, whose `to_shaped()` is the paragraph paint records, and whose caret, hit-test, selection, line and word queries `TextPainter` answers from (decision 15) |
| Text boundaries | `text_boundaries.rs` | Grapheme and word boundaries over ICU4X, the segmenters Parley clusters with: what hit-testing snaps to and word selection picks from, and what `flui-widgets`' editor steps through (ADR-0092 §6); each query segments from the start of its line |
| Raster side | `glyphs/{mod,key,registry,swash}.rs` | `GlyphKey` (a face named by font blob), `FontRegistry` (faces and interned variation instances), `SwashRasterizer` (the engine's atlas draws through it), `GlyphRasterizer`, `PlacedGlyph`, `GlyphImage` |
| Paint values | `paint/{style,path,shader,effects,image,clipping,blend_mode,canvas}.rs` | `Paint`, `Path` (with its shape hint), shaders, filters, images, clip and blend modes: the vocabulary the recorder records |
| Style values | `styling/*.rs`, `lerp_impls.rs` | `Color` (straight-alpha sRGB, premultiplied `lerp`), borders, radii, decorations, gradients, shadows |
| Text values | `typography/*.rs` | `TextStyle`, spans, alignment, decoration, metrics |
| Layout-facing values | `alignment.rs`, `box_fit.rs`, `text_painter/baseline.rs` | `Alignment` and its directional form, `BoxFit`/`BoxShape`/`FittedSizes`, `TextBaseline` |
| Decorations | `decoration.rs`, `table_border.rs` | `paint_box_decoration` / `box_decoration_hit_test`, `paint_table_border` |
| Test support | `testing/mod.rs` | `record` and the collection and host-scan queries tests read (`testing` feature) |

---

## The recorder

Every `draw_*` records one `DrawCommand { transform, op }` through
`Canvas::record`: the absolute transform at recording time and the
`DrawOp`. Clips are the one thing that scopes — `DrawOp::Save`/`Restore`
are unit markers the engine uses to unwind its clip stack. A `Clip::None`
clip records nothing. A `DisplayList` has no `&mut` surface: nothing outside
the crate can add, remove, or edit a command, so its cached `bounds()` is
always the union the recorder computed (`op.local_bounds()` through the
command's transform), and `Option<Rect>` because a list of only clips or
`Paint`s has no extent (which is not an empty rect at the origin).
`draw_picture` replays a list as `ctm * command.transform` per command.
The wire carries no serde ([ADR-0066](../../docs/adr/ADR-0066-display-list-command-representation.md)).

`Paint` is interned per canvas: each `draw_*` scans a small `Vec<Arc<Paint>>`
for an equal paint and shares the `Arc`. `Path` is copy-on-write
(`Arc<Vec<PathCommand>>`), so recording a caller's path copies nothing.
`DrawOp` is at most 128 bytes and `DrawCommand` 192;
`display_list_record` (criterion) measures the recording cost.

`Canvas::finish` is infallible: an unbalanced save fires a `debug_assert`
and a `tracing::warn!`, then the list ships as recorded, which is what
`PictureRecorder.endRecording()` does in release. `restore()` on an empty
stack is a silent no-op for the same reason.

---

## Text

`TextPainter` is the facade `RenderParagraph` drives. Every measurement takes
the `TextContext` it shapes through: `layout`, the four intrinsics, `dry_size`
and `dry_baseline` each take `&mut TextContext`, and a render object lends its
realm's (decision 14). `layout` shapes the paragraph once on Parley
(`TextContext::shape`: a `ParagraphSpec` with the painter's spans, scale,
width, `max_lines` and ellipsis), reads size and baselines from the
`ParagraphLayout`'s `metrics()`, and turns the same layout into the
`ShapedParagraph` it paints (`ParagraphLayout::to_shaped`). Intrinsic widths
come from a second shape without truncation (`content_widths`, decision 9).
The painter's cache keys on the context's collection and its
`FontCollection::generation`, so a layout from another realm's collection, or
from before a registration, shapes again. The app's collection is fed from one
scan of the host's fonts (`HostFonts`): its faces, the generics the bundled
faces leave unbound, and FLUI's fallback lists for the host (decision 17); a
style's family is resolved by one rule (decision 8).

`TextPainter::paint` records `DrawOp::Paragraph { paragraph, offset, color }`
with the very `Arc<ShapedParagraph>` its cache holds (ADR-0065, ADR-0092 §4):
the engine rasterises those runs and shapes nothing. The root colour rides on
the command; a span's own colour is carried by its runs, so a span recolour is
a layout change and a root recolour is not. A `ShapedParagraph` names no
shaper: its text, size, baselines and ink bounds, a face table of
`FontBlob`s (the font bytes and the blob id keys name them by), and runs of
glyph ids and logical positions with each run's face, size, variation
coordinates, synthesis and colour (decision 18). Truncation keeps the lines
the metrics keep; an ellipsis is shaped into the last kept line, text dropped
from its end until the line and the ellipsis fit the width.

Carets, selection boxes, word boundaries, line metrics and hit-testing read
the same `ParagraphLayout`, which the cache keeps beside the paragraph
(`parley_text/caret.rs`, decision 15): a query never shapes. `HostFonts` lays
nothing out: it is one fontdb scan of the host with the generic families and
fallback lists picked for it, a value the app's shared engine services build
and drop once `FontCollection::with_host_fonts` has fed the collection from it
(decision 17). No font state is process-global. A registration loads the
collection (decision 11). The embedded baseline faces (`fonts.rs`,
`bundled-fonts`) are in every collection, and bind its generics (decision 16).

The raster side is `glyphs` (ADR-0092 §5). What crosses to the engine is the
`ShapedParagraph`; the engine's atlas registers each run's face in its
`FontRegistry` (`FontRegistry::prepare_run`, which also interns the run's
variation coordinates) and places the run's glyphs with
`ShapedRun::placed_glyphs(key, origin, scale)`: `PlacedGlyph { key: GlyphKey,
x, y, color }` in device pixels, the pen split into a whole column and a
quarter-pixel bin, the baseline row `round(baseline × scale)`, as cosmic-text
placed glyphs. The atlas draws through the `GlyphRasterizer` trait: a key type
plus `rasterize(&mut self, key)`, deterministic per key, with `None` for a key
the rasterizer cannot draw (the atlas does not place it and asks again next
use). `SwashRasterizer` implements it with `GlyphKey`: blob id and face index,
glyph id, exact size bits, an interned variation instance, a horizontal
quarter-pixel bin, hinting and synthesis. A `VariationId` carries the random
identity of the `FontRegistry` that minted it, so a rasterizer over another
registry refuses it instead of drawing its own instance at the same index. It
drives swash with the sources, format and offsets cosmic-text's rasterizer
used, so for the same face bytes, glyph, size and bin it draws the bitmap
cosmic-text drew; `tests/parley_oracle.rs` checks that bit for bit on
Parley-shaped Latin, Cyrillic and Greek (Roboto) and icon glyphs (Material
Icons), against bitmaps recorded from cosmic-text before it was removed
(`tests/support/raster_recorded.rs`), on vendored faces only so the result
does not depend on the host. Complex scripts
and colour emoji have no vendored face and are not compared there. The key has
no vertical bin because the placement truncates a glyph's row before binning,
so its vertical bin is always zero. A registry holds every blob a key names,
so a key stays valid after fontique's source cache drops the file
(`a_held_blob_keeps_its_keys_across_a_prune`, `src/text_layout/context.rs`).

---

## Thread safety

`#![forbid(unsafe_code)]`. Every type is plain `Send + Sync` value data;
`Canvas` and `TextPainter` are mutated through `&mut self` by one owner.
The crate takes no lock of its own: its `clippy.toml` disallows `Mutex` and
`RwLock`, with no `#[expect]`ed site, and it declares no process-global font
state (`cargo xtask globals`).

The Parley path takes no FLUI lock. A `TextContext` is `Send` and used
through `&mut` by the realm that owns it (flui-rendering lends it to one
measurement at a time), so two realms shape at the same time
(`tests/text_context.rs`, `two_realms_shape_in_parallel`). The
`FontCollection` they share is fontique's shared mode: a registration takes
fontique's mutex and bumps a version, and each context re-reads the collection
under that mutex once, on its next shape; otherwise a shape costs one atomic
load (ADR-0092 §3 cites the fontique lines). `SwashRasterizer` takes no lock
either: it owns its `FontRegistry` and scaler, is `Send`, and is used through
`&mut` by whoever owns the atlas, so it can rasterize on another thread while
shaping continues. A `ShapedParagraph` is immutable and `Send + Sync`; its
`FontBlob`s share the font bytes with fontique's cache and the registries that
hold them.

---

## Mapping decisions

### 1. Closed `DrawCommand` enum, matched exhaustively

`DrawCommand` has one variant per paint operation and no `#[non_exhaustive]`:
the engine's `dispatch_command` has no wildcard arm, so a new variant is a
compile error there until its `render_*` arm exists. That is the whole
contract — a producer-side variant count would only prove the producer
updated its own test. A trait-object command was rejected for the same
reason `flui_layer::Layer` is an enum: a `Box<dyn Drawable>` is a boundary
the GPU backend cannot translate.

### 2. `Canvas` keeps the `dart:ui` surface, and only that

`Canvas` is user-facing through `CustomPaint`, so `dart:ui`'s methods stay
even where nothing in the workspace calls them today (`drawPoints`,
`drawColor`, `restoreToCount`, `skew`, `drawPicture`, …). What is NOT `dart:ui`
and had no consumer is gone: the closure helpers beyond `with_transform` /
`with_clip_*` / `with_blend_mode` / `with_opacity`, multi-canvas composition,
clip-bounds queries (whose implementation answered for the last clip only
and ignored `ClipOp::Difference`), and `draw_shader_mask` /
`draw_backdrop_filter` with their `DrawCommand` variants — masks and backdrop
filters are layers (`flui_layer::{ShaderMaskLayer, BackdropFilterLayer}`),
and the command-level copies had no producer and a second engine lowering.

### 3. No `DisplayList` mutation, no analysis layer, no trait pair

`DisplayList` exposes `iter`/`commands`/`len`/`is_empty`/`bounds`/`append`
as inherent methods. The sealed `DisplayListCore` + blanket `DisplayListExt`
pair had one implementor and no generic consumer — every import was there
to call `.len()` on a concrete list — and the filter/count/`stats`/
`with_opacity`/`apply_transform`/`filter`/`map`/`IndexMut` layer had no
consumer at all and broke the "immutable after recording" claim.

### 4. No `PaintingBinding`, no `ImageCache`, no `ClipContext`

`PaintingBinding` owned an image cache nothing read (the live decode cache
is `flui_widgets::image::decode_cache`) and a font-change notifier nothing
listened to; its one live accessor reached the process-wide font system,
which is gone (ADR-0092 §10 step 6a), and a registration goes through
`FontCollection::register_font`. `ClipContext` had no production implementor.

### 5. `Canvas::finish(self) -> DisplayList` stays infallible

Returning `Result` would put a `?` on every paint call site to report a
programmer error that is only a debug-time check; `save_count()` is there
for a caller that wants to check.

### 6. The zero-area background guard sits on the FILL, not on a caller

**Choice:** `paint_box_decoration` records no background — colour or gradient — when either
dimension of its rect is exactly zero. Border, shadow and image passes stay outside the guard.

**Why on the fill, not the class:** `ColoredBox` realizes as a `RenderDecoratedBox` with a
colour-only `BoxDecoration`, so one painter serves both widgets and a class-level guard would have
to pick one of the two to cover.

**Effect:** a zero-area `DecoratedBox` records no background. Identical pixels — a zero-area fill
rasterizes nothing either way — and one fewer display-list command.

**Alternatives:**
- Guard the whole decoration at zero size — rejected, and this is the edge case a class-level guard
  loses: a border on a degenerate box still draws lines, and a shadow
  still has a silhouette. Skipping the decoration wholesale would drop them silently.
- Give `ColoredBox` its own render object so each class can carry its own guard — rejected as
  duplication for one boolean. `RenderColoredBox` exists but is `Leaf` arity, so it cannot carry
  the child a `ColoredBox` has.
- Signed comparison (`<= 0`) instead of `== 0` — rejected. A rect whose min exceeds its max is
  INVERTED, not empty, and this module deliberately normalizes those through `shortest_side`'s
  `.abs()`; the signed test swallows that whole case.

**Unasserted:** no test pins this.


### 7. `anti_alias` is a paint OPTION, not a `BoxDecoration` field

**Choice:** `paint_box_decoration` takes a `DecorationPaintOptions` alongside the decoration, and
`RenderDecoratedBox` owns the flag. `BoxDecoration` is untouched.

**Why:** a decoration DESCRIBES an appearance and is `serde`-serializable here, while
anti-aliasing is a rasterization-quality hint that belongs to whoever is doing the rasterizing.
Serializing a rendering hint beside colours and radii would make the style data carry something
that is not style.

**Scope, stated:** the flag reaches the SOLID COLOUR fill and nothing else. Borders, shadows and
images keep the default because a `ColoredBox` has none of them, so nothing says
what they should do when it is off. Gradient backgrounds keep it for the same reason: a
`ColoredBox` has no gradient either, so the only reachable gradient is a `DecoratedBox`'s. (They COULD carry it — a gradient is a shader on an ordinary
fill paint since ADR-0066 — and deliberately do not.) `DecorationPaintOptions` is
`#[non_exhaustive]`, so growing it is additive if a consumer appears.

**It reaches the GPU, and that took wiring.** `Paint::anti_alias` had existed as metadata for a
long time with nothing reading it: `DrawBatcher`'s rect path never looked, and
`rect_instanced.wgsl` always applied `sdfToAlpha`, so a rect drawn with the flag off rasterized
identically. The batcher now marks the instance and the shader takes coverage from `step(dist, 0)`
instead of the smoothstep ramp. The flag rides in lane 1 of the existing `clip_kind` attribute —
no new vertex attribute, no stride change — with `0` meaning anti-aliased, because every
construction site zeroes that attribute and the opposite polarity would have silently turned AA
off everywhere. `ClippableInstance::with_clip` assigns that attribute LANE BY LANE for the same
reason: it used to write the whole vector, which dropped the paint's bit, and did so silently.

**Alternatives:**
- `BoxDecoration::anti_alias` — rejected per the above. It is additive (`#[non_exhaustive]`) and
  would have avoided the signature change, which is exactly why it was tempting.
- A bare `bool` fourth parameter — rejected for boolean blindness at the call site:
  `paint_box_decoration(c, r, d, false)` does not say false what.
- A second `paint_box_decoration_aliased` entry point — rejected; two names for one operation, and
  it does not extend to the next option.

**Unasserted:** no test pins this.

**Test-support note:** the harness's paint summary
(`flui_rendering::testing::snapshot::summarize_paint`) now prints ` aliased` for a paint that opted
out, and nothing for the default. Printing it unconditionally would have added a constant to every
line of every snapshot and changed all of them at once; printing only the deviation keeps existing
snapshots untouched while making the opt-out visible to any test reading those lines.


### 8. A style's font family is resolved against the collection before it reaches the shaper

**Choice:** [`src/text_layout/font_resolve.rs`](src/text_layout/font_resolve.rs) picks the family a
`TextStyle` is shaped with (`resolve_family_name`), instead of handing `style.font_family` to
Parley unchanged: the style's family when the collection holds it (spelled exactly), else the
first entry of `font_family_fallback` that is a generic or is held, else the sans-serif generic.
Parley is handed that one family and nothing after it.

This exists because an unresolvable family lets an emoji face shape the **space** of an ordinary
Latin run at roughly 1.24 em instead of 0.25 (issue #927). A family the collection lacks sends
every cluster down the collection's fallback order; on a unix host whose first listed family
present is its emoji face (the unix common list ends in `"Noto Color Emoji"`), the letters, absent
from that face, fall through to the sans-serif family while the space, present in it, stays.
Handing Parley a held family or a generic closes it. cosmic-text, which FLUI shaped with until
ADR-0092 §10 step 5, reached the same face by a second route (an exact-weight filter that emptied
its candidate lists into an emoji-first tail) and needed a scanned emoji-forbidden fallback and a
weight snap besides; both left with it. Parley matches a weight within the family and synthesizes
a bold the family lacks (decisions 10 and 18).

The generic families a degraded style lands on are bound to a face that can set Latin text: with
`bundled-fonts` to Roboto (decision 16), and on a host scan to the first family of the platform's
common list the host carries that maps `'A'` and `' '` (`bind_generic_families`), so an emoji,
symbols or icon face never becomes a generic.

**Accepted trade-off — fallback is per style, not per glyph.** A per-glyph fallback chain would
consult each family when a glyph is missing from a higher-priority one. The rule hands Parley exactly
one family (`FontFamily::Single`), so that is not expressed. What is expressed is the per-*style* chain:
resolution walks `TextStyle::font_family_fallback` in order and takes the first entry the collection
holds, degrading to the sans-serif generic only when none of them is present. The residual
limit is precisely locatable — a family that is present
but lacks the glyph still stops the walk: `font_family:
"CupertinoIcons", font_family_fallback: ["Noto Sans"]` on Latin text renders tofu,
because `CupertinoIcons` IS held. Closing that needs per-run family splitting above
the style, which is tracked separately.

Locked by the table `family_resolution` (`src/text_layout/context.rs`). Its row
`a_style_resolves_by_the_family_rule` pins the rule's answers, the limit above among them in both
directions: `"Material Icons"` with `["Roboto"]` stops on the present icon family (a walk "fixed" to
skip a present family fails it, and so does per-glyph fallback landing, the signal to retire this
record), and an ABSENT primary with the same chain reaches `Roboto`, so the stop is about presence
and not about the chain being unread. Its row `a_missing_family_never_takes_its_space_from_an_emoji_face`
shapes `"Ao Bo"` styled `CupertinoSystemText` on Parley over a collection fed from a host whose
fallback order puts an emoji face first, and asserts letters and space in one face with the space
under half an em; handing Parley the unresolved name turns it red (the space lands in the decoy at
1.3 em). It is hermetic, because the face it needs — one carrying `' '` and no letters, with "Emoji"
in the PostScript name — is *generated*, not borrowed: `tools/decoy-face/generate.py` writes
`decoy-wide-space.ttf`, whose space advance is fixed at 1.3 em by construction.

The same generator supplies the probe faces (`probe-mono-{100,600}.ttf`, `probe-sans-400.ttf`,
`probe-variable-wght.ttf`, `probe-arabic-ligature.ttf`), because every shipped font asset is a
single-weight, non-monospaced, static face; registration, generic-binding, host-feed and caret
tests load them.


### 9. Intrinsic width probes skip `max_lines` truncation, floor at ellipsis

**Choice:** [`TextPainter`](src/text_painter/measure.rs) min/max intrinsic width probes
(`layout()` cache fill and the uncached getters) shape with
`LineOverflow::IgnoreForWidthIntrinsic`, so `max_lines` truncation does not reach
`TextContext::shape`. When `max_lines` and a non-empty ellipsis are both set, the
probe then floors at the shaped ellipsis width (`ellipsis_width_floor`) — truncating
layouts may commit an ellipsis-only buffer once the text prefix is exhausted. Committed
`layout`, `dry_size`, `intrinsic_height`, and `dry_baseline` use `LineOverflow::Enforce`.

**Why:** At `max_width = 0`, soft wrap produces many visual lines; enforcing `max_lines` then
truncates toward an empty prefix and reports `min_intrinsic_width == 0` for non-empty text
(#1085). Dropping the ellipsis from that probe entirely under-reports when the ellipsis is
wider than the text's narrowest run. Intrinsics measure shaped runs / wrap opportunities,
with the ellipsis as a lower bound when truncation can leave only that glyph string.

**Alternatives:**
- Require exact intrinsic/`maxLines` equality with the truncated layout — rejected; the
  contract is unresolved.
- Derive min intrinsic from break opportunities without a zero-width layout — deferred; the
  overflow-free probe plus ellipsis floor restores the documented contract without a second
  shaping pipeline for the main text.
- Keep ellipsis inside the zero-width truncating probe — rejected; that reintroduces the
  empty-prefix collapse for ordinary `max_lines` without a wide ellipsis.

**Accepted trade-off:** `max_lines` does not shrink min/max intrinsic *width* below the
shaped content (or the ellipsis floor). Parents that need truncated size use dry layout /
committed layout. The ellipsis floor is the width of the ellipsis-only paragraph truncation
can keep, shaped in the first run's style as `ellipsize` shapes it; locked by
`wide_ellipsis_floors_min_intrinsic_width` and
`a_rich_span_ellipsis_floors_min_intrinsic_width` (`tests/text_painter_unit.rs`). For `max_lines` without an ellipsis, and for the `RenderParagraph`
intrinsics: **Unasserted:** no test pins this.

### 10. Synthetic bold uses an interpolated stroke width

**Rule:** a face with no bold weight is emboldened at raster time when the
style asks for bold. cosmic-text has no fake bold at all (its swash call sets
no `embolden`, only a 14° skew for `FAKE_ITALIC`), so there is no in-repo
reference.

**Choice:** `SwashRasterizer` grows the
outline by `size × ratio` in total, the ratio interpolated linearly from 1/24
at 9 px to 1/32 at 36 px and clamped outside. The constants are the
interpolation `SkScalerContext` uses (`kStdFakeBoldInterpKeys` `{9, 36}`,
`kStdFakeBoldInterpValues` `{1/24, 1/32}`), recalled from Skia source and not
checked against a clone. Skia's FreeType backend may embolden in the font host
instead, with a different strength, so the result is not known to match Skia's on Android and Linux. swash moves each point by its strength on each side,
so the strength passed is half the width.

**Why:** the growth stays a moderate share of the stem at large sizes, where
the prototype's `size / 24` per side doubled it, and the curve is small enough
to replace once a reference is checked.

**Alternatives:** FreeType's `FT_GlyphSlot_Embolden` (about `size / 24` in
total at every size), a candidate once Skia's per-platform output is
measured; no fake bold, as cosmic-text does — rejected, a bold style on a
regular-only face would draw regular.

**Accepted trade-off:** since paint draws Parley's runs (decision 18), bold on
a face with no bold weight paints emboldened: default-family, "Roboto" and
generic bold on the bundled Roboto Regular (decision 16), which cosmic-text
painted as Regular. The strength is not yet checked against Skia's source per
platform. Locked by `synthetic_bold_adds_the_interpolated_width` (width gain at
9, 20, 36 and 144 px) and `synthetic_bold_inks_more_than_regular` (a
readback, `flui-engine`).

### 11. The font collection is app-scoped and passed explicitly; each realm shapes through its own context

**Rule:** the Parley path has one `FontCollection` per app, and each realm
shapes through a `TextContext` of its own built from it. A face registered on
the collection reaches every context built from it, including ones built
before the registration. The collection offers no removal. This crate provides
both types; the runtime constructs them (the app's shared engine services hold
the collection, and each realm owns a context built in its constructor,
ADR-0092 §10 step 2). Layout, intrinsic and dry queries measure through the
realm's context (step 3, decision 14); layout measures on it (step 4a).

**Why:** FLUI runs several realms on their own threads (ADR-0027, ADR-0091).
An ambient collection behind one lock makes every realm's shaping wait on the
others, which is what the cosmic-text path's `FONT_SYSTEM` did until ADR-0092
§10 step 6a removed it, and is process-global state ADR-0097 retires. Passing the collection keeps it out of
any `static`; a context per realm keeps shaping lock-free. Removal is left out
because a glyph key names its face by blob and must not outlive it
(ADR-0092 §2).

**Accepted trade-off:** a registration makes each realm deep-copy the
collection's data once, on its next shape, and `register_font` itself clones
fontique's local collection data to get the `&mut` its registration takes,
rather than holding a FLUI lock; both are accepted because registration is
rare. The collection is the one registration door, and a registered face
reaches the collection alone: measurement, paint and carets read the one
layout shaped on it, so one registration reaches all three at the next layout,
and the host scan a collection was fed from is a value nothing reads again. The
collection judges the bytes on a scratch fontique collection before the shared
registration, which bumps fontique's version even for bytes with no family, so
a refused registration (no face, or a face with no `cmap`) changes nothing.
`FontCollection::check_font` gives the same verdict with no collection at
all, for the app to answer a registration made before its first window.
Locked by `two_realms_shape_in_parallel` and
`a_face_registered_after_the_fork_shapes_in_every_realm`
(`tests/text_context.rs`), and `registration_contract`
(`src/text_layout/context.rs`).


### 12. `TextDirection` sets line alignment on the Parley path, not the base direction

**Rule:** `ParagraphSpec::direction` aligns lines: `Ltr` to the left edge,
`Rtl` to the right. The bidi base direction is Parley's own, taken from the
paragraph's first strong character, so Latin-first text under `Rtl` is still
ordered as an LTR paragraph, and Hebrew-first text under `Ltr` is ordered RTL.

**Why:** Parley 0.11.1 has no way to set it: its analysis calls the bidi
resolver with `None` for the base level (`analysis/mod.rs:539-546`), and
neither the builder nor the layout exposes one. Right alignment is the part
of `Rtl` that can be honoured today.

**Accepted trade-off:** a right-to-left paragraph whose text starts with Latin
or neutrals lays out its runs in the wrong order until the base direction can
be set. Setting it needs a leading directional mark and an offset map through
spans, the ellipsis cut and every caret query, a step of its own after
ADR-0092 §10 step 5. Measurement, paint and carets read one layout, so they
agree on the order they have (decision 15). This is a divergence of the
default build from Flutter's. Locked by
`rtl_aligns_lines_right_without_setting_the_base_direction`
(`src/parley_text/shape.rs`).

### 13. `Color::lerp` interpolates premultiplied

Interpolating each straight-alpha channel on its own would darken a colour faded to
transparent (transparent *black*): red to transparent would pass through
`(128, 0, 0, 128)`. `Color::lerp` weights each channel by its
endpoint's alpha and divides by the mixed alpha, as CSS Color 4 does, so the
same fade stays red: `(255, 0, 0, 128)`. Between two opaque colours the result
is the plain per-channel lerp. When the mixed alpha is zero there is nothing to weight by and the
channels interpolate straight, which keeps both endpoints exact. Everything that
lerps a colour inherits it: border sides, shadows, decorations and gradient
stops. Locked by `lerp_to_transparent_keeps_the_hue`
(`tests/color_property.rs`).

### 14. `TextPainter` measures through the context it is given

**Rule:** every measuring method of `TextPainter` takes `&mut TextContext`;
there is no ambient collection to fall back on. A render object lends its
realm's context (`ctx.text()` in flui-rendering), and the painter's cache is
keyed on that context's collection and generation as well as the
constraints.

**Why:** a realm owns its text context (decision 11), and a realm's layout
must measure with it rather than with whichever context is ambient. Passing it
makes the realm visible in every signature that measures, which is what keeps
two realms' layouts apart (ADR-0092 §3). Keying the cache on the collection
closes the case a single ambient collection never had: one painter measured
through two collections.

**Accepted trade-off:** every signature that measures names the context, even
where only one realm exists. The context is shaped on in every build (ADR-0092
§10 step 4a). Locked by `measurement_follows_the_context_it_is_given`,
`intrinsic_widths_follow_the_context_they_are_asked_through` and
`a_registration_on_the_collection_invalidates_the_painter_cache`
(`tests/text_painter_unit.rs`, rows of `text_context_contract`), and at the
realm level by `a_realm_measures_text_with_the_faces_of_its_own_collection`
(`crates/flui-runtime/src/ui_realm/tests/text_context.rs`).

### 15. Carets, selection and hit-testing read the layout that measured

**Rule:** size, baselines, intrinsic widths, the painted glyphs, line
metrics, carets, selection boxes, word boundaries and hit-testing all come
from one Parley layout on the realm's context. `TextPainter`'s cache keeps
the `ParagraphLayout` beside the paragraph it paints, and the queries
(`parley_text/caret.rs`) answer in the painted box's coordinates: each
cluster edge takes the same per-line shift `to_shaped` gives the line's
glyphs (`ParagraphLayout::line_shift`), so a caret sits on the glyph it
follows under `Rtl` and on a line narrower than the width it broke at.
Parley metrics are unquantized, so a baseline reaches the device grid once,
when its glyphs are placed (`round(baseline × scale)`). Parley's width
excludes trailing whitespace, and so does a line's `width` in the line
metrics; Flutter's max intrinsic width includes it (recalled, not checked
against a clone).

The painter shapes with:

- `OverflowWrap::BreakWord`, so a word wider than the line breaks between its
  glyphs while the min-content width stays the widest word;
- an explicit size on every styled run, the default 14 px where no ancestor
  sets one, so a run's letter spacing and line height apply at the size it
  shapes at (`effective_style`);
- `max_lines` of zero as no limit (`TextPainter::set_max_lines`,
  `ParagraphSpec::max_lines`).

An empty paragraph measures one line of its style from the font: the line box
and the baseline a line of text in that style has (13.19 px at 14 px Roboto).
An empty span shapes no run, so the root's style, scaled as a run's is, is the
Parley paragraph's default style.

How the queries answer, and where that differs from Flutter (Flutter's
behaviour recalled from its `TextPainter` and SkParagraph, not checked
against a clone):

- **A caret is per scalar.** Parley splits a cluster of several scalars (a
  combining mark, a ZWJ sequence, a ligature) into one cluster per scalar,
  sharing the advance evenly, and `get_offset_for_caret` answers each scalar
  offset with its own edge: a proportional slice of the grapheme. Flutter
  snaps a caret to grapheme edges. FLUI does not, because a text store answers
  an input method's rect queries per scalar and platform selection is exact
  ([ADR-0090](../../docs/adr/ADR-0090-ime-pull-text-store-contract.md);
  flui-widgets mapping decision 35).
- **A hit snaps to a grapheme.** `get_position_for_offset` takes the cluster
  under the point, the edge the point is nearer, and then the nearer of the
  enclosing ICU4X grapheme's two edges on the hit line, so a tap never lands
  between `e` and its accent or between CR and LF, as in Flutter. A hit past
  a hard break's cluster answers its start, since the position after it is on
  the next line.
- **Selection boxes follow bidi runs.** `get_boxes_for_selection` gives one
  box per stretch of adjacent clusters of one direction on one line, in visual
  order, each carrying its run's direction, as Flutter does; a hard break gets
  no box. A box is as tall as the line box, where Flutter's default
  `BoxHeightStyle.tight` uses the run's ascent and descent; FLUI has no box
  height parameter.
- **Affinity picks the side.** At a soft wrap `Downstream` puts the caret at
  the next line's start and `Upstream` at the previous line's end, as in
  Flutter; across a bidi run boundary it picks the run. After a hard break, a
  trailing one included, the caret starts the next line whatever the
  affinity.
- **Truncated text keeps its queries in the kept text.** An offset in dropped
  lines or in an appended ellipsis answers the kept text's end, and a hit
  never answers an offset past it; the line metrics list only kept lines.
  Flutter's paragraph can place a caret in the ellipsis.
- **Line metrics** say where each kept line is painted in the paragraph's own
  box: `left` is the line's first painted x (so a short line aligned right
  starts right of 0), `hard_break` is true on a line that ends at an explicit
  break or at the end of the paragraph, as Flutter's `LineMetrics.hardBreak`
  documents, and false on the last kept line of truncated text. Flutter's
  `TextPainter.computeLineMetrics` also shifts them by the painter's paint
  offset; FLUI's `get_line_metrics` does not, where its carets and boxes do.
  (These two checked against `dart:ui`'s `text.dart` and the framework's
  `text_painter.dart` on flutter/flutter `master`.)
- **Word boundaries** come from ICU4X's word segmenter for non-complex
  scripts over the kept text, with FLUI's tie-break (a word beats whitespace
  on either side; between two words the following one wins). The layout's
  cluster flags come from the same segmenter but hold a space where the text
  has the CR of a CR LF (decision 18), so they are not read. Parley's
  `complex-scripts` feature stays off: without dictionary or LSTM data, CJK
  and Thai word selection is per character, where Flutter uses dictionaries.
- **The base direction** is still the first strong character's (decision
  12).
- **A CR before an LF** is shaped as a space (decision 18), so the rect of
  that one scalar is a space wide; only an exact input-method selection can
  land between CR and LF.

**Why:** a second shaper for carets was a second layout that could disagree
with the painted glyphs, and it did on multi-line text: the cosmic-text
layout compared global byte offsets with glyph offsets counted per buffer
line, so `"ab\ncd"` gave the selection `3..5` no box. One layout makes that
disagreement impossible.

**Accepted trade-offs:** a painter keeps the whole Parley layout (runs,
clusters, glyphs) in its cache beside the shaped paragraph, text that never
gets a cursor query included: with the paragraph, about 66 bytes per char of
a 570-char paragraph (ADR-0092 gate 6, `benches/text_startup.rs`). The first cursor query places the kept lines' clusters once and
keeps them with the layout; a later query walks them without allocating, in
time linear in the paragraph's length. Editable text is short, and a query
never shapes.

Locked by the table `caret_contract` (`tests/main.rs`, rows in
`tests/caret_contract.rs`): `a_combining_mark_is_one_hit_target` and
`a_zwj_family_is_one_hit_target` (per-scalar carets, grapheme hits),
`rtl_paragraph_carets_run_right_to_left`,
`mixed_bidi_boxes_carry_their_run_direction`,
`a_trailing_newline_puts_the_caret_on_the_empty_line`,
`crlf_is_one_break_for_carets`, `multi_line_selection_boxes_follow_their_line`,
`carets_sit_on_the_painted_glyphs`, `a_soft_wrap_caret_follows_its_affinity`,
`truncated_carets_stay_in_kept_lines`,
`truncated_text_without_an_ellipsis_stays_in_its_kept_line`,
`line_metrics_index_each_line`, `caret_position` and
`two_space_run_word_boundary`; by `word_boundaries_agree_with_the_layouts_clusters`
(`src/parley_text/caret.rs`); by
`a_lam_alef_ligature_is_one_glyph_and_two_caret_stops` (decision 19); and, as
pixels, by `selection_highlights_the_second_line` in flui-engine's
`parley_runs_read_back`. `parley_metrics_round_to_todays_baseline`
(`tests/parley_metrics_oracle.rs`) pins width, height and the painted
baseline row against the numbers the cosmic-text layout measured, recorded;
`measured_lines_are_painted_lines` pins that the measured height is the
painted paragraph's; `an_empty_paragraph_measures_a_line_of_its_style`
(`text_contract`, `tests/main.rs`) pins the empty paragraph;
`a_face_registered_on_the_collection_reaches_measurement_paint_and_carets`
(`tests/font_registration.rs`) pins that one registration moves all three.

### 16. With `bundled-fonts`, every generic family a host feed leaves bound stays Roboto

**Rule:** with `bundled-fonts`, every `FontCollection` holds the bundled
Roboto and binds sans-serif, serif, cursive, fantasy, monospace and system-ui
to it. A host feed (decision 17) binds only a generic the collection leaves
unbound, so it rebinds none of these, and never adds a host face of a family
the collection holds, so a host "Roboto" never joins the bundled one. Text
whose style names no family, names "Roboto" or names a generic is measured,
painted and given carets in the bundled Roboto Regular on every host; paint
synthesizes a bold weight on it (decisions 10 and 18). Without
`bundled-fonts` an unbound generic takes the host scan's pick (decision 8).

**Flutter:** the default family is the platform's (Segoe UI on Windows, the
system font on Apple platforms, Roboto on Android). Recalled, not checked
against a clone.

**Why:** every collection binds its generics to the bundled Roboto, the
standalone and bundled-only ones included. Were the host-fed collection's
generics the host's, default text would measure in Roboto on a bundled-only
collection and in Segoe UI, Arial or DejaVu on the app's; bound to Roboto,
default text is the same face on every host and in every collection.

**Accepted trade-off:** on every desktop host:

- default text is Roboto; an app that wants the host's face names the family,
  which the fed collection measures in that face (decision 17);
- a bold weight in the default family, in "Roboto" or in a generic paints as
  Regular synthetically emboldened, because only Regular is bundled
  (decision 10); a medium weight paints as Regular. Parley measures those runs
  with Regular's advances;
- `monospace` is the proportional Roboto, so `RenderErrorBox` and
  `flui-material`'s error style lose their fixed pitch.

Locked by the default-family, monospace and bold rows of
`parley_metrics_round_to_todays_baseline` (`tests/parley_metrics_oracle.rs`),
which fail when a row measures in another face than the bundled Roboto
Regular; by `a_host_copy_of_a_bundled_family_is_not_fed`
(`src/text_layout/context.rs`), which feeds a host "Roboto" at 400 and 700 and
fails when the feed stops skipping a held family (bold Roboto then measures in
the host face); and by `a_missing_path_is_skipped_and_the_feed_completes`
(same file), which fails when the feed rebinds a bound generic.

### 17. The app's collection is fed from one host scan, with FLUI's fallback lists

**Rule:** the app's `FontCollection` is fed from one `HostFonts::scan`, once
per app: flui-app's shared engine services build it with the bundled faces
(`FontCollection::with_host_feed`) and run the returned `HostFontFeed` on a
thread of its own, which scans and feeds what `FontCollection::with_host_fonts`
would, then raises the generation once if that changed the collection, and on
unwind (ADR-0092 §7; `a_feed_that_adds_nothing_leaves_the_generation_alone`).
Each source is read once on a scratch collection before it is registered, so
a file that panics on every read is skipped; a file replaced between that
read and the registration is not covered. The scan is fontdb's: the platform's font
directories, and fontconfig's configuration where there is one. The
collection holds the bundled faces, then every face the scan found whose
family it does not already hold, read from the same files. A generic it
leaves unbound takes the scan's pick for it, system-ui taking sans-serif's
(decision 16). A style's family is resolved by one rule, `resolve_family_name`
(decision 8), over the families the collection holds, spelled exactly:
fontdb matches names exactly, so the rule narrows fontique's
case-insensitive lookup (`holds_exactly`), and `"segoe ui"` degrades to the
sans-serif generic. Past that family the collection walks one fallback order,
`FallbackChain`: each script's list for the host platform, then the
platform's common list, then the sans-serif generic's family, and the common
list is the emoji generic. The lists are FLUI's (`fallback_tables`, per
platform, keyed by ISO 15924 code, ported from cosmic-text 0.19 and checked
against lists recorded from it); the host locale (`sys-locale`) picks the Han
list. `FontCollection::new()` stays bundled-only, for standalone contexts,
tests and the hot-reload plugin, and falls back to Roboto for every script.

**Why:** the app's text must find the host's faces. Over a bundled-only
collection, text the bundled faces do not cover had no face: `你好世界 emoji
😀` at 16 px measured 82.77 px where the host's faces give 134.01 px on
Windows, and Cupertino's chain, which the host resolves to Segoe UI, measured
in Roboto (163.29 px against 161.16 px). fontique's own `system` scan reaches
`windows`, which tier S forbids, and a hand-rolled scan would save the 6 ms
of a 43 ms startup cost that is mostly the feed (ADR-0092, gate 6), so the
scan is fontdb's. Flutter's engine collection resolves through the platform
font manager (recalled, not checked); FLUI keeps one layout for measurement,
paint and carets, so one set of lists, FLUI's, decides the face everywhere.

**Accepted trade-off:**

- fontique has no walk over every other face: a character neither the
  script's list, the common list nor the sans-serif family covers measures
  and paints as notdef. The host oracle skips such text.
- Family names match exactly, where CSS matches them without regard to case:
  a style must spell a family as the fonts name it.
- Parley appends the Han fallback to every cluster's fallback families
  (fontique `Query::set_fallbacks`), so such a character may measure in the Han
  fallback face.
- Only each script's default key is set, with no locale: the Parley path
  passes none. A change that passes one must set locale keys too.
- The Han list is picked for the whole process from the host locale, by its
  language, script and region subtags: `ja-JP` gets the Japanese faces,
  `zh-Hant` and `zh-TW` Taiwan's, `zh-HK` and `zh-MO` Hong Kong's, and every
  other locale the Simplified Chinese ones. cosmic-text matched the tag whole,
  so a host reporting `ja-JP` (every real one: `sys_locale` gives a region)
  got Simplified Chinese glyph forms; FLUI does not keep that. A document's
  own language does not pick the list.
- Android's platform common list is empty, so the collection falls back to the
  sans-serif family alone there; unverified, since Android is clippy-only here.
- A family name fontdb records in another language only (fontique keeps the
  English or first name) is not found by the family rule's exact match.
- A host copy of a bundled family (Roboto, Material Icons, CupertinoIcons) is
  not fed, so one family never mixes two copies; an app that wants a host's
  own icon font registers it under another family name.
- The first frame renders with the bundled faces; the host's arrive when
  the feed lands, and text measured before is laid out again then, as after
  a registration. A family the collection holds when the feed starts is not
  fed; one the app registers while it runs may also take the host's faces
  of that name.
- The feed adds one file per registration, so fontique's lock is held for
  one file at a time, and reads each source on a scratch collection inside
  `catch_unwind` first: a source that panics or holds no family is skipped
  instead of poisoning the shared lock. The generation rises once when the
  feed ends, even on unwind.
- A face registered after the feed reaches the collection alone, through
  `FontCollection::register_font` (decision 11).

Locked by `measured_width_equals_painted_width_on_host_faces` and
`every_family_the_host_scan_finds_resolves_in_the_collection`
(`tests/host_faces_oracle.rs`, host-dependent: a row whose text no listed
family covers is skipped, the Latin rows, a mis-cased family and a family the
host names exactly among them, never are),
`fontique_fallbacks_follow_the_chain_in_order`,
`the_sans_serif_family_ends_every_script_fallback`,
`the_emoji_generic_is_the_common_list`,
`platform_tables_match_the_recorded_lists` and
`a_platform_chain_picks_han_by_locale` (`src/text_layout/fallback_chain.rs`),
`a_glyph_the_named_family_lacks_measures_in_roboto_on_the_bundled_collection`
(`src/parley_text/shape.rs`),
the table `family_resolution` (`src/text_layout/context.rs`: the rule's
answers over a collection, and the emoji face a host fallback order puts
first kept out of a Latin run),
`a_missing_path_is_skipped_and_the_feed_completes` and
`a_source_that_panics_is_skipped_and_the_collection_stays_usable`
(`src/text_layout/context.rs`), and the table `host_feed_contract`
(`tests/font_registration.rs`: one generation step per feed, and no host
face added to a family held when the feed starts).

### 18. Text crosses the display list as `ShapedParagraph`, with its own face table

**Rule:** `DrawOp::Paragraph` carries an `Arc<ShapedParagraph>` built from the
Parley layout that measured the paragraph. Each run names its face by an
index into the paragraph's own table of `FontFace`s, each a `FontBlob` (the
font bytes, shared, and the blob id a `GlyphKey` names them by) and a face
index. Glyphs are ids with logical positions; the subpixel bin, the device row
and the raster size are computed at placement (`ShapedRun::placed_glyphs`),
under the transform the paragraph is replayed with. Variation coordinates
travel raw and are interned by the raster side's registry
(`FontRegistry::prepare_run`), whose `VariationId`s mean nothing to another
registry. Within its box each line starts at the left edge under `Ltr` and
ends at the right edge under `Rtl`; the box is as wide as the measured width,
and the paint offset places it within the width it was laid out at.

**Why:** a retained layer replays a picture recorded frames earlier, so a
table of the blobs this frame's recorders named would miss a replayed
paragraph's faces; a paragraph that carries its faces is complete by
construction, for the cost of one shared handle per distinct face per
paragraph. Holding the handle also keeps fontique's weakly cached blob, and so
its id, alive while any paragraph or registry names it. The bin and the device
row depend on the device transform, which only the replay knows. Parley aligns
a line within the width it was broken at; the painter's box is the measured
width, so lines are re-aligned in it, or an `Rtl` paragraph would be shifted
twice.

**Accepted trade-off:** paint follows Parley where it differs from the
cosmic-text paint it replaced:

- Synthetic bold reaches paint (decision 10): bold on the bundled Roboto,
  which has only Regular, paints emboldened, where cosmic-text painted
  Regular (`synthetic_bold_inks_more_than_regular`).
- Hard breaks are Parley's: a trailing `\n` or U+2029 adds an empty line
  (`"A\n"` is two lines, the owner's choice; `a_trailing_newline_is_a_line`);
  U+2028 and `\r` break (`a_line_separator_breaks`,
  `latin_breaks_at_a_line_separator`, `cjk_breaks_at_a_line_separator`,
  `colour_emoji_on_line_two`); CR LF breaks once, as `\n` does, so
  `"A\r\nB"` is two lines: Parley would break at the CR and again at the LF,
  so the painter hands it a CR directly before an LF as a space, the CR's
  one byte, and every byte offset into the layout still indexes the text
  (`crlf_breaks`, `crlf_puts_b_on_line_two`; the owner's decision, ADR-0092
  §10 step 4);
  U+0085 does not break (`next_line_does_not_break`).
- Fallback is per cluster over the collection's fallback families, where
  cosmic-text walked per word, so a mixed-script run can pick another face.
- An `Rtl` line ends at its box's right edge
  (`arabic_rtl_right_aligns_each_line`).
- Snapshots print the requested weight, not the weight cosmic-text snapped
  to: a W500 or W700 run on the bundled Roboto reads `weight=500` or
  `weight=700`.
- The ellipsis is shaped on Parley and measured: text is dropped until the
  last kept line with the ellipsis fits the width, so the painted line no
  longer overhangs the measured width
  (`truncated_paragraph_paints_what_it_measured`, `tests/text_overflow_unit.rs`).

The raster side's registry is append-only and keys a face by its blob's id,
not its bytes: every `FontCollection` build and every `register_font` wraps
the bytes in a new blob, so registering the same file again, or building a
second collection over the bundled faces, adds one more face to each
painter's registry and rasterizes its glyphs again under new keys; neither is
released while the painter lives. The growth is bounded by how often an app
registers or builds a collection, not by frames.

The ink bounds a damage extent is built from grow by the synthetic bold and
oblique the rasterizer applies (`paragraph_extent_covers_every_rasterized_glyph`,
`tests/damage_extent.rs`, whose oracle is the rasterizer itself). The baseline
rule is pinned against cosmic-text's rows by
`parley_metrics_round_to_todays_baseline` and read back by
`a_2x_baseline_row`; the readback rows are `parley_runs_read_back`
(`flui-engine`, `paragraph_readback_tests.rs`).

### 19. A ligature is one glyph and one caret stop per scalar

**Rule:** a ligature the face forms (lam + alef under `rlig`, `fi` under
`liga`) is painted as the one glyph the face gives and measured by that
glyph's advance. Carets stay per scalar (decision 15): the caret between two
of its components sits inside the ligature, the ligature's advance shared
evenly between them, and a hit inside it answers the nearest of those stops.

**Why:** the face decides what a ligature looks like and how wide it is, so
the measured width is the painted width only if both read the shaped glyph.
A caret is placed between scalars the user edits, and a lam-alef is two
letters to the user, so the ligature cannot be one stop; an even split is the
only position the face gives no reason to prefer against. Parley reports a
ligature's components as clusters of their own, sharing its advance, and
`caret.rs` places those clusters; a cluster that still spans several scalars
is sliced in proportion to its bytes (`Placed::x_at`).

**Accepted trade-off:** the removed text spike measured its `arabic_mixed`
corpus as 406 glyphs on Parley and 407 on cosmic-text (ADR-0092 gate 2). Both
differences are recorded here, not shaping defects:

- The count is face choice. cosmic-text's sans-serif named Open Sans, which
  the Windows host lacked, and fell through to Segoe UI; fontique's named
  Arial. Segoe UI draws lam-alef as two glyphs, Arial, Tahoma and Times New
  Roman as one ligature; on one face both shapers count the same. In FLUI
  the face comes from the family rule and the fallback tables, and
  measurement and paint read one layout.
- On a ligating face cosmic-text tagged the ligature glyph with the lam's
  byte and Parley tags it with the alef's cluster, the lam a component
  cluster with no glyph of its own. Carets read clusters, not glyphs, so both
  stops remain.

Locked by `a_lam_alef_ligature_is_one_glyph_and_two_caret_stops`
(`tests/caret_contract.rs`), on `FLUI Probe Arabic`
(`assets/fonts/probe-arabic-ligature.ttf`, generated by
`tools/decoy-face/generate.py`): four glyphs, the four advances measured,
the middle caret at the midpoint, each quarter of the ligature answering its
nearest stop.

---

## Open items

- **`Save`/`Restore` carry a transform nobody reads.** Every command is
  stamped, the markers included; a marker-only shape would save 64 bytes
  per scope at the cost of a second command type on the wire.

### Font fixtures and distribution provenance

Font-selection regressions use a generated FLUI Probe Sans family instead of a
restricted third-party test font. Its static, non-monospaced W400/Normal metadata
keeps the fallback tie with Roboto intact, and explicit `A`, `B`, `o`, and space
coverage gives the `Ao Bo` probe real glyph IDs and positive advances. Empty
outlines are sufficient for shaping metrics; no rasterization claim is made.
No test loads it: the load-order regression it was made for is **Unasserted:** no test
pins this. The xtask font gate's self-test still uses its bytes and its generator entry.

`assets/fonts/inventory.toml` binds each font to reviewed bytes, source provenance,
and complete local license/attribution texts. The font asset gate discovers
unlisted fonts, checks hashes/notices, regenerates all fixtures into a temporary
directory, and checks Cargo package file selection. This is a Rust test-fixture
and packaging decision; it changes no shaping contract. It also
does not solve registry dependency cycles or certify complete release archives.
