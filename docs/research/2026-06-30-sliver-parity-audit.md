# Contained-sliver Flutter-parity audit (post-`.flutter/` restore)

**Date:** 2026-06-30
**FLUI:** `crates/flui-objects/src/sliver/` (contained slivers only — not the lazy virtualizer)
**Oracle:** `.flutter/flutter-master/packages/flutter/lib/src/rendering/{sliver,sliver_padding,sliver_fill,proxy_sliver,sliver_fixed_extent_list}.dart`

## Fixed

1. **MEDIUM — `RenderSliverOffstage` forwarded the child's scroll correction.**
   When offstage, FLUI returned the child's `scroll_offset_correction` if
   present; Flutter sets `geometry = SliverGeometry.zero` unconditionally
   (proxy_sliver.dart) — a hidden sliver never re-triggers viewport layout. The
   docstring/comment falsely claimed this divergence was parity. **Fixed
   `2121fa65`** (correction-propagation removed; doc corrected). Verified vs the
   oracle + existing offstage tests stay green; no new red→green test (the sliver
   harness has no correction-producing child — test-infra gap).

2. **LOW-MED — `RenderSliverFillRemainingAndOverscroll` positioned the child by
   its measured extent.** Reverse-axis offset used the (overscrolled) measured
   child size instead of `geometry.scroll_extent`; Flutter's
   `RenderSliverSingleBoxAdapter.setChildParentData` uses
   `paintExtent + scrollOffset - scrollExtent`. Reverse axis only (BottomToTop/
   RightToLeft): child mispositioned by `max_extent - scroll_extent`. **Fixed
   `3d0699af`** — routed through the same `child_paint_offset` helper its
   verified-faithful siblings use (positioning now identical to them). Forward
   axis unchanged (existing test green); reverse case shares the siblings' tested
   logic; no new reverse-axis test (viewport harness can't set up reverse-axis
   overscroll + child-offset inspection — test-infra gap).

## Intentional + documented (not a geometry/layout/hit-test divergence)

`RenderSliverOpacity` at full opacity: Flutter pushes an `OpacityLayer` for any
`alpha > 0` (incl. 255); FLUI treats `alpha == 255` as no-layer with an
`always_needs_compositing` opt-in for animations. Rendered pixels + hit-testing
identical; only the compositing-layer tree differs. Documented FLUI optimization;
alpha 0 and 0<alpha<255 match.

## Non-findings (verified non-divergent)
- `sliver_fill_remaining` `.max(0.0)` clamps on extent/max_extent: subsumed by
  the child-present terms; the only differing case trips the oracle's own assert.
- `hasVisualOverflow` heuristic in fixed-extent/fill-viewport: both conservative;
  affects clipping only, coincide in the eager all-visible case.

## Faithful (verified, no finding)
`RenderSliverPadding` (geometry/constraints/child-position line-for-line),
`RenderSliverToBoxAdapter`, `RenderSliverFillRemaining` (+`WithScrollable`),
`RenderSliverIgnorePointer`, `RenderSliverOpacity` layout + hit-test passthrough,
and the eager extent/geometry/positioning of `RenderSliverFillViewport` /
`RenderSliverFixedExtentList`.

## Skipped (lazy/virtualized — out of scope)
`sliver_list_lazy.rs`, `sliver_list.rs`, `virtualized_band.rs`, `viewport.rs`.

## Takeaway
Two real bugs (one a false-parity claim, one a reverse-axis positioning error),
both fixed by matching the oracle / the verified-faithful siblings. Both hit the
**sliver test-infra gap** (no correction-producing child; no reverse-axis
overscroll harness) — another facet of the secondary-query/test-wiring
architectural workstream (`2026-06-30-secondary-query-layout-gap.md`). The
contained slivers are otherwise faithful.
