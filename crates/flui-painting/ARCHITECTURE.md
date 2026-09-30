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
| Text | `text_layout/{layout,font_resolve}.rs`, `text_painter/{mod,measure,paint,baseline}.rs` | `TextPainter`; the process-wide font system and `SharedFontSystem`, `TextLayout` (the caret, hit-test and line queries until ADR-0092 §10 step 5), family resolution against the host |
| Per-realm text context | `text_layout/context.rs` | `FontCollection` (the app's shared, add-only fontique collection) and `TextContext` (one realm's Parley font and layout contexts over it, used through `&mut`); constructed by the runtime, one context per realm; every `TextPainter` measurement shapes on it |
| Parley shaping | `parley_text/shape.rs` | `TextContext::shape`: a `ParagraphSpec` (styled spans, width, line height, direction, `max_lines`, ellipsis) to a `ParagraphLayout` whose `metrics()` read the laid-out lines and whose `to_shaped()` is the paragraph paint records |
| Raster side | `glyphs/{mod,key,registry,swash}.rs` | `GlyphKey` (a face named by font blob), `FontRegistry` (faces and interned variation instances), `SwashRasterizer` (the engine's atlas draws through it), `GlyphRasterizer`, `PlacedGlyph`, `GlyphImage` |
| Paint values | `paint/{style,path,shader,effects,image,clipping,blend_mode,canvas}.rs` | `Paint`, `Path` (with its shape hint), shaders, filters, images, clip and blend modes: the vocabulary the recorder records |
| Style values | `styling/*.rs`, `lerp_impls.rs` | `Color` (straight-alpha sRGB, premultiplied `lerp`), borders, radii, decorations, gradients, shadows |
| Text values | `typography/*.rs` | `TextStyle`, spans, alignment, decoration, metrics |
| Layout-facing values | `alignment.rs`, `box_fit.rs`, `text_painter/baseline.rs` | `Alignment` and its directional form, `BoxFit`/`BoxShape`/`FittedSizes`, `TextBaseline` |
| Decorations | `decoration.rs`, `table_border.rs` | `paint_box_decoration` / `box_decoration_hit_test`, `paint_table_border` |
| Test support | `testing/mod.rs`, `text_layout::init_font_system_with_faces` | `record` (`testing` feature); pinning the font system to a known face set |

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
from before a registration, shapes again. The app's collection is fed from the
process font system's faces, generics and fallback order, and both shapers
resolve a family by one rule (decision 17).

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

Carets, selection, word boundaries, line metrics and hit-testing still read a
cosmic-text `TextLayout` (`TextLayout::from_spans`, shaped through the
process-wide font system, family and weight resolved against the host
database first, decision 8), built on the first such query and kept while the
process font system's generation is unchanged (decision 15). Plain `Text`
never builds one. The font system is a `OnceLock<Arc<Mutex<FontState>>>`, an
ambient residual (a process-global the runtime still reaches); `AppRuntime`
installs it at realm install. `SharedFontSystem` has two public doors:
`shape(|Shaper| …)` resolves and shapes under one acquisition and never bumps
the generation; `generation()` is what the caret layout keys on. The one
mutation, append-only and bumping the generation, is crate-internal
(`add_face`): an app registers through `FontCollection::register_font` on a
collection built by `with_host_faces`, which loads the face into the
collection (measurement and paint) and here (carets) alike (decision 11). The
`testing` feature keeps `SharedFontSystem::register_font` as a caret-only
door for tests. The collection is fed from its discovery (decision 17). The
embedded baseline faces (`fonts.rs`, `bundled-fonts`) are installed at
construction.

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
used, so for the same face bytes, glyph, size and bin the two draw identical
bitmaps; `tests/parley_oracle.rs` checks that bit for bit on Parley-shaped
Latin, Cyrillic and Greek (Roboto) and icon glyphs (Material Icons), on
vendored faces only so the result does not depend on the host. Complex scripts
and colour emoji have no vendored face and are not compared there. The key has
no vertical bin because the placement truncates a glyph's row before binning,
so its vertical bin is always zero. A registry holds every blob a key names,
so a key stays valid after fontique's source cache drops the file
(`a_held_blob_keeps_its_keys_across_a_prune`, `src/text_layout/context.rs`).

---

## Thread safety

`#![forbid(unsafe_code)]`. Every type is plain `Send + Sync` value data;
`Canvas` and `TextPainter` are mutated through `&mut self` by one owner.
The one lock is the cosmic-text path's font system, which carets, selection
and host discovery still take, never nested with another lock in this crate;
neither measurement, paint nor rasterization takes it. The crate's
`clippy.toml` disallows `Mutex` and `RwLock`; that font system is the one
`#[expect]`ed site (`text_layout/layout.rs`), and it leaves at ADR-0092 §10
step 6.

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
which `shared_font_system()` now names directly, and a registration goes
through `FontCollection::register_font`. `ClipContext` had no production implementor.

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


### 8. A style's font family is resolved against the host before it reaches the shaper

**Choice:** [`src/text_layout/font_resolve.rs`](src/text_layout/font_resolve.rs) picks the family a
`TextStyle` is shaped with, instead of handing `style.font_family` to cosmic-text unchanged. A named
family the font database does not carry degrades to `Family::SansSerif`, and the five generic family
names are pointed at families the database actually carries when their configured targets are
missing (`FontSystem::new` hard-codes sans-serif to *Open Sans*, which a stock Debian/Ubuntu desktop
does not install).

This exists because an unresolvable family lets an emoji face shape the **space** of an ordinary
Latin run at roughly 1.24 em instead of 0.25. Shaping runs per word, which is what lets the letters
and the space diverge: the letters are absent from an emoji face and move on, the space is present
in it and stays. Two independent routes reach that face, and closing only one leaves the defect
live — cosmic-text's unix `common_fallback()` list *ends* in `"Noto Color Emoji"`, so the walk
reaches it at **any** weight (400 included, measured) when no earlier text family from that list is
installed; and `default_font_match_key`'s candidate filter
`font_weight_diff == 0 || variable_weight_match` empties every list for a family shipping only 400
and 700, dropping the run into an unfiltered tail whose derived ordering puts emoji faces first.
(There is an `|| is_mono` term in cosmic-text, but it lives in `next_item`'s
`font_match_keys_iter`, and `is_mono` there is `default_families[i] == &Family::Monospace` — a
property of the *request*, not of any face. Reading it as a face property is what made an earlier
revision of `family_accepts_weight` accept any monospaced face at any weight.) Naming a family the database carries forecloses both,
because `Database::query`'s front-insert puts the CSS-matched face ahead of the emoji entry in each.

The requested *weight* is resolved alongside the family, snapped to one the resolved family can
serve (`snap_weight`), and the two travel as one value (`ResolvedFont`) so no caller can pair a
resolved family with the style's original weight. Both halves of that sentence are corrections of
earlier decisions recorded here, and both were wrong for reasons worth keeping:

- Snapping was first *rejected* as "worse than doing nothing", on the grounds that
  `Database::query` already applies CSS font matching and that a snap strips the requested instance
  off a variable face. The first is true and irrelevant: `query` picks the best face *within* a
  family, while the abandonment happens a layer up, in `default_font_match_key`, whose empty result
  makes `next_item` leave the family altogether. The second is why `family_accepts_weight` probes
  the variable `wght` axis before it snaps, and snaps only when no face — static or variable — can
  serve the request.
- The snap then ran on measurement only. `SharedFontSystem` exposed the family without its weight,
  so the raster path read `style.font_weight` directly and shaped a string in a different font from
  the one it was measured in. Making the pair the only obtainable value is the fix; a second
  accessor returning just the family would let the same defect back in.

**Alternatives:**
A custom `Fallback` impl whose `forbidden_fallback()` excludes emoji families is complementary
rather than competing, and now ships (`EmojiForbiddenFallback`): it suppresses emoji faces in the
unfiltered tail, the path reached when neither the resolved family nor the script list can serve a
word. It was first deferred here for wanting "a per-platform emoji family list and its own red
test". Neither obstacle survived contact: the trait returns `&[&'static str]` borrowed from `&self`,
so the list is *scanned* from the host database using cosmic-text's own emoji predicate
(`post_script_name.contains("Emoji")`, the same one that produces its `not_emoji` sort key) rather
than guessed per platform; and the red test is hermetic on the generated decoy face.

Two properties of that impl are load-bearing and easy to get wrong. It **extends** the platform's
forbidden list rather than replacing it — macOS's is `[".LastResort"]`, and dropping that entry
would let the system tofu face win a fallback on the one platform CI never executes. And it
**declines to forbid anything** where `common_fallback()` is empty — Android and wasm, per
`font/fallback/other.rs` — because there the unfiltered tail is the only route to any fallback face,
so forbidding emoji families would not redirect a Latin run, it would make emoji unrenderable. That
is a runtime check on the platform list, not a `cfg`: the question is "is there another route", and
a future target answers it without being enumerated.

**Accepted trade-off — fallback is per style, not per glyph.** A per-glyph fallback chain would
consult each family when a glyph is missing from a higher-priority one. `Attrs::family` holds exactly
one family, so that cannot be expressed. What is expressed is the per-*style* chain:
resolution walks `TextStyle::font_family_fallback` in order and takes the first entry the host
carries, degrading to the sans-serif generic only when none of them is present. The residual
limit is precisely locatable — a family that is present
but lacks the glyph still stops the walk: `font_family:
"CupertinoIcons", font_family_fallback: ["Noto Sans"]` on Latin text renders tofu,
because `CupertinoIcons` IS installed. Closing that needs per-run family splitting above
`Attrs`, which is tracked separately.

Pinned by `a_present_but_narrow_family_stops_the_chain_without_per_glyph_fallback`, which
guards the limit in both directions: it fails if the walk is "fixed" to skip a present family
(that would be a different guess, not per-glyph fallback), and it fails again if per-glyph fallback
ever lands — which is the signal to retire this record rather than let it go stale. Its control
asserts that an ABSENT primary still reaches the chain, so the stop is about presence and not about
the chain being unread.
Replacement coverage, per rule #1. Which family a run shapes in, in both fixture load orders so that
no load order satisfies it, is **Unasserted:** no test pins this.
`oversized_space_from_an_emoji_face_is_closed` observes the letters and the space landing apart: it
asserts the red state (space above 1 em, on a different face from the letters) before asserting the
fix. It is hermetic,
because the face it needs — one carrying `' '` and no letters, with "Emoji" in the PostScript name
so cosmic-text classifies it as one — is *generated*, not borrowed:
`tools/decoy-face/generate.py` writes `decoy-wide-space.ttf`, whose space advance is fixed at 1.3 em
by construction. It used to build the fixture from the host's emoji font, which made a
merge-blocking assertion depend on a distro package's metrics and needed a `FLUI_REQUIRE_EMOJI_FONT`
CI variable to keep the absent-font skip branch honest; both the package install and the variable
are gone, because the skip branch is.

The same generator supplies the fixtures for the weight probes, and for the same reason — every
shipped font asset is a single-weight, non-monospaced, static face, so three arms of the resolution
were untestable against it. `probe-mono-{100,600}.ttf` is one monospaced family at two weights (the
`face.monospaced` arm, and the CSS-versus-nearest tie-break, where 100 and 600 disagree at a W500
request); `probe-variable-wght.ttf` carries an `fvar` `wght` axis spanning 100..900 over a
`usWeightClass` of 400 (the variable-weight arm). **Unasserted:** no test pins this.


### 9. Intrinsic width probes skip `max_lines` truncation, floor at ellipsis

**Choice:** [`TextPainter`](src/text_painter/measure.rs) min/max intrinsic width probes
(`layout()` cache fill and the uncached getters) shape with
`LineOverflow::IgnoreForWidthIntrinsic`, so `max_lines` truncation does not reach
`TextLayout::from_spans`. When `max_lines` and a non-empty ellipsis are both set, the
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
committed layout. The ellipsis floor is locked by `wide_ellipsis_floors_min_intrinsic_width`
(`tests/text_painter_unit.rs`). For `max_lines` without an ellipsis, and for the `RenderParagraph`
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
others, which is what the cosmic-text path's `FONT_SYSTEM` does today, and is
process-global state ADR-0097 retires. Passing the collection keeps it out of
any `static`; a context per realm keeps shaping lock-free. Removal is left out
because a glyph key names its face by blob and must not outlive it
(ADR-0092 §2).

**Accepted trade-off:** a registration makes each realm deep-copy the
collection's data once, on its next shape, and `register_font` itself clones
fontique's local collection data to get the `&mut` its registration takes,
rather than holding a FLUI lock; both are accepted because registration is
rare. Until ADR-0092 §10 step 6 the bundled faces sit in both this collection
and the cosmic-text font system, and so does every registered face: the
collection is the one registration door, and one built by `with_host_faces`
keeps that font system as its caret side and loads each face there first, so
one registration reaches measurement, paint and carets at the next layout. The
collection judges the bytes on a scratch fontique collection before the caret
side loads anything, so bytes one side reads and the other does not (a face
with no `cmap`: fontdb loads it, fontique finds no family) change neither.
`FontCollection::check_font` gives the same verdict with no collection at
all, for the app to answer a registration made before its first window.
`FontCollection::new` has
no caret side and never touches the process font system. Locked by
`two_realms_shape_in_parallel` and
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
be set; that belongs to ADR-0092 §10 step 5, where editable text moves to
Parley and its acceptance covers LTR, RTL and mixed bidi. The painted runs
come from the layout that measured, so measurement and paint agree; the caret
layout is cosmic-text's (decision 15), shaped separately: line widths do not
depend on run order, so only a line break that falls differently shows, and
then the caret layout's line count can differ from the painted one. This is a
divergence of the default build from Flutter's. Locked by
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

### 15. Carets and selection use a different shaper until ADR-0092 §10 step 5

**Rule:** size, baselines, intrinsic widths and the painted glyphs come from
one Parley layout on the realm's context; line metrics, carets, selection,
word boundaries and hit-testing still come from a cosmic-text `TextLayout` of
the same spans, built on the first such query. Parley metrics are
unquantized, as cosmic-text's are, so a baseline reaches the device grid once,
when its glyphs are placed (`round(baseline × scale)`). Parley's width
excludes trailing whitespace; cosmic-text's includes it, and so does
Flutter's max intrinsic width (recalled, not checked against a clone).

The painter hands both shapers inputs they read alike:

- a word wider than the line breaks between its glyphs: Parley shapes with
  `OverflowWrap::BreakWord`, cosmic-text's buffers wrap `WordOrGlyph`.
  `BreakWord` rather than `Anywhere`, so the min-content width stays the
  widest word;
- every styled run carries an explicit size, the default 14 px where no
  ancestor sets one, because cosmic-text applies a run's letter spacing and
  line height only at a size the run carries while Parley applies them at the
  inherited one (`effective_style`);
- `max_lines` of zero is no limit (`TextPainter::set_max_lines`,
  `ParagraphSpec::max_lines`), as cosmic-text always read it.

An empty paragraph measures one line of its style from the font: the line box
and the baseline a line of text in that style has (13.19 px at 14 px Roboto),
where cosmic-text put that baseline mid-line (8.40 px), which left an empty
`Text` in a baseline-aligned row off its neighbours' baseline. An empty span
shapes no run, so the root's style, scaled as a run's is, is the Parley
paragraph's default style: its family, weight and line height shape the empty
line as they would a line of text.

**Why:** carets and selection move to Parley's clusters with gate 3's LTR,
RTL and mixed-bidi tests (ADR-0092 §10 step 5); until then the caret queries
keep the layout they were tested on.

**Accepted trade-off:** where the two shapers read a paragraph differently,
the caret layout and the painted glyphs disagree:

- Hard breaks (decision 18): after a trailing `\n` or U+2029 the painted
  paragraph has an empty last line, and the caret sits at the end of the line
  before it instead of on it; after U+2028 or a lone `\r` the painted text
  continues on a line the caret layout does not have, and at U+0085 or
  U+001C–U+001E the caret layout breaks where paint does not. An editable
  paragraph's caret and selection can then be a line away from its glyphs.
- A caret after trailing whitespace in `EditableText` can sit past the
  measured width, because the caret layout counts the whitespace.
- A face the two shapers resolve differently shapes the carets in another
  face. Over the app's collection, fed from the host (decision 17), that is
  only the residue decision 17 lists. A bundled-only collection
  (`FontCollection::new()`, standalone contexts and the hot-reload plugin)
  measures and paints every family it lacks, and every glyph a family it holds
  lacks, in Roboto.
- A face loaded on the process font system alone (the `testing` door
  `SharedFontSystem::register_font`) reaches the caret layout but neither
  measurement nor paint; an app registers through its collection, which
  reaches all three (decision 11).
- A line narrower than a space's advance (2 px at 12–17 px) takes each space
  of a glyph-broken paragraph onto a line of its own in the caret layout and
  hangs it on Parley (`"ab cd"` at 0 px: 4 lines painted, 5 in the caret
  layout).
- An `Rtl` paragraph's lines can break differently in the two (decision 12).
- A truncated paragraph's caret layout drops text by cosmic-text's rule, one
  character at a time until its ellipsis fits, which can keep a character
  more or less than the painted line.

On the same face the two agree: `parley_metrics_round_to_todays_baseline`
(`tests/parley_metrics_oracle.rs`) pins equal width and height between the
measurement and the cosmic-text layout, and the device row the painted runs
put the first baseline on against cosmic-text's, for the bundled Roboto by
name, as the default family and as the monospace generic (decision 16), at
13–32 px, default and 1.5 line height, scales 1–2.
`measured_lines_are_painted_lines` pins that the measured height is the
painted paragraph's for wrapped paragraphs, an overlong word, hard breaks,
spacing and height set without a size, and zero `max_lines`;
`an_empty_paragraph_measures_a_line_of_its_style` (`text_contract`,
`tests/main.rs`) pins the empty paragraph;
`measured_width_equals_painted_width_on_host_faces`
(`tests/host_faces_oracle.rs`) pins the caret layout's size against the
measurement on the host's faces.
`a_face_registered_on_the_collection_reaches_measurement_paint_and_carets`
(`tests/font_registration.rs`) pins that one registration moves all three.

### 16. With `bundled-fonts`, the process font system's generic families bind to Roboto

**Rule:** with `bundled-fonts`, the process font system installs the bundled
Roboto on every host, in place of any host face named "Roboto", and binds
sans-serif, serif, cursive, fantasy and monospace to it before the host
generics are bound (`fonts::bind_generics_to_bundled`), as every
`FontCollection` does; the app's collection, fed from the host, binds its
generics to the families the process font system binds them to, so Roboto
there too (decision 17). In the caret layout, a weight Roboto lacks snaps to
the Regular it has, the monospace generic included (`font_resolve::snap_weight`).
Text whose style names no family, names "Roboto" or names a generic is
measured, painted and given carets in the bundled Roboto Regular on every host;
paint synthesizes a bold weight on it instead of snapping (decisions 10 and 18).

**Flutter:** the default family is the platform's (Segoe UI on Windows, the
system font on Apple platforms, Roboto on Android). Recalled, not checked
against a clone.

