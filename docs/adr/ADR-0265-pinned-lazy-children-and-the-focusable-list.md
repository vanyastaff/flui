# ADR-0265: A pinned lazy child is laid out and announced outside the band, and a focusable list is one Tab stop that pins its row

- **Status:** Accepted (2026-10-29). Not implemented: §3, the focusable list.
- **Date:** 2026-10-29
- **Supersedes:** [ADR-0056](ADR-0056-keep-alive-is-an-element-side-lease.md), "A parked child is
  not moved", for a pinned child only: a pinned child outside the band is laid out, where a held
  child is not.
- **Related:** [ADR-0053](ADR-0053-one-lazy-child-lifecycle-for-multi-box-slivers.md) (the band
  eviction a pin exempts a child from), [ADR-0026](ADR-0026-focus-traversal-seam.md) (the
  traversal entry a focusable list registers), [ADR-0054](ADR-0054-the-viewport-commits-one-result.md)
  (the one result a viewport commits, which a pinned child adds nothing to)

## Context and problem statement

A keep-alive hold keeps a lazy child's element state through band eviction, and the sliver leaves
the held child attached but not laid out (ADR-0056). Its placed-generation stamp goes stale, so
paint, hit testing and semantics all skip it. That is right for a half-typed row the user scrolled
away from, and wrong for the row that has keyboard focus: a screen reader loses the focused node,
focus geometry reads a rect from a pass that no longer holds, and the next arrow key has nothing to
move from. The lazy multi-box slivers are `RenderSliverList`, `RenderSliverFixedExtentList`,
`RenderSliverGrid` and `RenderSliverCarousel`, driven by `SliverAdaptorManager` in `flui-view`.
How does a lazy child stay measurable and announced outside the band without changing what the
sliver paints, hits or reports as its extent?

## Decision drivers

- A focused row scrolled away by the pointer stays focused and in the accessibility tree; the next
  arrow key moves from it.
- The sliver's geometry, anchor correction and measured extents are a function of the band alone:
  a pinned row far away must not move the scroll extent or the anchor.
- The contract is the same on every lazy multi-box sliver: no sliver documents an exception.
- A viewport clip is not a reason a child is unpainted: under `Clip::None` there is no clip.
- No new public surface beyond the lease; the decision stays on the element side, where ADR-0056
  put holds.
- Taking or releasing a pin reaches the render object without a scroll to prompt a relayout.

## Considered options

1. A pin lease in the keep-alive table; the sliver lays out pinned residents outside the band and
   skips them explicitly in paint and hit testing.
2. A held child stays unlaid (ADR-0056 unchanged); focus keeps its last rect, and semantics keeps
   the node from the last pass that placed it.
3. Grow the band to include the focused row.
4. Move the focused row out of the list, into an overlay, while it is out of the band.

## Decision

Chosen option: 1, because it is the only option that keeps a real, current rect for semantics and
focus while leaving the band, and with it the sliver's geometry, untouched.

### §1 A pin is a hold the sliver also lays out

`KeepAliveHandle::pin(&self) -> KeepAliveLease` takes a hold that is also a pin. A pin counts as a
hold everywhere a hold counts (`retain_band` does not evict the child), and the lease's `Drop`
releases both. The table records pins per holder beside the holds and resolves them to children
through the tree when asked, as it resolves holds (`KeepAliveHolds::pinned_children`), so a pinned
row that moves under reconcile or between hosts keeps its pin.

After each band eviction `SliverAdaptorManager` resolves the pinned residents to their logical
indices, keeps those outside the retained band, and hands the set to the render object through
`LazyMultiBoxRender::set_pinned_indices(&mut self, &BTreeSet<usize>) -> RenderUpdateImpact`; a
changed set relays out the sliver and republishes its semantics. The manager registers the host's
`LocalRenderInvalidationHandle` with the table (`KeepAliveHolds::watch_pins`); taking or releasing
a pin marks every watching sliver for layout, clones the handles out of the table before calling
them, and drops the handles whose node has detached. A holder that unmounts with pins notifies the
same way.

### §2 The sliver lays the pinned child out, and nothing else sees it

