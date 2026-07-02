# Proxy-paint render-object Flutter-parity audit (post-`.flutter/` restore)

**Date:** 2026-06-30
**FLUI:** `crates/flui-objects/src/proxy/{opacity,clip,decorated_box,repaint_boundary,colored_box}.rs`
**Oracle:** `.flutter/flutter-master/packages/flutter/lib/src/rendering/proxy_box.dart` (+ `painting/box_decoration.dart`)

Scope: **layout + hit-test** behavior (verifiable). Paint-pixel / compositing-layer
strategy differences are noted but out of scope (often intentional optimizations).

## Fixed

1. **CRITICAL — `RenderRepaintBoundary` did not hit-test its child.** It defined
   no `hit_test`, so the trait default (`is_within_own_size`) absorbed every
   in-bounds hit on the boundary itself and never recursed — **blocking its
   entire subtree from pointer events**. Flutter's `RenderRepaintBoundary` extends
   `RenderProxyBox` (`hitTestSelf` false → hit iff child hit). **Fixed `31359807`**
   — added a `has_child`-gated passthrough `hit_test` (mirrors `RenderOpacity`) +
   `harness_repaint_boundary_hit_tests_through_to_child`.

2. **HIGH — `RenderDecoratedBox` tested the decoration shape before the child.**
   A rounded decoration excludes the rect's corners from `hitTestSelf`, but FLUI
   evaluated that first and returned false, so a child hittable in a cut corner
   was rejected. Flutter's default `RenderBox.hitTest` is
   `hitTestChildren() || hitTestSelf()` — child first, decoration shape as
   fallback. **Fixed `31359807`** (reordered) + `harness_decorated_box_hit_tests_child_before_decoration_shape`.

## Faithful (verified layout + hit-test)
- `RenderOpacity` — layout passthrough; hit-test gated on own-size, **not** alpha
  (alpha==0 child still hit-testable — this *matches* Flutter, which doesn't
  override `hitTest`; the audit brief's "alpha==0 → not hit-tested" premise was
  itself non-Flutter and FLUI correctly follows Flutter).
- `RenderClipRect/RRect/Oval/Path` — layout passthrough; hit-test rejects points
  outside the clip shape; the oval ellipse test (`dx²+dy²≤1`) is arithmetically
  equivalent to Flutter's `distanceSquared > 0.25` reject (incl. the boundary);
  default rect/rrect/path clips are rectangular (match Flutter's `_clipper==null`
  super-hitTest).

## Paint-only / compositing-strategy (noted, out of layout/hit-test scope)
`RenderOpacity` needs_compositing/paint_alpha/skip_paint vs Flutter
alwaysNeedsCompositing/OpacityLayer; clip uses a layer scope (not canvas clip) and
approximates oval as elliptical RRect for *painting*; `RenderDecoratedBox`
isComplex/BoxPainter caching; `RenderRepaintBoundary` always_needs_compositing +
paint_count. Rendered pixels equivalent.

## Unable to confirm
RRect rounded-corner `contains` for a custom rounded clipper with oversized/
overlapping radii (FLUI per-corner ellipse vs Flutter `RRect.contains` after
`scaleRadii`); ClipPath default-clip exact-edge points. Interiors identical.

## Not present in FLUI (nothing to audit)
`RenderColorFilter`, `RenderImageFiltered`, `RenderBackdropFilter`,
`RenderShaderMask`, `RenderPhysicalModel/Shape` — no FLUI definitions yet.

## Takeaway
Two real hit-test bugs (one CRITICAL — RepaintBoundary blocking subtree input;
one HIGH — DecoratedBox child/shape order), both fixed with red→green tests. The
opacity + clip family are faithful. This completes the contained render-object
parity sweep (box, flex, transform/fit, sliver, wrap, proxy-paint).
