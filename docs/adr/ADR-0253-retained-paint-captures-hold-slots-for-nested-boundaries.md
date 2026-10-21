# ADR-0253: A retained paint capture holds a slot for each nested boundary

- **Status:** Accepted
- **Date:** 2026-10-21
- **Amends:** `crates/flui-layer/ARCHITECTURE.md` (retained paint, item 3) and the
  "A composited-layer update patches the enclosing capture" entry of
  `crates/flui-rendering/ARCHITECTURE.md`

## Context

A `RetainedSubtree` is the flattened layer output of one repaint boundary, **including the layers
of every boundary nested in it**. Two things follow:

- A dirty nested boundary invalidates every capture that embeds it. `run_paint` declines to graft
  an enclosing boundary while any entry of its `nested_boundaries` is queued, and re-runs
  `paint_raw` for each of its inline nodes, then each clean sibling boundary is grafted.
  A caret blinking in a `RepaintBoundary` in a list item in a `Scaffold` re-records the app bar,
  the background and every item's non-isolated nodes.
- The root boundary is never retained, so its own region repaints every frame the walk starts.

The embedding is also the source of the bug class around it: `nested_boundaries` is checked at
graft time, and again by two eviction loops and a "re-note the capture's list" step, because a
stale inner capture inside a reused outer one replays old output that nothing repaints. A layer
update served from the outer capture patches only that copy.

Content tokens already survive an outer re-descent (`ContentToken` is kept while the boundary is
absent from the queue), so the damage differ is not misled; what is wasted is the recording and
the repeated copies of the same layers (memory is O(depth x layers)).

## Decision

1. **Disjoint captures.** A capture holds its own region only:

   ```text
   RetainedSubtree {
       own:   Vec<RetainedNode>,            // layers painted by this boundary's nodes
       slots: Vec<NestedSlot>,              // one per nested boundary directly inside
       effect_slots, content,
   }
   NestedSlot { boundary: RenderId, parent: Option<usize>, origin: Offset }
   ```

   The nested boundary's `OffsetLayer` is **not** part of the enclosing capture's `own`: the slot
   owns it. At replay the slot mints that layer from the boundary's *current* layout offset and
   its *current* content token (kept when the boundary is clean, freshly minted when it was
   repainted), so a repainted nested boundary always carries a new stamp for the damage differ,
   and a boundary that moved needs only the new offset. `origin` is the slot's position in its
   parent's layer space; the stored value is relative, never an absolute anchor.

2. **Replay recurses through slots.** Grafting a capture mints its own nodes, and at each slot
   either grafts the nested boundary's capture or, when that boundary is queued for a repaint or has
   no capture, runs `paint_subtree` for it and records a fresh capture. The replay carries the
   parent's `AnchorFrame` down and rebases it at each slot (`frame.rebased(origin + current
   offset)`), exactly as the painting walk does, so a nested painter that reads `paint_anchor`
   sees the position after an ancestor moved the boundary. A missing capture is a per-slot repaint, not an eviction of its ancestors.

3. **A clean boundary is replayed whatever is nested.** The graft-time `nested_boundaries` check,
   `note_boundary` and `capture_scopes`, the two ancestor-evicting `retain` loops and the "re-note on
   graft" step are deleted: with disjoint captures there is no embedded copy to go stale.

4. **The root is a boundary like the rest.** It is retained and replayed when it is not queued; the
   root stamp logic stays as is.

5. **Layer updates stay addressed by the owning boundary** (`layer_updates` is already keyed
   by it); a patch now touches the one capture that owns the node, so the "served here, stale
   there" hazard has no path.

6. **Unchanged:** the layer tree a frame emits (shape, stamps, tokens), `Leader`/`Follower`
   eviction (per boundary: one that holds a link layer in its own region is not retained and
   repaints), damage pairing, and the cost of re-composing the output tree, which stays
   O(layers) of `Arc` clones. Persisting the layer tree across frames is a separate change.

## Diagram

```
 Root --- AppBar            (own)               before: dirty C => repaint Root, B
   |                                                    (Root.own + B.own re-recorded;
   +-- B (boundary) --- Item (own)                       A, D grafted from copies)
   |     |
   |     +-- C (boundary, dirty) -- caret          after:  dirty C => only C.own recorded
   |     +-- D (boundary)                                  Root, B, D replayed from captures
   +-- A (boundary)
                                                 Root.slots = [B, A]   B.slots = [C, D]
 replay(Root): own, slot B -> replay(B): own, slot C -> paint(C) fresh, slot D -> graft(D)
```

## Consequences and verification

- Paint cost for a nested-boundary-only mark is the dirty boundary's own subtree plus an
  O(layers) re-compose; capture memory is O(layers).
- Tests, in `crates/flui-rendering/tests/` as one module of `main.rs`:
  `nested_boundary_mark_paints_only_that_boundary` (`nodes_painted` equals the dirty boundary's
  subtree), a differential test that runs random trees with random marks and compares every frame's
  layer tree with a fresh owner's full repaint (the oracle that the retained output is the painted
  output), a repainted nested boundary re-stamps its offset layer with the new token, a moved
  boundary replays with a rebased anchor, a missing nested capture repaints that slot only, the layer-update cases of the
  existing suite (nested, two levels, patch then repaint), and the root retained when clean.
  Existing graft tests that assert eviction of ancestors are rewritten to assert replay.
- `RetainedSubtree` loses `nested_boundaries`; the `paint` bench gains a nested-caret arm.
- Built in order: disjoint captures and slot replay, then the retained root.

## Alternatives rejected

- **Keep the embedding and repair it** (more graft checks, patch every copy). It keeps O(depth x
  layers) memory and the whole bug class above.
- **Promote every effect node to a boundary** to shrink regions. Rejected by the existing entry on
  composited-layer updates; it multiplies layers.
- **A persistent layer tree edited in place.** The larger win, but it changes the engine contract
  for scenes; this ADR is the prerequisite, not the competitor.

## Follow-up

A persistent layer tree edited in place across frames would remove the remaining O(layers)
re-compose. It changes the scene contract with the engine and is a separate decision.
