# ADR-0051: Anchor-stationary scroll correction for lazy slivers

*When something above the first visible item of a lazy sliver changes extent —
a child is remeasured, an unmeasured item is re-hinted — the sliver corrects
the viewport's scroll offset by exactly that growth, in the same layout pass,
in either scroll direction, so the first visible item does not move on screen.
Flutter's `RenderSliverList` instead retains each resident child's stale
`layoutOffset` and corrects only when the walk hits a boundary, letting growth
of retained-but-invisible children shift the visible content. FLUI keeps the
divergence, records it here, and replaces the affected oracle case with a FLUI
oracle.*

---

- **Status:** Accepted (2026-09-03)
- **Date:** 2026-09-03
- **Deciders:** @vanyastaff
- **Scope:** `RenderSliverList` / `RenderSliverListLazy` and the shared band
  walk (`crates/flui-objects/src/sliver/virtualized_band.rs`), the
  `Virtualizer`'s `AnchorCorrection` (`crates/flui-rendering/src/virtualization/mod.rs`),
  the viewport's correction loop (`crates/flui-objects/src/sliver/viewport.rs`).
- **Related:** [ADR-0003](ADR-0003-virtualization-core-and-reentrant-build.md)
  (the agnostic windowing core; its consumer note on backward suppression is
  amended by this record), [ADR-0017](ADR-0017-build-during-layout-callback-seam.md)
  (the in-frame fixpoint that services lazy requests), issue #530.

## Context

Flutter's `RenderSliverList.performLayout` (`rendering/sliver_list.dart`) has no
per-item extent model. Each resident child keeps the `layoutOffset` it was given
by an earlier walk; a pass starts from the first retained child's stale offset
and re-lays out children forward with their *current* sizes. A
`scrollOffsetCorrection` is emitted only when the leading walk hits a boundary:
the list ran out of children before reaching the scroll offset (`-scrollOffset`),
or the computed first-child offset went negative (`-firstChildScrollOffset`).
Growth of a retained child that is above the viewport but below the first
retained child therefore moves everything under it on screen, with no user
input — the well-known "content jumps when an off-screen item resizes".

FLUI's lazy list is built on a prefix-sum `Virtualizer` (ADR-0003) that knows
every item's measured or hinted extent. Its scroll offset is always the sum of
the extents above, so it can do what Flutter cannot: when an item above the
anchor changes size, shift the offset by the delta and leave the anchor where
it was. Compose's `LazyListState` (`firstVisibleItemIndex` +
`firstVisibleItemScrollOffset`) and GPUI's `ListState` anchor the same way.

Two things were unsettled when #530 opened:

1. Whether the two-item difference between FLUI's windows and the oracle's in
   `slivers_test.dart` 'SliverList can handle inaccurate scroll offset due to
   changes in children list' was a bug. It is not: traced pass by pass on
   2026-09-03, the swap-time correction is +192 (two odd items above the anchor
   grew 0 → 96), Flutter expresses the same 192 px as a visual shift, and from
   there both models accumulate the same growth and clamp at zero. FLUI is
   exactly 192 px further from the top at every later checkpoint — one more
   250 px drag from the oracle's final `[0,6]`.
2. Whether ADR-0003's consumer note — suppress the correction while scrolling
   backward — was load-bearing. It was not: with suppression on and off the
   scene produced identical windows and offsets at every checkpoint. Suppression
   can only defer a correction by one pass, and the deferral is itself a
   one-frame jump of the anchor by the growth above it.

## Decision

1. **The anchor is the first visible item, and it stays pixel-stationary.**
   Every extent change above it — a remeasure (`Virtualizer::set_measured`) or
   an adaptive re-hint of unmeasured items (`Virtualizer::adapt_default_estimate`)
   — contributes its delta to one accumulator, emitted as
   `SliverGeometry::scroll_offset_correction` at the end of the pass. The
   viewport applies it and re-runs layout in the same pass (Flutter's own
   correction loop, `RenderViewport.performLayout`), so a frame never paints an
   anchor that moved.
2. **Corrections are direction-independent.** `take_anchor_correction` drains
   the accumulator whenever it is non-zero; scroll direction is not an input.
   ADR-0003's backward-suppression note is retired.
3. **The Flutter oracle case is replaced, not narrowed.** The `#[ignore]`d pin
   keeps the oracle's literal windows in-tree as the executable statement of
   the behaviour FLUI declined; a FLUI oracle beside it asserts the stationary
   anchor across the swap, the accurate-offset windows at every checkpoint, and
   `[0,6]` at `pixels == 0` one drag later. The parity manifest records the
   case under `decision = "ADR-0051"` in the `diverged` bucket; it never counts
   as parity.

## Consequences

- No visual jump when an off-screen item above the viewport changes size, in
  either scroll direction. The scrollbar's total follows the accurate offset.
- A drag that scrolls past growing items travels less than its raw delta by
  exactly that growth (the offset is corrected under the finger while the
  content stays put); reaching the list's start from a position that has
  unmeasured items above it takes their real, not estimated, extent. Flutter
  has the same property from its boundary corrections; the two models differ
  only in *when* the growth is charged.
- The `last_scroll_offset` state and the forward/backward branch are gone from
  both list render objects and the band walk.
- What stays estimated: items above a jump that were never resident keep
  their hint until they enter the band. That is inherent to O(band) layout —
  Flutter lays out every child between the last known offset and the target
  and is exact at O(distance) — and is the trade ADR-0003 already made.

## Alternatives considered

- **Port Flutter's retained-stale-offset walk.** Rejected: it gives up the
  Virtualizer's O(log n) offset queries and exact-when-measured totals to
  reproduce a content jump that Compose, SwiftUI and GPUI all avoid.
- **Keep the backward suppression.** Rejected on the measurement above: no
  observable effect in the oracle scene, and a one-frame anchor drift in the
  only case it can act on (a resident item remeasured during a backward drag
  with no build in the same frame).
