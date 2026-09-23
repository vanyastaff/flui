# ADR-0003: Protocol-agnostic virtualization core; lazy slivers build their band through the layout↔build fixpoint

- **Status:** Accepted
- **Date:** 2026-06-12
- **Related:** ADR-0017 (the layout↔build fixpoint), ADR-0051 (anchor-stationary scroll correction)

## Context

A virtualized list lays out only the items that intersect the viewport plus a cache band. Doing
that well needs four pieces of windowing math: an `O(log n)` map between scroll offset and item
index in **both** directions over the running sum of item extents; `O(log n)` structural edits
(mid-list insert/delete) so dynamic lists (`set_count`, infinite feeds, reorder) stay cheap; an
estimate for unmeasured items so the total extent and the scrollbar are stable before everything
has been laid out; and an anchor correction that keeps on-screen content still when a measured
extent differs from its estimate. None of this is render-specific — it is equally the math behind
a list, grid, table, timeline or virtualized text.

Flutter's `RenderSliverList` is the thing to beat: linked-list child tracking with
estimate-based scroll-extent jitter, a cache extent recomputed inline every scroll frame, and a
raw-pixel anchor that jumps when an item above the viewport is re-measured (flutter/flutter#97676).

Market survey:

| Engine | Backing structure | Seek | Mid-list edit | Anchor |
|---|---|---|---|---|
| GPUI / Zed `list.rs` | `SumTree<ListItem>`, `Unmeasured \| Measured`, `{count, height}` summary | `O(log n)` both ways | `O(log n)` | item identity (`ListOffset`) |
| TanStack Virtual | flat array + binary search | `O(log n)` | `O(n)` rebuild | item key |
| Flutter `RenderSliverList` | linked list + `scrollOffsetCorrection` | linear walk | splice | raw pixel |
| Compose `LazyColumn` | none, re-derived per frame | — | — | item key |
| egui | uniform row height only | `O(1)` | trivial | — |
| Xilem | fixpoint measure loop | iterative | iterative | convergence |

## Decision

### 1. A deep, protocol-agnostic `virtualization` module in `flui-rendering`

`flui_rendering::virtualization` owns windowing math only. Its public surface names no render,
sliver or protocol type — `ScrollWindow`, `VisibleRange`, `AnchorCorrection`, `Extent`,
`ItemExtent`, `Virtualizer` and plain `f32` extents — so it stays a general-purpose abstraction.
The `SliverConstraints → ScrollWindow` adaptation lives in the consumer (`RenderSliverList` and
`virtualized_band` in `flui-objects`), never inside the module; that also keeps the dependency
acyclic should the module ever become its own crate.

- **Backbone: a focused augmented B+-tree (SumTree-style)** over per-item extents, with an
  `{ item count, total extent }` summary at every internal node (`virtualization/sumtree.rs`).
  `O(log n)` seek in both directions and `O(log n)` insert/delete. It is focused (count + extent),
  not a fully generic `SumTree<T, Summary>`: generality lives at the `Virtualizer` boundary.
- **Measured vs estimated is type-level:** `ItemExtent::{Unmeasured { hint }, Measured { extent }}`,
  not a side boolean. `total_extent()` returns `Extent::{Exact, Estimated}`, with
  `measured_count` / `estimated_count` for scrollbar stability.
- **The anchor is item identity, not a raw pixel:** `(index, sub_offset)`, as in GPUI, Compose and
  TanStack. `set_measured(index, extent, anchor)` returns a signed `AnchorCorrection` iff the change
  shifts content above the anchor item; an out-of-range anchor is ignored, never clamped into a
  fabricated one. When the caller applies it is the consumer's policy (ADR-0051), not the core's.
- **`query(&self, &ScrollWindow) -> VisibleRange` is a pure read** returning both the tight visible
  range and the cache range plus the leading item's offset. The core holds no viewport state;
  `scroll_to_item` takes the viewport extent as an argument.
- Fixed-extent lists need no tree (`offset = index × extent`); only variable extents use it.

The module stays inside `flui-rendering` because it has one direct consumer; lazy widgets reach it
through the slivers, not directly. A crate boundary would be a shallow component paid for by one
consumer. Because the public surface is protocol-free, lifting it into its own crate later is a
mechanical, non-breaking move.

### 2. Children are built through the layout↔build fixpoint, at whole-pass granularity

The core is build-agnostic: it answers visible range and anchor only. A lazy sliver, during its
own layout, calls `request_child_build(logical_index)` for a missing child and
`emit_retain_band(first, last)` for the band it wants kept. The element tree's `ChildManager`
services those requests **between layout passes inside the frame** — ADR-0017's fixpoint, bounded
by `MAX_LAYOUT_BUILD_PASSES` and the softer `MAX_LAZY_BAND_PASSES` — and a post-frame service picks
up whatever the budget left over.

`ChildLayout` is `{ Scheduled, NoChild, Unwired }`: a request is answered at whole-pass
granularity — the child is built after the pass that asked for it and laid out in the next pass of
the same frame. What the original design wanted from a mid-pass build — a fresh band materialised
in the frame that needed it, no blank frame, no one-frame-behind scroll — is delivered by the
in-frame fixpoint instead of by re-entrancy.

The cost: under a wrong extent estimate a band converges over several passes. The band walk
therefore adapts its estimate to the band's own measured mean and re-queries in the pass that
measured it (ADR-0051), and a band that still has not settled when its budget trips defers the
remainder to the next frame instead of hitting the fixpoint's `BUG:` bound.

**Re-adding a true mid-pass build** (Compose `SubcomposeLayout` style) is a breaking change to
`SliverLayoutCtxErased` and `ChildLayout`. It returns only with a consumer that measures a real
cost of whole-pass granularity.

### 3. Eviction has one owner; no recycling pool

A child that leaves the retain band is disposed. Eviction has exactly one owner: the element
tree's `SparseChildren::retain_band`, driven by the band the render object emitted. With one owner
the double-remove hazard of two competing disposal paths cannot arise.

RecyclerView's two-level recycling pool is not adopted. Its justifying constraint — expensive
`View` inflation under GC — does not hold for a Rust arena with cheap `can_update` reconciliation.
Pooling is added only if a real-frame benchmark shows the need.

## Divergence from Flutter

- **Item-identity anchor** instead of Flutter's raw-pixel anchor, so re-measuring an item above the
  viewport never moves on-screen content (the #97676 class of jump).
- **Seek over a SumTree** instead of dead-reckoning from a linked list of resident children.
- **Build happens between layout passes, not inside `performLayout`.** Flutter's
  `invokeLayoutCallback` mutates the trees mid-walk under a debug flag; FLUI cannot while the layout
  walk holds `&mut RenderTree` (ADR-0017). The observable result — the band is present and painted
  in the frame that asked for it — matches.

## Consequences

- `Virtualizer` is testable in isolation (`virtualization/tests.rs`, `benches/virtualizer.rs`) and
  through the sliver harness, with no widget layer.
- `RenderSliverList` lays out only the visible-plus-cache band; its win is measured against a real
  frame, not asserted.
- A frame can take several layout passes while a band converges; steady state is one.
- Mid-pass build is no longer reachable without a breaking change — a deliberate trade, recorded in
  Decision 2.

## Rejected alternatives

- **A separate `flui-virtualization` crate now.** One direct consumer; a crate boundary buys
  nothing it can pay for. Extraction stays cheap because the surface is protocol-free.
- **A `SliverProtocol`-bound `Virtualizer`.** Serves only slivers, and naming `SliverConstraints`
  would make an extracted core depend on `flui-rendering` — a cycle.
- **A Fenwick tree / BIT backbone.** `O(log n)` prefix sums but a flat array, so mid-list
  insert/delete is `O(n)`. Correct only for append-only lists; the old `FenwickExtents` had no
  callers and was deleted.
- **A render-owned mid-pass build path** (render objects inserting children through a
  deferred-mutation queue). It was built and deleted: no view ever produced it, and the fixpoint
  delivers the same observable result with one build path.
- **A RecyclerView-style recycling pool.** See Decision 3.

## References

- GPUI / Zed `crates/gpui/src/elements/list.rs`; Zed's rope/SumTree write-up
  (<https://zed.dev/blog/zed-decoded-rope-sumtree>).
- flutter/flutter#97676 (raw-pixel anchor jitter).
- TanStack Virtual (estimate-then-correct); WICG Scroll Anchoring (anchor by node, not pixel).