**Why:** every collection binds its generics to the bundled Roboto, the
standalone and bundled-only ones included, and carets are shaped on the
process font system until ADR-0092 §10 step 5. Bound to a host face, default
text would be measured and painted in Roboto on a bundled-only collection and
its carets placed in Segoe UI, Arial or DejaVu. The host-fed collection (decision 17) would agree with either
binding; default text stays the same face on every host.

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
which fail without the binding (the bold monospace rows also without the
monospace snap), and by `a_host_copy_does_not_replace_a_bundled_face`
(`src/fonts.rs`), which fails when a host Roboto, Material Icons or
CupertinoIcons keeps its place.

### 17. The collection mirrors the process font system's faces, generics and fallback order

**Rule:** the app's `FontCollection` is built with
`FontCollection::with_host_faces` over the process font system, once per app
(flui-app's shared engine services). It holds the bundled faces, then every
face the process font system holds whose family it does not already hold, read
from the same files (or shared from the same in-memory fonts). Its generic
families name the families the process font system binds them to, system-ui
naming sans-serif's: with `bundled-fonts`, Roboto (decision 16). Both
shapers resolve a style's family by one rule,
`resolve_family_name` (decision 8's rule, over the families each side holds),
and Parley is handed that one family. A side holds a family only when spelled
exactly as its fonts name it: fontdb matches exactly, so the Parley side
narrows fontique's case-insensitive lookup (`holds_exactly`), and `"segoe ui"`
degrades to the sans-serif generic on both. Past it both walk one fallback
order, `FallbackChain`, built once beside the process font system: the font
system is constructed over it, and the collection gets each script's list, then
the common list, then the sans-serif generic's family as that script's fallback
families, and the common list as the emoji generic. `FontCollection::new()`
stays bundled-only, for standalone contexts, tests and the hot-reload plugin,
and falls back to Roboto for every script.

**Why:** measurement and paint (Parley over the collection) and the caret
layout (cosmic-text over the process font system, decision 15) must pick the
same face for the same text. Over a
bundled-only collection, text the bundled faces do not cover measured in
another face than it painted in: `你好世界 emoji 😀` at 16 px measured 82.77 px
and painted 134.01 px on Windows, and Cupertino's chain, which the host
resolves to Segoe UI, measured in Roboto (163.29 px against 161.16 px). The
feed reads the process font system's discovery rather than scanning again
through fontique's `system` feature, which reaches `windows` and is forbidden
at tier S (ADR-0092 §10 step 5). Flutter's engine collection resolves through
the platform font manager (recalled, not checked); FLUI has two shapers until
ADR-0092 §10 step 6, so it mirrors one into the other instead.

**Accepted trade-off:**

- cosmic-text's last resort, any face not forbidden, has no Parley
  counterpart beyond the trailing sans-serif family: a character neither the
  script's list, the common list nor that family covers measures and paints as
  notdef, while the caret layout shapes it in whatever face that walk finds.
  The oracle skips such text.
- Family names match exactly on both sides, where CSS matches them without
  regard to case: a style must spell a family as the fonts name it.
- Parley appends the Han fallback to every cluster's fallback families
  (fontique `Query::set_fallbacks`), so such a character may measure in the Han
  fallback face.
- cosmic-text falls back per word, Parley per cluster; in a word whose
  characters only partly fall back, the two can split it differently.
- Only each script's default key is set, with no locale: the Parley path
  passes none. A change that passes one must set locale keys too.
- Android's platform common list is empty, so the collection falls back to the
  sans-serif family alone there while the caret layout still walks its last
  resort;
  unverified, since Android is clippy-only here.
- Two scans decide what is carried: a file that disappears between them is
  carried on the caret side and absent from the collection, and a family name
  fontdb records in another language only (fontique keeps the English or first
  name) resolves on the caret side alone.
- A host copy of a bundled family (Roboto, Material Icons, CupertinoIcons) is
  not fed, so one family never mixes two copies. With `bundled-fonts` the
  process font system replaces every host face of those families with the
  bundled one (`fonts::install_bundled`), so both sides measure and paint the
  bundled copy; an app that wants a host's own icon font registers it under
  another family name.
- The feed reads the host's font files a second time before the first frame
  (about 35 ms over 76 families on the Windows development host), until
  ADR-0092 §10 step 3b's event lets it run off the owner thread.
- A face registered after the feed reaches the collection and the process
  font system together, through `FontCollection::register_font` (decision 11).

Locked by `measured_width_equals_painted_width_on_host_faces` and
`every_family_the_process_font_system_carries_resolves_in_the_collection`
(`tests/host_faces_oracle.rs`, host-dependent: a row whose text only
cosmic-text's last resort reaches is skipped, the Latin rows, a mis-cased
family and a family the host names exactly among them, never are),
`fontique_fallbacks_follow_the_paint_chain_in_order`,
`the_sans_serif_family_ends_every_script_fallback` and
`the_emoji_generic_is_the_common_list` (`src/text_layout/fallback_chain.rs`),
`a_glyph_the_named_family_lacks_measures_in_roboto_on_the_bundled_collection`
(`src/parley_text/shape.rs`),
the row `the_collection_resolves_the_family_the_font_system_does` of
`family_resolution_contract` (`src/text_layout/font_resolve.rs`), and
`a_missing_path_is_skipped_and_the_feed_completes`
(`src/text_layout/context.rs`).

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
second collection over the bundled faces (the performance overlay's
`WgpuPainter::draw_label` builds its own), adds one more face to each
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
