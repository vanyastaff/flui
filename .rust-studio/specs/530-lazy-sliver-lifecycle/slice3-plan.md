# Slice 3 plan — key-first identity, remap, and the delegate panic boundary

Acceptance served: "Keyed children preserve state across insert/remove/reorder", "Stateful builder
children mount, update, and dispose correctly", "Builder panic recovery leaves a valid tree". Lands
BEFORE slice 2 (review §9 #3): static children move onto the lazy path only once keyed remap exists.

## Contract
- `SliverChildDelegate` (flui-view, object-safe): `build(index) -> Option<BoxedView>`,
  `find_index_by_key(&dyn ViewKey) -> Option<usize>`, `should_rebuild(&dyn SliverChildDelegate) -> bool`,
  `estimated_child_count() -> usize` (stays `usize`; growable count is a later issue).
  - `BuilderDelegate { count, builder, find_index_by_key: Option<Rc<dyn Fn(&dyn ViewKey) -> Option<usize>>> }`.
  - `ListDelegate { children: Vec<BoxedView>, key_map: OnceCell<HashMap<KeyId, usize>> }` — the map is
    derived lazily from the children's keys (Flutter's `_keyToIndexMap`), keys compared by `key_eq`.
- `SparseChildren` → resident record `{ element, key: Option<Box<dyn ViewKey>> }` (key captured at
  mount from `view.key()`, refreshed on update).

## The refresh, in Flutter's order (`widgets/sliver.dart:1008-1034`)
1. For every resident with a key: `new_index = delegate.find_index_by_key(key)`. If `Some(j) != i`:
   move the record to `j` (a `BTreeMap` remove/insert; if `j` was resident with a different element,
   that one is processed at its own index and evicted if unmatched). Clear its committed layout
   offset (the walk repositions from the virtualizer anyway).
2. For each index in the union of old and new positions: `delegate.build(index)`; `None` ⇒ evict and
   (if it was the last) clamp the count; `Some(view)` ⇒ `can_update_by_id` (type + key) ⇒ update in
   place, else evict + mount.
3. Band-local fallback for delegates without `find_index_by_key`: before step 2, learn
   `key → resident index` from the residents and match a built view's key against it (first-wins on
   duplicates, as `id_reconcile`); a key that left the band is not recoverable — documented as the
   same limit Flutter has without the callback.
4. After a move: `node.slot = j`; `tree.recompute_subtree_ancestry(element)` re-derives
   `sliver_slot` down the composite chain; `stamp_first_render_descendants(element, j)` re-stamps
   the parent data in place (slice 1's helper); the walk's uniqueness `debug_assert!` guards the
   invariant.
5. `did_finish_layout(first, last)` forwarded from `emit_retain_band` (cheap; Flutter parity).

## Panic boundary (per item, as Flutter)
- One helper `build_item_or_error(delegate, index) -> Option<BoxedView>`: `catch_unwind` around
  `delegate.build(index)`; on panic → `FlutterError::from_panic` + `ErrorView::build_error_view`,
  keyed with the index so it reconciles like any item. Used at all three call sites (`service`
  requests, `refresh_resident` step 2, and `find_index_by_key` — the callback is also user code).
- Documented escape: a panic in the item's own `create_render_object` during `on_mount` is not
  under this boundary (follow-up issue with the KeepAlive one).

## Tests (widget tier unless noted)
- Keyed insert-at-head on `ListView::builder` with `find_index_by_key`: item states (init counters)
  survive; on `ListView::builder` WITHOUT the callback the head insert loses the item that left the
  band and keeps the rest (band-local fallback) — both asserted, the second documented as Flutter's
  own limit.
- Keyed reorder (swap 3 and 5) on the builder delegate: `ViewState` identity preserved, parent-data
  index re-stamped (assert via the walk: no uniqueness assert, items painted at the new offsets),
  then `set_state` inside the moved item replaces its root render child → stamped with the NEW index
  (review §8 #3's scenario).
- Duplicate keys in one band: first wins, second remounts (no teleport).
- Builder panic at index k: `ErrorView` at k, neighbours intact, sliver geometry sane, next frame
  scrolls normally; panic inside `find_index_by_key`: same.
- `SparseChildren` unit tests for the remap bookkeeping.

## Out of scope (named)
- Static-children lists on the lazy path (slice 2), KeepAlive, growable count.

## Addendum (2026-09-03): `ErrorView` owns no render node
`crates/flui-view/src/view/error.rs`: `ErrorElement` mints no render object and no `RenderErrorBox`
exists in the workspace (`rg RenderErrorBox crates` → nothing). As a lazy child the default error
view is therefore invisible with zero extent — the sliver never sees a render descendant for that
index (it stays absent from `logical_to_slot`; the manager sees it built and skips, so no loop).
Flutter's `ErrorWidget` is a `RenderErrorBox` (red box, message in debug). Slice 3 adds
`RenderErrorBox` to flui-objects (paints the error red with the message via the text pipeline in
debug, a plain red box in release) and makes `ErrorElement` a render element over it — this also
serves #561's "render detailed local ErrorView in development". The `ERROR_VIEW_BUILDER` override
stays; a custom builder may return any view.
