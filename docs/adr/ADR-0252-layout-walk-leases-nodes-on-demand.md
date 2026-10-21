# ADR-0252: The layout walk leases nodes on demand, so its cost follows the work

- **Status:** Proposed
- **Date:** 2026-10-21
- **Amends:** the render-tree storage entry of `crates/flui-rendering/ARCHITECTURE.md`
  ("Render-tree storage uses a `Slab<RenderNode>`")

## Context

`layout_dirty_root` prepares a walk before any `perform_layout` runs:

1. `collect_subtree_ids` visits every descendant of the dirty root and hashes it into a `HashSet`;
2. `get_subtree_mut` resolves each id, builds a `HashMap` of wanted slots, then iterates the
   **whole slab** (`collect_disjoint_mut`), so it also pays for nodes outside the subtree;
3. `SubtreeArena::new` builds a third map, `RenderId -> (NodePtr, AtomicBool)`.

A scroll frame that marks one viewport (a relayout boundary) therefore costs O(subtree) plus
O(slab) and three allocations, although the walk stops at the first cache hit under every item.
A single dirty leaf under the window's root boundary touches the whole tree. Relayout boundaries
and the layout cache bound the *work*; the preparation does not follow that bound.

The earlier attempt to acquire lazily stopped on a real constraint: `slab::Slab` offers no raw
per-slot pointer, and `Slab::get_mut(&mut self)` while another node's `&mut` is live is undefined
behaviour (Miri). Pruning the pre-collection at clean relayout boundaries is unsound too, since a
parent can hand a clean boundary new constraints.

## Decision

1. **Own the slot storage.** `RenderTree` keeps nodes in a `NodeSlab`: a `Vec` of slots with a
   free list, each slot `{ in_flight: Cell<bool>, node: UnsafeCell<RenderNode> }`. The
   generation table stays where it is. The `slab` dependency leaves `flui-rendering`.

2. **One lease per walk.** `RenderTree::lease(&mut self) -> TreeLease<'_>` takes the slot base
   pointer once (from `Vec::as_mut_ptr`, so every slot pointer has whole-allocation provenance)
   and holds the exclusive borrow of the tree for the walk. No `&mut Slab` is formed again until
   the lease drops, so the Miri objection above cannot arise.

3. **Acquire along edges.** `TreeLease::acquire(parent, child) -> Result<NodeGuard, RenderError>`:
   resolves `child` with the existing generation check (O(1)), rejects an id that is not a
   current child of `parent` (`NodeNotFound`, as an id outside the subtree is today), sets the
   slot's `in_flight` bit or returns `LayoutCycle`, and yields the `&mut RenderNode` for exactly the
   guard's life. The bit lives beside the `UnsafeCell`, never inside it, so reading it while a
   node's `&mut` is live up the stack touches no aliased memory. This is the invariant the arena's
   side map provides today, without the map.

4. **`SubtreeArena` becomes a thin walker over the lease.** Its sinks (`laid_out`, failures,
   poison retries, child requests) stay as they are; `by_id`, `parent_data_seeds` collection and
   `from_tree` go. The test-only parent-data seed lookup becomes a lookup by id on demand.

5. **Dead code goes.** `collect_subtree_ids` (one remaining caller in `accessors.rs`, which takes
   a plain iterator walk instead), `get_subtree_mut` and `collect_disjoint_mut` are deleted.

## Diagram

```
 before                                   after
 ------                                   -----
 dirty root R                             dirty root R
   |                                        |
   v  O(subtree) + O(slab)                  v  O(1)
 collect ids -> HashSet                   lease(&mut tree)  -- base ptr taken once
 resolve ids -> HashMap                     |
 iterate whole slab                         v  per layout_child(parent, child)
 build by_id HashMap                      acquire: resolve gen  -> edge check
   |                                         -> in_flight bit   -> &mut node
   v                                         |
 walk (stops at cache hits)               walk (stops at cache hits)  O(nodes reached)
```

## Consequences and verification

- Layout preparation is O(nodes the walk reaches), with no allocation proportional to the subtree.
  `PipelineCounters` gains `nodes_acquired`, so the claim is a counter, not a stopwatch.
- Tests (in `crates/flui-rendering/tests/`, one module of `main.rs`):
  `a_scroll_frame_acquires_only_the_nodes_it_lays_out` (a 10k-item list, one viewport mark:
  `nodes_acquired` equals laid-out plus cache-hit roots, not the item count), re-entry of an
  ancestor still reports `LayoutCycle`, an id outside the walked subtree still reports
  `NodeNotFound`, a panicking `perform_layout` leaves no `in_flight` bit set, and the existing
  poison and degradation tests run unchanged. The `pipeline::owner` Miri sweep covers the new
  `unsafe`, which stays in the one sanctioned file.
- `layout/scroll_10k` joins the frame benches.
- Out of scope: pruning the walk itself; the layout cache already does that.

## Alternatives rejected

- **Prune the pre-collection at clean relayout boundaries.** Unsound: a parent can hand a clean
  boundary new constraints, and the boundary must then lay out.
- **Keep `Slab` and cache the arena between frames.** The cache would need invalidation on every
  structural change, which is the bug class this walk exists to avoid.
- **A per-node atomic flag inside `RenderNode`.** Reading it while the node's `&mut` is live up
  the stack is the aliasing read the side bit exists to avoid.
