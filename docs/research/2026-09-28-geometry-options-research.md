# Geometry and painting values: what other frameworks learned

- **Date:** 2026-09-28
- **For:** [ADR-0098](../adr/ADR-0098-owned-f64-geometry-values.md) and
  [issue #1352](https://github.com/vanyastaff/flui/issues/1352)
- **Method:** five independent reviews, each asked to find problems rather than confirm the
  proposal, citing a URL or `file:line` for every claim. Claims marked *unverified* are
  recorded as such. Local counts come from the
  [census](2026-09-28-geometry-value-layer-census.md).

The first proposal exposed euclid aliases in the Stable facade. The reviews rejected that and
converged on FLUI-owned `f64` value types over kurbo and glam. The sections below give the
evidence in the order the decision uses it.

## 1. Which crate, in 2026

crates.io, recent downloads (90 days) on 2026-09-28:

| Crate | Recent downloads | Role |
|---|---:|---|
| euclid 0.22.14 | 28.9M | mostly transitive, through Servo/Firefox, lyon and etagere |
| kurbo 0.13.1 | 18.4M | the 2D-UI geometry of vello, peniko, Xilem/Masonry, Blitz and Dioxus `anyrender` |
| glamx 0.3 (new 2026) | 425K | poses and rotations for the dimforge physics stack, not UI geometry |
| vello_api 0.0.7 (new 2026) | 62K | Linebender's renderer API; speaks kurbo and peniko; relevant to ADR-0087, not layout |
| anyrender 0.13 | 93K | 2D canvas abstraction over kurbo and peniko |
| glamour 0.18 | 4.8K | typed glam; little use, last release 2025-05 |

Nothing newer replaces kurbo for 2D UI geometry. glam stays the matrix crate.

## 2. Scalar: `f64`, narrowed once

- **f32 fails on the scroll axis.** At 5e7 logical px (1M rows of 50 px) the f32 ulp is 4 px:
  `f32(5e7) + 0.3 - 5e7 == 0.0`, and child offsets computed as
  `layout_offset - scroll_offset` land on 4-px steps. At 1e6 px the step is 1/16 px, enough to
  shimmer text. egui, which is f32 throughout, reports jitter above about 2M rows and breakage
  above 100M ([egui#1391](https://github.com/emilk/egui/issues/1391)). Firefox has the same
  class of bug past 16M device px
  ([bug 1726431](https://bugzilla.mozilla.org/show_bug.cgi?id=1726431)).
- **Flutter lays out in `double`** and narrows to `float` at the `dart:ui` boundary through
  `SafeNarrow` ([engine#40098](https://github.com/flutter/engine/pull/40098)), after its child
  offsets are already small differences. SwiftUI's `CGFloat` and kurbo are `f64` as well.
- **FLUI today** has f32 `scroll_offset`, `pixels` and sliver extents
  (`crates/flui-rendering/src/constraints/sliver_constraints.rs:58-83`,
  `crates/flui-rendering/src/view/scroll_position.rs:36-38`) and tolerances at or below one ulp
  (`EPSILON_F32 = 1e-6` in `sliver_geometry.rs:11`; `1e-3` in
  `crates/flui-objects/src/sliver/sliver_fixed_extent_list.rs:77`, below one ulp past 16,384 px).
- **Fixed point** (Blink's 1/64-px `LayoutUnit`, Gecko's 1/60-px `nscoord`) buys exact sums at
  the cost of a range cap (about 33.5M px for Blink, *unverified against source*; 17.9M for Gecko)
  and saturating arithmetic. It is not needed when `f64` is available.
- The alternative, an f32 box model with an f64 scroll carve-out, needs a list of fields that
  change type and a narrowing function in the sliver protocol. `f64` everywhere removes the
  special case.

## 3. Public types: owned, not upstream

**euclid aliases**, the first proposal:

- **Same names, different results.** euclid's `Box2D::contains` is half-open, and FLUI's is
  inclusive (`crates/flui-geometry/src/rect.rs:471-476`, `euclid-0.22.14/src/box2d.rs:217-219`).
  `union` skips empty boxes, and FLUI's does not (`box2d.rs:305-312`, `rect.rs:571-575`).
  `intersection` returns `None` for touching boxes, where FLUI returns `Some` (`box2d.rs:277-284`,
  `rect.rs:558`). Every call site keeps compiling after a switch.
- **WebRender's `Rect` → `Box2D` port** ([bug 1711648](https://bugzilla.mozilla.org/show_bug.cgi?id=1711648))
  was backed out several times and regressed in bugs 1714926, 1717655, 1725179 and 1736569.
- **euclid has changed semantics in patch releases.** In 0.22.5 empty boxes stopped counting
  toward `union`. 0.22.13 added `From` impls, which can break inference. Its NaN `min`/`max`
  depend on argument order ([euclid#540](https://github.com/servo/euclid/issues/540)).
- **The typed spaces have escape hatches.** `cast_unit`, `to_untyped` and `From<(T, T)>` all
  exist. A WebRender clone has 131 escape-hatch calls, one of them covering a value "that should
  actually be in RasterPixels" (`picture.rs:5461-5467`).
- **Slint** uses euclid internally but exposes plain `LogicalSize`/`PhysicalSize` structs
  (`internal/core/lengths.rs`; [docs](https://docs.rs/slint/latest/slint/struct.LogicalSize.html)).
  GPUI has its own types.

**kurbo directly**, as Xilem, Blitz and anyrender do:

- **Its breaking releases barely touched the core value types.** 0.11 moved `mint` impls, 0.12
  removed the deprecated `Rect::is_empty`/`Size::is_empty`, and 0.13 made functions `const`.
  Type identity still changes with each major, though: `kurbo 0.12::Rect` is not
  `kurbo 0.13::Rect`. No 1.0 is planned. Linebender releases in lockstep (peniko re-exports kurbo
  and color, `peniko-0.6.1/src/lib.rs:37,40`), and Xilem follows because it makes no stability
  promise. parley keeps kurbo out of its API (`parley-0.11.1/src/util.rs:16-24`).
- **FLUI's vocabulary is Flutter's, and kurbo's differs.**
  - `Insets` grow outward and take `(left, top, right, bottom)`; FLUI's `EdgeInsets::new` takes
    `(top, right, bottom, left)`. That is 59 call sites that would compile and lay out wrong.
  - kurbo has no elliptical `Radius`, no perspective matrix and no superellipse.
  - Masonry still grew its own `Length`, `LayoutSize` and `SizeDef` on top of kurbo
    (`masonry_core/src/layout/length.rs:4-10`).
- **Upstream types cannot take FLUI's methods, constants, `Display` or `serde`.** An extension
  trait resolves `Rect::from_ltwh` only while it is imported. An inherent method of the same name,
  added upstream later, silently wins over it (checked with a compiled probe on rustc 1.98).

**Reversibility decides it.** Owned `#[repr(C)]` `f64` structs with kurbo's field layout can later
become kurbo aliases (after a kurbo 1.0) with a mechanical change. Exposing kurbo now and
withdrawing it later breaks every user.

## 4. Logical and physical coordinates

- The census shows the typed space machinery never paid off. `ScaleFactor` and `to_device` have
  no production caller; flui-engine types device rectangles as `Rect<Pixels>`; 28 boundary sites
  cast by hand.
- Flutter, Xilem (through the `dpi` crate) and winit keep logical/physical apart only at the
  window boundary. Distinct integer device types there, which never meet logical values
  elsewhere, cover the mixing risk.
- Local/parent/global confusion is not caught by types in any framework: Slint, with typed
  spaces, still shipped [slint#13242](https://github.com/slint-ui/slint/issues/13242). Hit-test
  and paint tests under non-identity transforms are the guard.

## 5. Pixel snapping

- **Ties toward +∞ (`floor(x + 0.5)` in double) is Skia's `sk_float_round`**
  (`include/private/SkFloatingPoint.h:38,119`). Snapping edges rather than sizes is how Chromium
  (`SnapSizeToPixel`, `layout_unit.h:805-818`) and iced 0.14 (#2768) avoid seams. The seams
  Flutter never fixed come from not doing this: [flutter#46604](https://github.com/flutter/flutter/issues/46604),
  [#31305](https://github.com/flutter/flutter/issues/31305),
  [#37578](https://github.com/flutter/flutter/issues/37578).
- **Snap in raster-target coordinates, after composing ancestor transforms.** Snapping local
  offsets gives 1-px gaps ([Taffy PR #1189](https://github.com/DioxusLabs/taffy/pull/1189);
  GPUI documents the trade-off in `crates/gpui/src/taffy.rs`).
- **Border and stroke widths need their own rule.** At DPR 1.25 a 1-logical border snapped by
  edges gives 1 px on one side and 2 px on the other. CSS's "snap as a border width" rule
  ([csswg#10729](https://github.com/w3c/csswg-drafts/issues/10729)) and GPUI's
  `round_stroke_to_device_pixel` fix this. A non-zero width must never collapse to 0.
  Odd-width strokes centre on half pixels (egui 0.31 `StrokeKind`).
- **Snap only under translation plus positive axis scale.** That is WebRender's snapping
  transform ([bug 1574493](https://bugzilla.mozilla.org/show_bug.cgi?id=1574493)) and Chromium's
  transformed rasterization. Animated transforms are not re-snapped each frame: WebRender
  [bug 1611601](https://bugzilla.mozilla.org/show_bug.cgi?id=1611601) (stutter) and
  [1635406](https://bugzilla.mozilla.org/show_bug.cgi?id=1635406) (blur).
- **Text snaps the run origin, not each glyph.** Impeller's per-glyph jitter:
  [engine#40073](https://github.com/flutter-team-archive/engine/pull/40073),
  [flutter#149652](https://github.com/flutter/flutter/issues/149652).
- **Every pipeline snaps the same way.** iced had a 1-px band between gradient and solid quads
  (iced #2962).
- **Platform rules:**
  - Wayland fractional-scale-v1 sends the scale as n/120 and rounds the buffer size halfway away
    from zero ([protocol](https://wayland.app/protocols/fractional-scale-v1)), so compute it in
    integers.
  - On Windows the physical client size is the source of truth (1001 px at 125% is 800.8
    logical).
  - On the web, use `devicePixelContentBoxSize`
    ([web.dev](https://web.dev/articles/device-pixel-content-box)).

## 6. Painting values

- **`Path` over `kurbo::BezPath` must keep a shape hint.** `Path::rrect_hint`
  (`crates/flui-types/src/painting/path.rs:271-293`) feeds the analytic Material shadow, and
  kurbo bakes arcs into cubics (`kurbo-0.13.1/src/arc.rs:77-103`). Arcs are then built at
  construction, not at device scale, so the construction tolerance must be tight. Containment is
  non-zero only (`shape.rs:123-136`), so even-odd uses `winding % 2`.
- **Stroking.** lyon double-blends self-overlapping translucent strokes
  (`lyon_tessellation-1.0.22/src/stroke.rs:43-47`). Outlining with `kurbo::stroke` and then
  filling matches Skia and vello_cpu.
- **Colour.**
  - `color::AlphaColor::lerp` premultiplies (`color-0.3.3/src/color.rs:475-487`), and so does
    CSS Color 4. Flutter's `Color.lerp` interpolates straight components, which darkens fades to
    transparent.
  - peniko gradients default to premultiplied sRGB (`peniko-0.6.1/src/gradient.rs:250,311`).
    Flutter interpolates unpremultiplied, which is the open
    [flutter#48674](https://github.com/flutter/flutter/issues/48674). The alpha space should be a
    gradient property with a deliberate default.
  - `Color` loses `Eq`/`Hash` if it stores floats. Two production types derive `Eq` over it
    (`crates/flui-painting/src/text_layout/glyphs.rs:26`,
    `packages/flui-material/src/color_scheme.rs:46`).
  - Flutter 3.27's wide-gamut migration broke exact equality and implementers
    ([guide](https://docs.flutter.dev/release/breaking-changes/wide-gamut-framework)). Keeping
    storage private leaves room for that later.
- **peniko pins kurbo and color.** Bump the three as one group.

## Not verified

- Chromium's `LayoutUnit` range, taken from a secondary source.
- WebRender's scroll-offset scheme.
- Masonry's exact field types.
- Large-list behaviour in Iced, Slint and Taffy.
- Skia/Impeller `isRRect` internals.
- The pixel difference between the kurbo and lyon strokers.
- The `f64` memory and speed cost for FLUI.
- Firefox and Safari support for `devicePixelContentBoxSize`.