Each lazy sliver keeps a `PinnedChildren` (`crates/flui-objects/src/sliver/pinned.rs`). After the
band, it lays out every attached child whose index is pinned and outside the band, under the
band's child constraints, at the place the band would give it: the virtualizer's offset for that
index in `RenderSliverList`, `index × item_extent` in `RenderSliverFixedExtentList`, the tile
layout's cell in `RenderSliverGrid`, and in `RenderSliverCarousel` the place where it rests on
the large keyline (the keylines at the current offset park every item outside them at an edge,
which is no place to reveal it from). The child gets a fresh placed-generation stamp, so semantics
and focus see it at a current rect.

Its extent goes into none of `scroll_extent`, `paint_extent`, `cache_extent`, the anchor
correction or `Virtualizer::set_measured`. Paint and hit testing skip its slot explicitly, by the
slots the last pass recorded, since a freshly stamped child would otherwise be painted and hit.
The carousel paints and hits only the items it shows, so its pinned child needs no skip there;
its semantics visit admits the pinned slot alongside the shown items.

### §3 A focusable list is one Tab stop, and its row holds the pin

A focusable `ListView` registers one traversal entry (ADR-0026): Tab enters the remembered row,
or the first visible one; arrows, Page Up/Down and Home/End move inside it. The row's focus marker
holds a pin while the row has primary focus or is the list's remembered row; on a change it takes
the new pin before it releases the old one. The bound on built rows is therefore the band, the
cache and one pinned row.

### Consequences

- Good, because a focused row scrolled away stays in the accessibility tree at a current rect, and
  the next key moves from it.
- Good, because the sliver's geometry is unchanged by a pin: the harness compares it with the
  unpinned run.
- Good, because the lazy slivers share one implementation of the pinned slot.
- Bad, because a pinned row costs a layout per frame while it is pinned; one row per focusable
  list bounds it.
- Bad, because the pinned child's offset in `RenderSliverList` is the virtualizer's estimate, not a
  measurement; it is exact in the fixed-extent list and the grid.
- Neutral, because a pinned child outside the cache stays in the semantics tree only below a reveal
  ancestor (a scrollable answering `ScrollIntoView`), as any laid-out child past the cache does.

### Confirmation

- `harness_lazy_slivers_keep_a_pinned_child_out_of_band` (`crates/flui-objects/tests/render_object_harness.rs`):
  on all four slivers, under `Clip::HardEdge` and `Clip::None`, the pinned row is laid out at its
  place, announced, not painted, adds nothing to the sliver geometry, and leaves the tree when
  unpinned; it fails with the paint skip or the pinned layout removed.
- `a_pinned_row_stays_announced_until_its_pin_is_released` (`crates/flui-widgets/tests/lazy_list.rs`):
  a pinned `ListView` row keeps its state and its node while scrolled away, and is evicted on the
  frame after the pin drops, with no scroll; it fails without the watcher notification or without
  the manager handing the pins to the render object.
- §3 is confirmed by `focused_row_is_pinned_while_scrolled_away` once the focusable list lands.

## Pros and cons of the options

### 1. Pin lease, laid out outside the band, skipped explicitly

- Good, because semantics and focus read a rect from the current pass.
- Good, because the band and the geometry stay a function of the scroll position alone.
- Bad, because paint and hit testing need an explicit skip the stamp gave for free before.

### 2. Keep the held child unlaid

- Good, because nothing changes.
- Bad, because the focused node leaves the accessibility tree the moment it leaves the band.
- Bad, because focus geometry and `ScrollIntoView` aim at a rect from a pass that no longer holds.

### 3. Grow the band to the focused row

- Good, because the row is an ordinary band member.
- Bad, because the band then spans every row between the viewport and the focused row: scrolling
  ten thousand rows away builds ten thousand rows.

### 4. Move the focused row into an overlay

- Good, because the list's band stays untouched.
- Bad, because the row changes parent while focused, which loses its render state, its semantics
  position in the list ("5 of 40") and every ancestor it reads inherited data from.

## More information

Flutter keeps a focused row alive through `AutomaticKeepAlive` and drops it from layout and
semantics outside the cache (`flutter/flutter@97cd226ec5ee4c4075b8516ea294152dba1fbfcc`
`packages/flutter/lib/src/rendering/sliver_multi_box_adaptor.dart`, `_keepAliveBucket`), so a
screen reader loses the focused row there too. Revisit if a list ever needs more than one pinned
row per focus owner, which would change the build bound in §3.
