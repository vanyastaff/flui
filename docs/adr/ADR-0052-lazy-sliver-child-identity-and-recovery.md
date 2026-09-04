# ADR-0052: Lazy sliver child identity, relocation, and per-item recovery

*A lazy sliver's resident children are reconciled against a changed data
source in two phases into a fresh map — every keyed resident is matched by
key wherever it now sits, relocated in place when its index moved, and
re-stamped — so keyed insert, remove, and reorder keep element state without
a callback for moves inside the built band, and with `find_index_by_key` for
moves out of it. A per-item wrapper carries the item's key salted, never as
its `GlobalKey`. A panicking item builder yields a render-owning error box at
exactly that index. A `GlobalKey` graft out of the list makes the list forget
the child.*

---

- **Status:** Accepted (2026-09-03)
- **Date:** 2026-09-03
- **Deciders:** @vanyastaff
- **Scope:** `SparseChildren` (`crates/flui-view/src/element/sparse_children.rs`),
  the lazy adaptors (`crates/flui-view/src/element/sliver_adaptor.rs`),
  `ChildManager::forget_child`, `ElementTree::relocate_sparse_child`,
  `SaltedKey` (`crates/flui-foundation/src/key.rs`), `RepaintBoundary`'s
  forwarded key, `RenderErrorBox` (`crates/flui-objects/src/proxy/error_box.rs`),
  `ErrorView` as a render view.
- **Related:** [ADR-0050](ADR-0050-global-key-identity-and-frame-reservations.md)
  (GlobalKey identity and the duplicate verdict), [ADR-0051](ADR-0051-anchor-stationary-scroll-correction.md),
  [ADR-0003](ADR-0003-virtualization-core-and-reentrant-build.md), issue #530.

## Context

Flutter's `SliverMultiBoxAdaptorElement.performRebuild` (`widgets/sliver.dart`)
rebuilds every resident index when the delegate changes. It first remaps
keyed residents through `delegate.findIndexByKey`, writes the remapped
children into a *second* map (`newChildren`), then processes each index with
`updateChild`. Two properties of that shape are load-bearing: the second map
(an in-place remap of one index-keyed map overwrites a resident on any shift
of two or more), and the key match happening only through the callback (a
`SliverChildListDelegate` derives one; a builder delegate without
`findChildIndexCallback` loses state on reorder).

Flutter restores each item's key outside the per-item `RepaintBoundary` with
a `KeyedSubtree` carrying a `_SaltedValueKey` (`widgets/scroll_delegate.dart`).
FLUI's wrapper must own the render node the sliver reads, so it carries the
key itself; forwarding the raw key made the wrapper and the item both
register a `GlobalKey` — a debug panic at mount, a duplicate report in
release — so no lazy item could carry a `GlobalKey` at all.

`SliverChildBuilderDelegate.build` wraps `builder(context, index)` in its own
try/catch and substitutes `ErrorWidget.builder(details)`, a `RenderErrorBox`.
FLUI's item builder was called bare, and `ErrorView`'s element owned no render
node — as a lazy child it would have been a phantom index the sliver never
laid out.

## Decision

1. **Two-phase keyed reconcile into a fresh map** (`SparseChildren::reconcile`).
   Snapshot every resident `(index, element, key)`; build every resident index
   inside the band the layout pass retained (a keyless resident outside it is
   carried over untouched for the band eviction that follows — never rebuilt,
   never mounted fresh out of band) plus every keyed resident's index and, for
   each keyed resident, the index `find_index_by_key` reports; match
   each built view to the first unclaimed resident with an equal key wherever
   it sat (first wins on duplicate local keys, as the dense reconciler does),
   or positionally for keyless views of the same type; apply into a fresh map —
   update in place, relocate first when the index changed, mount the unclaimed
   views, evict the unclaimed residents. **Improvement over Flutter:** a keyed
   move *within* the built band needs no callback; `find_index_by_key` only
   widens the set of indices to build (Flutter's callback is the only way it
   matches at all). A move out of the band without the callback loses state,
   as in Flutter; that limit is documented on the API.
2. **Relocation is one tree operation** (`ElementTree::relocate_sparse_child`):
   the slot is written first, the inherited `sliver_slot` is re-derived down the
   composite chain, then the render descendants are re-stamped in place — so a
   later `set_state` inside a moved item mounts its render child under the new
   index. The band walk's uniqueness `debug_assert!` guards the invariant.
3. **The wrapper's key is salted** (`SaltedKey`): equal only to another salt of
   an equal item key, hashed apart, never `is_global_key`. A delegate's
   `find_index_by_key` sees the item's own key through `SaltedKey::unsalt`.
