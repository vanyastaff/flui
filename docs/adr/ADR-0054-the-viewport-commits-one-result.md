# ADR-0054: The viewport commits one result

- **Status:** Accepted (2026-09-04), landing in steps — the "Status of the decisions" section says
  which are in the tree.
- **Date:** 2026-09-04
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-objects/src/sliver/viewport.rs` (`RenderViewport`,
  `RenderShrinkWrappingViewport`), the layout contexts in `crates/flui-rendering/src/context/`,
  the semantics assembler (`crates/flui-rendering/src/pipeline/owner/semantics.rs`),
  `crates/flui-widgets/src/scroll/{viewport,custom_scroll_view}.rs`.
- **Related:** [ADR-0017](ADR-0017-build-during-layout-callback-seam.md) (+ amendment),
  [ADR-0053](ADR-0053-one-lazy-child-lifecycle-for-multi-box-slivers.md); issue #537; follow-ups
  #833 (the per-pixel rebuild) and #834 (the pipeline's positioned-this-pass stamp).

## Context

A viewport's layout is a loop: attempt a pass, apply a sliver's scroll correction, attempt again,
and accept once the scroll position takes the resulting dimensions. Three things about how FLUI
committed that loop's result were inconsistent with each other and with Flutter
(`rendering/viewport.dart`, read 2026-09-04):

- **Positions were committed by the pass, not by the loop.** Each attempt positioned children as it
  walked, against a size the shrink-wrapping viewport did not yet know (an unbounded main axis
  gives it a provisional size); a second full layout of every child re-ran after acceptance only to
  recompute the physical offsets of a reverse axis, and any correction that pass asked for was
  warned about and ignored. Flutter stores logical offsets in parent data and resolves physical
  offsets at paint (`paintOffsetOf`).
- **A failed child pass is invisible to the viewport.** The pipeline catches a sliver child's panic,
  non-finite geometry or layout cycle in the child's own frame and hands the parent
  `SliverGeometry::ZERO`; the parent accepts a stand-in pass, `apply_content_dimensions` clamps the
  scroll position, and the user's offset teleports. A panic poisons the child on its first failure
  and clears the parent's own `NEEDS_LAYOUT` in the same walk, so there is no retry to wait for.
- **`anchor` and `clip_behavior` did not exist**, and `center_sliver_index` was the mirror of
  Flutter's `center` (forward prefix, reverse suffix walked forward), which made the reference's
  `center` + `anchor` oracles unportable.

## Decisions

1. **Positions are resolved once, at commit.** `layout_child_sequence` stages
   `(slot, layout offset, growth direction, paint extent)`; after the loop accepts, `commit_positions`
   resolves physical offsets against the final size. The shrink-wrapping viewport's second layout
   pass is deleted. Every child is laid out exactly once per accepted pass (pinned by a counting
   sliver: one pass, one layout), and a reverse axis positions from the final extent (pinned).
   Improvement over Flutter's model: the offsets are committed with the layout, so paint and
   hit-test read state instead of resolving it.
2. **A degraded pass publishes no dimensions.** The layout context reports whether a descendant's
   layout was degraded during this node's pass (a failure turned into a stand-in, or a poisoned
   node served its stand-in) — at any depth, because the walk is depth-first and synchronous. A
   viewport whose pass was degraded still lays out and positions (the tree stays internally
   consistent: every offset and every child geometry come from one pass) but does not apply
   viewport or content dimensions to the scroll position, so the user's offset survives a broken
   child; publishing resumes when the child recovers. Its size is always what the current
   constraints admit, never a stored size a new constraint might reject.
3. **Flutter's center model and `anchor`.** `center` is the first forward child; the reverse group
   is the prefix before it, walked backwards and laid out first, its correction negated; `anchor`
   places the center at `main_axis_extent × anchor`; the content dimensions carry the anchor
   terms; a `center` past the last child is invalid. The name changes (`center_sliver_index` →
   `center`) so every caller of the old semantics is found by the compiler. The sliver hook
   `center_offset_adjustment` — unwired, with zero overrides anywhere in the codebase or in
   Flutter's own sliver family — is deleted rather than wired; see "Status of the decisions" for
   the reasoning.
4. **`clip_behavior`** on both viewports (default `HardEdge`, `None` clips nothing); the paint clip
   a child sees is the viewport's bounds shrunk by the previous sliver's overlap (Flutter's
   `describeApproximatePaintClip`); the semantics clip is the bounds extended by the cache extent
   along the axis. The assembler drops a node whose rect falls outside the semantics clip and
   marks hidden one fully outside the paint clip, in both its full walk and its graft.
5. **Shrink-wrap keeps Flutter's infinite window.** The issue asked that an unbounded main axis
   never reach the grid arithmetic; Flutter passes the infinite window and the lazy objects bound a
   sentinel count (ADR-0053). Laying out with a bounded window and growing would reintroduce the
   non-terminating builder that cap exists for. Recorded as an override of the issue's wording.

## Recorded divergences and gaps

- The wgpu backend ignores a clip layer's `Clip` mode: `None` versus clipped is observable, the
  clipped modes are not distinguishable on screen. Pixel evidence stays per primitive in the
  engine's readback suite.
- `center` is an index, not a key; a key-based center is a follow-up.
- The hit-test child-count snapshot the slivers keep (committed only when a pass validates) is
  retired by #834's pipeline stamp, not here.
- The per-pixel rebuild of the viewport subtree on scroll is #833; every setter added here returns
  no impact on equality so that rebuild stays cheap until then.

## Status of the decisions

Decision 1 landed first, with the reverse-axis and one-layout-per-pass pins. Decision 2 landed
next: the walk counts every degradation event (a layout that failed and handed its caller a
stand-in, or a poisoned node that served one), a layout context reports whether that count moved
during its node's pass, and both viewports publish no scroll dimensions from such a pass — pinned
at one level and at two levels below the viewport, each reading a clamped offset without the
guard. Decision 3 landed third:

- `RenderViewport`'s `center_sliver_index: Option<usize>` (forward prefix, reverse suffix) is
  renamed `center: Option<usize>` and re-meant to Flutter's model: the first FORWARD child: the
  prefix `[0, center)` is the reverse group, walked backwards (`center - 1` down to `0`) and laid
  out FIRST, its correction returned negated; the suffix `[center, child_count)` is the forward
  group, laid out second, always. `None` still means "every child forward" (`center == 0`).
  `Some(n) == child_count` — FLUI's old "no center" spelling — is no longer meaningful under this
  model (Flutter's center is always a direct child): `debug_assert!`ed and, in release, clamped
  to the last child with a one-time warning.
  `anchor: f32` (default `0.0`) is new alongside it. An out-of-range or non-finite `anchor` is
  **clamped rather than asserted** (non-finite to `0.0`), with a one-time warning: it is caller
  input, not an internal invariant, and this library does not panic on a configuration gap — the
  same rule `RenderTable` follows for a baseline alignment with no text baseline. Flutter asserts;
  `NaN` would otherwise poison every offset the layout derives from it. The anchor drives
  `_attemptLayout`'s formulas verbatim
  (`centerOffset`, the reverse/forward remaining-paint/cache-extent splits, the reverse group's
  hardcoded `overlap: 0.0`, the forward group's `overlap` folding in the leading-edge overscroll
  only when there is no reverse group ahead of it) and the anchor terms in
  `applyContentDimensions`'s accepted range. `RenderShrinkWrappingViewport` gets neither — Flutter
  gives it no `center`/`anchor` either, and it keeps its all-forward, no-center layout unchanged.
- The obstruction bookkeeping (`sliver_obstruction_extents`, `max_scroll_obstruction_extent_before`)
  is re-keyed from layout-visit order (a push per child, in the order `layout_child_sequence`
  happened to walk them) to absolute child-slot order (an index-sized `Vec` written by index),
  and the query is direction-aware: a forward child sums `[center, child_index)`, a reverse child
  sums `(child_index, center)` — the slivers closer to `center` than it. A pin
  (`viewport_max_scroll_obstruction_extent_before_is_keyed_by_slot_not_layout_order`) exercises a
  3-child, `center: Some(2)` tree where visit order and index order actually disagree and shows
  the push-order/`.take(child_index)` reading swaps what indices 0 and 1 report.
- `RenderSliver::center_offset_adjustment` — the sliver hook `viewport.dart`'s loop reads once per
  frame (`offset.pixels + centerOffsetAdjustment`) — is DELETED rather than wired. It had exactly
  one implementation in the whole codebase (the trait's own `0.0` default) and zero overrides
  anywhere in FLUI's port of Flutter's own sliver family either (Flutter's base `RenderSliver`
  also just returns `0.0`; nothing in `rendering/*.dart` overrides it) — wiring it would mean a
  new cross-protocol callback (a `SliverCenterOffsetAdjustmentCallback` alongside
  `SliverLayoutChildCallback`, threaded through `ErasedBoxLayoutCtx`, `BoxLayoutCtxErased`,
  `LayoutContext`, and `layout_dirty_root`'s closure construction) for a value that can only ever
  evaluate to `0.0` today. Deleting the always-zero hook and using `self.offset.pixels()` directly
  is behaviorally identical to wiring it, so nothing observable changes; if a future sliver family
  genuinely grows in both directions from one scroll offset, the hook is cheap to re-add with a
  real caller at that point.

Decision 4 landed in two parts. The first is `clip_behavior` itself: both viewports carry it
(default `HardEdge`), and each clips only when its content overflows **and** the behaviour is not
`Clip::None`, which produces no clip layer at all — pinned structurally by
`viewport_clip_behavior_controls_the_clip_layer`, which reads the composited layer kinds for the
overflowing-clipped, overflowing-unclipped and fitting cases. `Viewport`, `ShrinkWrappingViewport`
and `CustomScrollView` all expose the knob. Recorded gap: the wgpu backend ignores a clip layer's
`Clip` mode (`push_clip_rect` takes it and discards it), so `None` versus clipped is observable on
screen but the clipped modes are not distinguishable from one another; per-mode pixel evidence
belongs to the engine's own readback suite, not here.

The second part — the paint and semantics clips a child sees
(`describeApproximatePaintClip`/`describeSemanticsClip`, and the assembler dropping a node outside
the semantics clip and marking hidden one outside the paint clip) — is deliberately **not** in that
change: the hooks would have had no consumer until the assembler reads them, and a hook nothing
calls is the defect class this repo names. It lands with the assembler work. Decision 5 needs no
code.

The degradation query is deliberately a count-since-context-creation rather than a failure flag:
the pipeline catches a failure in the failing node's own walk frame and hands the parent a
stand-in, so a flag on the direct child sees nothing when the failure is deeper, and a poisoned
node's stand-in is served on later frames with no failure recorded at all. Both arms are pinned.

Two consequences of the same shape, each pinned by a test that reads a wrong number without it:

- **A cache hit inherits the degradation.** Geometry committed by a degraded pass is marked on the
  node (`RenderFlags::GEOMETRY_DEGRADED`, sticky until the node completes a pass in which nothing
  below it degraded). The layout walk's clean-node shortcut counts a degradation when it serves
  such geometry, and a viewport re-lays out rather than serving its own cache for such a child —
  otherwise the broken descendant is never walked and the pass looks healthy, which is exactly
  what happens to a sliver that has scrolled beyond the window.
- **The viewport dimension is published before the pass runs**, as in Flutter, and a page position
  moves `pixels` to keep its fractional page across a resize. The dimension itself is not degraded
  data — it comes from the viewport's constraints — so it is kept, but the offset it moved is
  restored when the pass turns out degraded.
