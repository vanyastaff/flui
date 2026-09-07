# 536 slice 1 — give a boundary's layers an identity that survives the frame

Scoping pass only; nothing implemented. Every claim below was checked against
the tree on 2026-09-07, because ADR-0061 is from 2026-08-16 and one of its
statements has drifted.

## What ADR-0061 settled

Damage is **not** derived from the paint phase — the render objects that always
repaint cover the screen. It comes from comparing consecutive layer trees,
which needs layers to be pairable across frames. The ADR names two
prerequisites, one present and one missing.

**Cheap comparison — present.** `PictureLayer` holds `Arc<DisplayList>` since
#755 and a graft re-inserts clones of the same layers, so `Arc::ptr_eq` answers
"is this content unchanged" in constant time.

**Pairing across frames — missing.** Each frame builds a fresh `LayerTree` with
different slab indices, so `LayerId` pairs nothing.

## Correction to the ADR, verified today

The ADR says `LayerNode::element_id` "is always `None` in production" and that
`LayerTree::insert_with_element` "is never called from the paint path". Both
are still true in effect, but the situation is better than that reads:

- `insert_with_element` has **no caller anywhere** (the `flui-semantics` grep
  hit is a same-named method on a different tree).
- `paint.rs`'s `graft` (`pipeline/owner/paint.rs`, the `RetainedSubtree`
  replay) **already propagates** `element_id` when it re-inserts a captured
  node. So the carry path through retention is built and tested; what is
  missing is only an **origin** that puts a non-`None` value there in the
  first place.

That narrows slice 1 considerably: this is one stamp at one function, not a
plumbing project.

## Where the origin goes

`PaintComposer::push_layer` (`pipeline/owner/paint.rs:634`) is the single
funnel — five call sites reach it (opacity, transform ×2, a scope layer, and an
offset layer for a moved boundary). A boundary's identity is available on the
paint walk that calls them.

## The fork slice 1 has to decide first

`LayerNode::element_id` is an `ElementId`. The paint path works in render-tree
terms and has a `RenderId`. ADR-0061 explicitly allows "it, or an equivalent
`RenderId`".

- **`RenderId` (expected answer).** Already in hand at every `push_layer` call
  site; no lookup; and the thing whose repaint boundary produced the layer IS a
  render object. The field would be renamed or joined by a sibling.
- **`ElementId`.** Matches the existing field, but needs a render→element
  lookup on a hot path for an identity the paint phase does not otherwise use.

Decide this before writing anything; it changes the field, the accessor, and
every test's fixture.

## Acceptance for slice 1 (observable)

1. Two consecutive frames over an unchanged tree produce layer trees whose
   boundary layers pair one-to-one by stamp.
2. A boundary that repaints keeps its stamp; a boundary that is destroyed and
   recreated does not reuse it.
3. A grafted (retained) subtree carries the stamps its capture had — this path
   already exists and must stay green, so it is a regression pin, not new work.
4. The stamp is absent (`None`) on layers no boundary originated, so a future
   pairing pass cannot mistake a structural layer for a boundary.

Each test must fail with the stamp removed. Note the trap the rest of this
series kept hitting: a fixture where a wrong implementation happens to agree —
here, a single-boundary tree pairs correctly under *any* stamping rule, so the
fixture needs at least two boundaries and a reorder.

## Explicitly NOT in slice 1

The comparison pass, `DamageRegion`'s non-`Full` variants, the
composited-layer-update dirty queue, `markNeedsCompositedLayerUpdate`
equivalence, the GPU readback proof, and the benchmark. #536's acceptance list
covers all of those; none is reachable until layers can be paired, and each is
its own slice.
