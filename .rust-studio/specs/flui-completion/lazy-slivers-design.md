# C1 — Lazy Slivers: design notes

Branch `core2-lazy-slivers` (off main `acf706ea`). Goal: real Flutter-parity lazy
on-demand child building for scrollable viewports — children built/laid-out only
within `scrollOffset .. scrollOffset + remainingCacheExtent`, NOT eagerly. No
MVP-as-parity (AGENTS.md DoD): the reentrant build-during-layout seam is the point.

## Reference contract (`.flutter/`, verified by reading)

The full chain (top → bottom):

1. `RenderSliverList.performLayout` (`rendering/sliver_list.dart`) walks the child
   linked list; when it needs a child not yet present it calls
   `addInitialChild` / `insertAndLayoutLeadingChild` / `insertAndLayoutChild`.
2. Those (`rendering/sliver_multi_box_adaptor.dart`) call
   `_createOrObtainChild(index, after)` which wraps the mutation in
   **`invokeLayoutCallback<SliverConstraints>((c) { childManager.createChild(index, after) })`**.
   - `invokeLayoutCallback` = the sanctioned mid-layout render-tree-mutation window
     (sets `_doingThisLayoutWithCallback`; normally inserting/removing children
     during layout is forbidden). **THIS is the keystone seam FLUI lacks.**
   - keepAlive children are pulled from `_keepAliveBucket` instead of rebuilt.
3. `childManager` = `SliverMultiBoxAdaptorElement` (`widgets/sliver.dart`):
   `createChild` opens **`owner.buildScope(this, () { updateChild(_childElements[index], _build(index), index) })`** — a NESTED reentrant build of ONE child element,
   mid-layout. `_build(index)` → `SliverChildBuilderDelegate.build(ctx, index)` → View.
   `updateChild` reconciles → child element → child render box →
   `insertChildRenderObject(child, after-slot)` into the sliver's render child list.
4. `collectGarbage(leading, trailing)` removes off-cache-extent children:
   `_destroyOrCacheChild` → keepAlive bucket OR `childManager.removeChild` (unmount).
5. `estimateMaxScrollOffset` (childCount known → exact; unknown → estimate from laid
   children) feeds `SliverGeometry.scrollExtent` so the scrollbar/overscroll work.
   `SliverGeometry.scrollOffsetCorrection` re-runs layout when leading insertion
   reveals the estimate was wrong.

Child parent-data (`SliverMultiBoxAdaptorParentData`): `index`, `layoutOffset`,
`keepAlive`, `_keptAlive`, + linked-list `previousSibling`/`nextSibling`.

## VERIFIED STATE (ground-truthed 2026-06-28, not scout-trusted)

The render foundation is **DONE & mature** per **ADR-0003** (virtualization-core-and-
reentrant-build): U1 `Virtualizer` (SumTree, O(log n)), U2 mid-pass-*capable* build
contract (`ChildLayout`/`BoxChildRef`, v1 **next-frame** backend via deferred-mutation
queue; U3c closed logical-index + dispose gaps), U3 `RenderSliverListLazy`
(`sliver_list_lazy.rs`, 60+ tests, anchor-correction, backward-scroll suppression).
The remaining gap is **ADR-0003 U4** (flui-view/flui-widgets lazy widgets).

