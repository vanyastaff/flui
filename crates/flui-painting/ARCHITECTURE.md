# flui-painting Architecture

`flui-painting` is two things under one name: the recorder that turns a
render object's (or a `CustomPaint` painter's) drawing into a
[`DisplayList`](src/display_list/mod.rs), and the text stack that shapes an
inline span through cosmic-text and answers layout queries on the result.
Nothing is rasterised here — `flui-engine` replays the list.

The reference is `dart:ui`'s `Canvas`/`Paint`/`Path` vocabulary and
Flutter's `TextPainter`; the paint vocabulary itself (`Paint`, `Shader`,
`BlendMode`, `Path`, geometry) is defined in `flui-types` and re-exported.
Divergences from Flutter are recorded under [Mapping decisions](#mapping-decisions).

---

## Module map

| Concern | Files | What lives there |
|---|---|---|
| Recorder | `canvas/{mod,state,transform,clipping,drawing,scoped}.rs` | `Canvas`: the `dart:ui` surface, save/restore, transforms, clips, `draw_*`, and the `with_*` helpers that pair a save with its restore |
| Wire vocabulary | `display_list/{mod,command,command_ops}.rs` | `DisplayList` (commands + cached bounds), `DrawCommand` (the closed enum `flui-engine` matches exhaustively), `DrawCommand::bounds` |
| Text | `text_layout/{layout,font_resolve}.rs`, `text_painter/{mod,measure,paint,baseline}.rs` | The process-wide font system and `SharedFontSystem`, `TextLayout` (shape, truncate, caret/hit-test/line queries), family resolution against the host, `TextPainter` |
| Parley raster side | `parley_text/{key,registry,swash}.rs` (`parley` feature) | `ParleyGlyphKey` (a face named by font blob), `FontRegistry` (faces and interned variation instances), `SwashRasterizer`; no production caller until ADR-0092 §10 step 3 |
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
`DrawOp` is at most 128 bytes and `DrawCommand` 192, pinned by
`draw_command_fits_its_budget`; `display_list_record` (criterion) measures
the recording cost.

`Canvas::finish` is infallible: an unbalanced save fires a `debug_assert`
and a `tracing::warn!`, then the list ships as recorded, which is what
`PictureRecorder.endRecording()` does in release. `restore()` on an empty
stack is a silent no-op for the same reason.

---

## Text

`TextLayout::from_spans` shapes a paragraph through the process-wide font
system: family and weight are resolved against the host database first
(decision 8), the buffer is shaped, and `max_lines`/`ellipsis` truncation
RE-SHAPES the kept prefix so size, line metrics, and glyphs agree.
`TextPainter` is the Flutter-shaped facade over it that `RenderParagraph`
drives; its intrinsic-width probes shape without truncation (decision 9).
`TextPainter::paint` records `DrawCommand::Paragraph { layout, offset,
color }` with the very `Arc<TextLayout>` its cache holds (ADR-0065): the
engine rasterises what was measured and shapes nothing. The root colour
rides on the command; a span's own colour is baked into the layout, so a
span recolour is a layout change and a root recolour is not.

The font system is a `OnceLock<Arc<Mutex<FontState>>>`, an ambient residual
(a process-global the runtime still reaches); `AppRuntime` installs it at realm
install so first use is not whichever text measurement runs first.
`SharedFontSystem` is the handle the engine's glyph atlas rasterises from
(ADR-0016), so a face registered through `register_font` measures and
paints alike. It has four doors: `shape(|Shaper| …)` resolves and shapes
under one acquisition and never bumps the generation; `register_font` is
the only mutation, append-only, and bumps it; `generation()` is what every
shaped-text cache keys on (ADR-0065); `rasterize(GlyphKey)` returns one
glyph's bitmap for the engine's atlas (ADR-0067). The embedded baseline
faces (`fonts.rs`, `bundled-fonts`) are installed at construction, so a
headless test measures an `Icon` in the face the app paints.

What crosses to the engine is `TextLayout::placed_glyphs(origin, scale)` —
an iterator of `PlacedGlyph { key: GlyphKey, x, y, color }` in device
pixels — and the `GlyphImage` that `rasterize` returns for a key. The shaped
`cosmic_text::Buffer` never leaves this crate, and `GlyphKey` is opaque:
the one cosmic-text type on the public surface is `Family`, carried by
`ResolvedFont`.

The engine's atlas draws through the `GlyphRasterizer` trait (`glyphs.rs`):
a key type plus `rasterize(&mut self, key)`, deterministic per key, with
`None` for a key the rasterizer cannot draw (the atlas does not place it and
asks again next use). `SharedFontSystem` implements it with `GlyphKey`, and
is the engine's default. `parley_text::SwashRasterizer` implements it with
`ParleyGlyphKey`, the key ADR-0092 §5 names: blob id and face index, glyph
id, exact size bits, an interned variation instance, a horizontal
quarter-pixel bin, hinting and synthesis. It drives the same swash scaler
as the cosmic-text path with the same sources, format and offsets, so for
the same face bytes, glyph, size and bin the two draw identical bitmaps;
`tests/parley_oracle.rs` checks that bit for bit on Parley-shaped Latin,
host complex scripts and colour emoji. The key has no vertical bin because
cosmic-text truncates a glyph's row before binning, so its vertical bin is
always zero.

---

## Thread safety

`#![forbid(unsafe_code)]`. Every type is plain `Send + Sync` value data;
`Canvas` and `TextPainter` are mutated through `&mut self` by one owner.
The one lock is the font system's, never nested with another lock in this
crate, and never held across a call into another crate except the engine's
`with_mut` closure, which takes no painting lock. `SwashRasterizer` takes no
lock at all: it owns its `FontRegistry` and scaler, is `Send`, and is used
through `&mut` by whoever owns the atlas, so it can rasterize on another
thread while shaping continues.

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
which `shared_font_system()` now names directly, and `register_font` lives
on `SharedFontSystem`. `ClipContext` had no production implementor.

### 5. `Canvas::finish(self) -> DisplayList` stays infallible

Returning `Result` would put a `?` on every paint call site to report a
programmer error Flutter also reports only in debug; `save_count()` is there
for a caller that wants to check.

### 6. The zero-area background guard sits on the FILL, not on a caller

**Rule:** [`AGENTS.md`](../../AGENTS.md) Design stance ("Flutter is a reference, not a spec") — behaviour is the floor, and an
improvement over the reference owes a named record plus a replacement test.

**Choice:** `paint_box_decoration` records no background — colour or gradient — when either
dimension of its rect is exactly zero. Border, shadow and image passes stay outside the guard.

**Why not where Flutter puts it:** Flutter guards this on the CLASS. `_RenderColoredBox.paint`
(`widgets/basic.dart`) wraps its `drawRect` in `if (size > Size.zero)`, and `RenderDecoratedBox`
(`rendering/proxy_box.dart`) has no such check, so a degenerate `DecoratedBox` there DOES record a
zero-area fill. FLUI cannot copy that placement: `ColoredBox` here realizes as a
`RenderDecoratedBox` with a colour-only `BoxDecoration`, so one painter serves both widgets and a
class-level guard would have to pick which of the two to match while diverging from the other.

**The divergence, stated:** a zero-area `DecoratedBox` in FLUI records no background where Flutter
records one. Identical pixels — a zero-area fill rasterizes nothing either way — and one fewer
display-list command. What a caller reading the display list sees is the difference, which is
precisely the reason the guard was worth adding for `ColoredBox` in the first place.

**Alternatives:**
- Guard the whole decoration at zero size — rejected, and this is the edge case a naive port of
  Flutter's class-level guard loses: a border on a degenerate box still draws lines, and a shadow
  still has a silhouette. Skipping the decoration wholesale would drop them silently.
- Give `ColoredBox` its own render object so each class can carry its own guard — rejected as
  duplication for one boolean. `RenderColoredBox` exists but is `Leaf` arity, so it is not the
  analogue of Flutter's child-bearing `_RenderColoredBox` either.
- Signed comparison (`<= 0`) instead of `== 0` — rejected after it broke
  `circle_zero_size_and_negative_area_rects_do_not_panic`. A rect whose min exceeds its max is
  INVERTED, not empty, and this module deliberately normalizes those through `shortest_side`'s
  `.abs()`; the signed test swallowed that whole case.

**Replacement test:** `harness_decorated_box_skips_the_fill_rect_at_zero_size_like_flutter`
(`crates/flui-objects/tests/render_object_harness.rs`) covers all three degenerate shapes. The twin
that pinned the old unconditional behaviour is deleted rather than left contradicting it.


### 7. `anti_alias` is a paint OPTION, not a `BoxDecoration` field

**Rule:** [`AGENTS.md`](../../AGENTS.md) Design stance ("Look around before settling") — pick the best-known shape and say
where it comes from.

**Choice:** `paint_box_decoration` takes a `DecorationPaintOptions` alongside the decoration, and
`RenderDecoratedBox` owns the flag. `BoxDecoration` is untouched.

**Why:** Flutter puts `isAntiAlias` on `_RenderColoredBox`, not on any decoration, and the reason
generalizes: a decoration DESCRIBES an appearance and is `serde`-serializable here, while
anti-aliasing is a rasterization-quality hint that belongs to whoever is doing the rasterizing.
Serializing a rendering hint beside colours and radii would make the style data carry something
that is not style.

**Scope, stated:** the flag reaches the SOLID COLOUR fill and nothing else. Borders, shadows and
images keep the default because a `ColoredBox` has none of them, so the reference says nothing
about what they should do when it is off. Gradient backgrounds keep it for the same reason: a
`ColoredBox` has no gradient either, so the only reachable gradient is a `DecoratedBox`'s, where the
reference has no anti-alias knob. (They COULD carry it — a gradient is a shader on an ordinary
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

**Replacement tests:** `harness_decorated_box_background_carries_the_anti_alias_flag` (render
level, red when the flag is dropped between the render object and the canvas) and
`colored_box_anti_alias_defaults_on_and_reaches_the_recorded_paint` (widget level, the two oracle
cases from `basic_test.dart`, reading the composited layer tree's own `DrawRect`).

**Test-support note:** the harness's paint summary
(`flui_rendering::testing::snapshot::summarize_paint`) now prints ` aliased` for a paint that opted
out, and nothing for the default. Printing it unconditionally would have added a constant to every
line of every snapshot and changed all of them at once; printing only the deviation keeps existing
snapshots untouched while making the opt-out visible to any test reading those lines.


### 8. A style's font family is resolved against the host before it reaches the shaper

**Rule:** Design stance ("Flutter is a reference, not a spec") — a behavioural divergence from the reference is recorded with the
test that replaces the reference's own coverage.

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

**Accepted trade-off — this is where the divergence lies.** Flutter's `fontFamilyFallback`
(`packages/flutter/lib/src/painting/text_style.dart`) is searched **per glyph**: each family in the
chain is consulted when a glyph is missing from a higher-priority one. `Attrs::family` holds exactly
one family, so a per-*glyph* chain cannot be expressed. What is expressed is the per-*style* chain:
resolution walks `TextStyle::font_family_fallback` in order and takes the first entry the host
carries, degrading to the sans-serif generic only when none of them is present. The residual
divergence is therefore narrower than it was, and precisely locatable — a family that is present
but lacks the glyph still stops the walk, where Flutter would fall through: `font_family:
"CupertinoIcons", font_family_fallback: ["Noto Sans"]` on Latin text renders tofu here and text in
Flutter, because `CupertinoIcons` IS installed. Closing that needs per-run family splitting above
`Attrs`, which is tracked separately.

Pinned, per rule #1, by `a_present_but_narrow_family_stops_the_chain_where_flutter_would_not`, which
guards the divergence in both directions: it fails if the walk is "fixed" to skip a present family
(that would be a different guess, not per-glyph fallback), and it fails again if per-glyph fallback
ever lands — which is the signal to retire this record rather than let it go stale. Its control
asserts that an ABSENT primary still reaches the chain, so the stop is about presence and not about
the chain being unread.
Replacement coverage, per rule #1, in two tests because no single fixture gives both properties.
`an_uninstalled_family_shapes_in_the_bound_generic_both_ways` pins which family a run shapes in,
hermetically and in both fixture orders so that no load order satisfies it — but its fixture carries
no emoji face, so it never observes the letters and the space landing apart.
`oversized_space_from_an_emoji_face_is_closed` is the one that does: it asserts the red state
(space above 1 em, on a different face from the letters) before asserting the fix. It is hermetic,
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
`usWeightClass` of 400 (the variable-weight arm). Each of the three fails when its production arm is
reverted; that was verified, not assumed.


### 9. Intrinsic width probes skip `max_lines` truncation, floor at ellipsis

**Rule:** Design stance ("Flutter is a reference, not a spec") — Flutter is not a clean oracle for this edge
([flutter/flutter#13512](https://github.com/flutter/flutter/issues/13512) still open; pinned
`text_painter_test.dart` skips the intrinsic/`maxLines` block). Record the FLUI contract and
replace the skipped reference with a FLUI test.

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
- Copy Flutter's skipped exact intrinsic/`maxLines` equality expectations — rejected; the
  upstream contract is unresolved.
- Derive min intrinsic from break opportunities without a zero-width layout — deferred; the
  overflow-free probe plus ellipsis floor restores the documented contract without a second
  shaping pipeline for the main text.
- Keep ellipsis inside the zero-width truncating probe — rejected; that reintroduces the
  empty-prefix collapse for ordinary `max_lines` without a wide ellipsis.

**Accepted trade-off:** `max_lines` does not shrink min/max intrinsic *width* below the
shaped content (or the ellipsis floor). Parents that need truncated size use dry layout /
committed layout. Locked by `max_lines_does_not_collapse_min_intrinsic_width`,
`wide_ellipsis_floors_min_intrinsic_width`, and the matching `RenderParagraph` intrinsic
tests.

### 10. Synthetic bold follows Skia's fake-bold strength

**Rule:** a face with no bold weight is emboldened at raster time when the
style asks for bold. Flutter's engine does this through Skia's fake bold;
cosmic-text has no fake bold at all (its swash call sets no `embolden`, only a
14° skew for `FAKE_ITALIC`), so there is no in-repo oracle.

**Choice:** `SwashRasterizer` grows the outline by Skia's stroke width:
`size × ratio`, the ratio interpolated linearly from 1/24 at 9 px to 1/32 at
36 px and clamped outside (`SkScalerContext`'s `kStdFakeBoldInterpKeys`
`{9, 36}` and values `{1/24, 1/32}`, recalled from Skia source and not checked
against a clone). swash moves each point by its strength on each side, so the
strength passed is half the width.

**Why:** Skia is what Flutter paints with, so its strength is the observable
contract a bold-synthesized paragraph should match. The prototype's `size / 24`
per side doubled Skia's width at large sizes and had no reference.

**Alternatives:** FreeType's `FT_GlyphSlot_Embolden` (about `size / 24` in total at
every size) — rejected, it is not what Flutter draws; no fake bold, as
cosmic-text does — rejected, a bold style on a regular-only face would draw
regular.

**Accepted trade-off:** only `parley_text` applies it; the cosmic-text path
keeps drawing no fake bold until ADR-0092 §10 step 4 moves paragraphs to
Parley, which revisits the strength against paragraph output. Locked by
`synthetic_bold_adds_the_skia_strength` (width gain at 9, 20, 36 and 144 px).


---

## Open items

- **Nothing marks text render objects dirty on `register_font`.** The caches
  heal at the next layout (they key on `generation()`), but the layout is
  not requested by the registration — Flutter's `PaintingBinding.systemFonts`
  listener is a realm-level broadcast that belongs to flui-app.
- **`Save`/`Restore` carry a transform nobody reads.** Every command is
  stamped, the markers included; a marker-only shape would save 64 bytes
  per scope at the cost of a second command type on the wire.

### Font fixtures and distribution provenance

Font-selection regressions use a generated FLUI Probe Sans family instead of a
restricted third-party test font. Its static, non-monospaced W400/Normal metadata
keeps the fallback tie with Roboto intact, and explicit `A`, `B`, `o`, and space
coverage gives the `Ao Bo` probe real glyph IDs and positive advances. Empty
outlines are sufficient for shaping metrics; no rasterization claim is made.
Both fixture load orders remain asserted, and bypassing family resolution fails
the family-selection regression. The four older generated fixtures are unchanged.

`assets/fonts/inventory.toml` binds each font to reviewed bytes, source provenance,
and complete local license/attribution texts. The font asset gate discovers
unlisted fonts, checks hashes/notices, regenerates all fixtures into a temporary
directory, and checks Cargo package file selection. This is a Rust test-fixture
and packaging decision; it changes no Flutter-derived shaping contract. It also
does not solve registry dependency cycles or certify complete release archives.