4. **Per-item recovery** (`build_item_or_error`): every builder call — for a
   requested index, for a reconciled index, and the `find_index_by_key`
   callback — runs under `catch_unwind`; a panic yields the registered error
   view for that index only, unkeyed (so it can never claim a user's keyed
   item), updating in place while the panic persists. `ErrorView` is a render
   view over `RenderErrorBox`, which fills a bounded axis and takes a finite
   fallback row on an unbounded one (Flutter's `100000 × 100000` would swallow
   a scroll extent), paints the message in debug builds only, and reports it to
   diagnostics in every build.
5. **A losing list forgets a grafted child** (`ChildManager::forget_child`,
   driven from the active `GlobalKey` retake): its bookkeeping drops the
   element so a later band eviction or delegate refresh never reaches into the
   new parent's subtree. Flutter's `forgetChild`.

## Consequences

- Keyed insert/remove/reorder on `ListView::builder` / `GridView::builder`
  preserve `ViewState`; `GlobalKey`'d items mount under the per-item boundary;
  a panicking item renders a visible error row and its neighbours are intact.
- `RenderErrorBox` joins the render-object catalog (harness-tested).
  `ErrorElement` and `ElementKind::Error` are gone: an error view is an
  ordinary render element. A render element mounted without a `PipelineOwner`
  (a render-less unit-test tree) now treats `update` as a no-op; the two
  half-states (owner without render id, render id without owner) stay `BUG:`
  panics.
- **Recorded divergences.** `_replaceMovedChildren` (Flutter: `SliverList`
  only) is effectively always on: a moved keyed resident's old index is rebuilt
  because every keyed resident's index is; the extra build is evicted by the
  next band at worst. **Improvement over Flutter:** `performRebuild` rebuilds
  every resident and `collectGarbage` drops the out-of-band ones afterwards;
  FLUI reconciles before it evicts (so a keyed item can move with the viewport
  and keep its state) and therefore skips the keyless residents the band is
  about to drop — a scroll that rebuilds the host costs no builder call for an
  item it does not keep (`grid_view_builder_does_not_cache_item_builder_calls_across_scroll`
  pins the count). The `_didUnderflow` look-ahead one past the last key is covered
  structurally: the adaptor's render update forces a layout, and the band walk
  re-requests the next index inside the same frame's fixpoint. `updateChild`'s
  layout-offset preservation across a render swap is covered by the walk
  rewriting every in-band offset from the virtualizer each pass.
- **Gap recorded here, closed by issue #838 (2026-09-04).** A `GlobalKey`'d
  *descendant* of an unkeyed subtree that a parent removes used to be unmounted
  at once, where Flutter deactivates the subtree and lets another parent retake
  the descendant before `finalizeTree`. It was tree-wide (dense parents too),
  and it surfaced here because the per-item `RepaintBoundary` is itself an
  unkeyed wrapper: a lazy item under one could not be grafted to another list
  with its state, so the graft test had to turn boundaries off to run at all.
  `remove_subtree` now stops its walk at a keyed descendant and routes it
  through the soft-remove path instead — not descending is the other half,
  since a retaken element keeps its own children. The graft test runs with the
  default boundary on, which is what an app would actually write, and is red
  without the change.

  Two things the first draft of that change got wrong, both caught in review.
  It described itself as tree-wide while covering only the sparse and root
  paths: an ordinary DENSE parent removes through
  `id_reconcile::remove_child`, which carried its own copy of the same subtree
  walk and kept freeing keyed descendants. The copies are gone — that path
  delegates to `remove_subtree`, so there is one implementation and it cannot
  drift again. And deactivation is not right for every caller:
  `detach_root_widget` is permanent teardown with no frame after it, so a
  deactivated element would sit in the inactive queue with `dispose` never
  run and a later attach could retake stale state. `SubtreeRemoval` makes the
  two cases explicit at the call site rather than assuming one behaviour fits
  both.

## Alternatives considered

- **In-place single-map remap.** Rejected on review: any shift or swap of two
  keyed residents orphans one element (still attached, stamped, unevictable)
  and trips the band walk's uniqueness assertion.
- **Band-local key matching only, before building.** Rejected: it needs the
  built views to match against; it is phase two of the design, not a
  pre-step.
- **Forwarding the raw item key on the boundary.** Rejected: the `GlobalKey`
  double registration above.
- **An index-keyed error view.** Rejected: `key_eq` to a user's
  `ValueKey<usize>` lets `find_index_by_key` move the error element onto the
  user's item.
