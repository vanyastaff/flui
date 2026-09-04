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
3. **Flutter's center model, `anchor`, and the wired `center_offset_adjustment`.** `center` is the
   first forward child; the reverse group is the prefix before it, walked backwards and laid out
   first, its correction negated; `anchor` places the center at `main_axis_extent × anchor`; the
   content dimensions carry the anchor terms; a `center` past the last child is invalid. The name
   changes (`center_sliver_index` → `center`) so every caller of the old semantics is found by the
   compiler. The sliver hook `center_offset_adjustment`, which existed unwired, joins the loop.
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
guard. Decisions 3 and 4 land in that order as separate changes; decision 5 needs no code.

The degradation query is deliberately a count-since-context-creation rather than a failure flag:
the pipeline catches a failure in the failing node's own walk frame and hands the parent a
stand-in, so a flag on the direct child sees nothing when the failure is deeper, and a poisoned
node's stand-in is served on later frames with no failure recorded at all. Both arms are pinned.