**THE IMPEDANCE MISMATCH (the real reason U4 is design-heavy):**
`RenderSliverListLazy.child_source: Arc<dyn Fn(usize) -> Option<Box<dyn
RenderObject<BoxProtocol>>>>` yields a **detached, owned single render object** per
index. But FLUI's three-tree model makes render objects **arena-resident, created and
owned by elements**. An arbitrary child View (`Container(Padding(Text))`) is an element
*subtree*, not one `Box`. So the `Fn->Box` seam is a **render-only demo seam** (doctests
pass bare render objects; ADR-0003: "No flui-view consumer is required to validate the
core") — it does NOT directly host arbitrary child Views.

Eager `ListView` (verified `list_view.rs`) passes `Vec<BoxedView>` to
`SliverFixedExtentList` via the **normal element-child mechanism** (arena children laid
out by `ctx.layout_sliver_child(index)`), bypassing the `Fn->Box` closure entirely.

**Design direction (Flutter-parity, ADR-0003-aligned):** the lazy element owns a sparse
`index -> child ElementId` map and builds child element subtrees **on demand**, arena-
resident; the lazy sliver references them as real children built through a reentrant
single-child build via the deferred queue (v1 next-frame). This likely needs the build
backend to yield an **arena RenderId** (element-built child), not a detached `Box` —
either a new seam alongside `child_source`, or feeding `build_and_layout_box_child` from
an element that materializes the subtree first. Resolve in the architecture design pass.

## FLUI port — required new pieces (fill "exists?" from scout map)

| # | Piece | Layer | Notes |
|---|-------|-------|-------|
| 1 | `invokeLayoutCallback` equiv — sanctioned mid-layout mutation window | flui-rendering pipeline | reentrancy into build while subtree_arena holds layout borrows — the RISKY core bit |
| 2 | reentrant single-element `build_scope` callable during layout | flui-view BuildOwner | current build_scope is top-down take/put (PR-K). Need a nested-safe variant |
| 3 | `RenderSliverMultiBoxAdaptor` base + ordered child list + parent-data (index/layoutOffset/keepAlive) | flui-objects | FLUI uses Slab arenas — need ordered child iteration (firstChild/childAfter) |
| 4 | `RenderSliverList` performLayout (+ `RenderSliverFixedExtentList` fast-path) | flui-objects | port the algorithm above |
| 5 | child-manager seam trait (createChild/removeChild/estimateMaxScrollOffset/childCount/setDidUnderflow) | flui-rendering (trait) ↔ flui-view (impl) | the render↔element bridge |
| 6 | `SliverMultiBoxAdaptorElement` equiv — sparse `BTreeMap<usize, ElementId>` child map, implements seam | flui-view | holds the build-backend |
| 7 | `SliverChildBuilderDelegate { builder: Fn(usize)->View, child_count }` + `SliverChildListDelegate` | flui-widgets | |
| 8 | `SliverList` / `ListView.builder` widgets wiring delegate → element → render | flui-widgets | replace current eager ListView |

## Open design questions (resolve before coding)

- Q1: Can FLUI's subtree_arena layout borrows be safely suspended for a reentrant
  build window, or does mid-layout child-build need a deferred-insertion queue
  (build into a staging area, splice after the borrow is released)? **Decides #1/#2.**
- Q2: Slab-arena ordered child list — reuse existing multi-child render storage or
  add an explicit intrusive linked list in parent-data? Flutter relies on
  firstChild/childAfter ordering heavily.
- Q3: scrollOffsetCorrection loop — how does RenderViewport re-run sliver layout in
  FLUI's pipeline (Flutter loops in `RenderViewportBase.performLayout`).

## STATUS (2026-06-28)

- **U4.1 `SparseChildren`** (flui-view) — built, 5 tests green. Held **uncommitted** until U4.3 (its
  consumer) exists, so no dead code lands.
- **U4.2 render→element request seam** — IN REVIEW on branch `core2-lazy-slivers` (commits `3454ad50`
  band-walk extraction into `virtualized_band.rs`; `a7fa68d1` `pending_child_requests` sink +
  `request_child_build -> ChildLayout<BoxChildRef>` + `RenderSliverList`). Gates green; unsafe-auditor
  SAFETY-GATE signed; spec-compliance (5a) passed; rust-reviewer 5b returned a test-gap (residents-at-offset,
  off-band dispose, scroll_extent==estimate, forward-only correction) → builder closing it. The plan +
  3-agent review synthesis live in `C:\Users\vanya\.claude\plans\bubbly-imagining-comet.md`.

### U4.3 launch-ready notes (verified)

- **Frame-driver insertion point** (`crates/flui-binding/src/lib.rs` `pump_frame`, ~:333): phases are
  clock→`poll_deadlines`→`vsync.tick_all`→`build_scope`→`run_frame` (run_frame = layout+paint+composite;
  `layout_dirty_root` drains `pending_child_requests` into the `PipelineOwner` field during it). The U4.3
  post-layout phase slots **after** `run_frame` restores the owner: `pipeline_owner.take_pending_child_requests()`
  → route each `(sliver RenderId, logical_index)` to the managing lazy `ElementId` (O1 registry) →
  `SparseChildren::ensure`/`evict` → a **second** `build_scope` to flush new child subtrees → mark slivers
  needs-layout → children lay out next `pump_frame` (the honest next-frame settling).
- **O1 (registry):** no `RenderId→ElementId` reverse map exists; need a `ChildManagerRegistry: HashMap<RenderId,
  ElementId>` populated when a lazy adaptor element mounts its sliver (element knows its `render_id` at
  `behavior.rs:789`). Placement (binding vs BuildOwner) is the U4.3 ARCH-GATE call.
- **O2:** the second `build_scope` must only build freshly-mounted lazy children (dirty heap empty after the
  first drain) — assert with a build-count test.

## Phasing (each gate-green, tested, honest scope)

- **P0 (design lock):** answer Q1–Q3 against scout map + adversarial review. NO code.
- **P1:** `RenderSliverMultiBoxAdaptor` base + parent-data + ordered child list +
  child-manager TRAIT, with a *test-only synchronous* manager (children pre-supplied)
  → port `RenderSliverList.performLayout` + harness tests. NO element reentrancy yet.
- **P2:** the reentrancy seam (#1/#2) — `invokeLayoutCallback` window + nested
  build_scope; unit-test with a tiny element that builds on demand.
- **P3:** `SliverMultiBoxAdaptorElement` + delegate + `SliverList`/`ListView.builder`;
  integration tests (scroll a 10k-item list, assert only cache-extent children built).
- **P4:** keepAlive bucket + `RenderSliverFixedExtentList` fast-path + garbage parity.
</content>
</invoke>
