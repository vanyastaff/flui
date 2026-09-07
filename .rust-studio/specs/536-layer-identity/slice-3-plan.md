# 536 slice 3 — update a boundary's own layer without repainting its children

Slice 1 (#967) gave a boundary's root layer a `RenderId` that survives the frame
boundary. Slice 2 (damage from a tree diff) was withdrawn as measured-inert.
This slice builds the OTHER consumer of retained identity, and the one the
issue is actually named after: an **update-only commit** — Flutter's
`markNeedsCompositedLayerUpdate` / `PaintingContext.updateLayerProperties`.

Its payoff is CPU paint, not the GPU scissor, so nothing here depends on the
withdrawn damage work.

Verified against the tree and against `.flutter` at tag `3.44.0` on 2026-09-07.

---

## What already exists

Retention landed in #755 and **works**. `PipelineOwner::retained_boundaries`
maps `RenderId -> RetainedSubtree`; the paint walk grafts a clean boundary
instead of repainting it (`pipeline/owner/paint.rs`, the `child_is_boundary`
arm). The pieces the issue asks for that are already here:

| Issue's ask | Where it already is |
|---|---|
| owner-scoped retained handle | `PipelineOwner::retained_boundaries` |
| generation validation | `RenderId` is generational — a recycled slab slot misses the map (`accessors.rs`, `remove_subtree`) |
| invalidate on detach | `remove_subtree` evicts; `run_paint`'s residue scan evicts unreached boundaries |
| eligibility predicate | `Scheduler::mark_needs_paint`'s `owns_retained_layer` = `is_repaint_boundary_flag() && was_repaint_boundary()` — Flutter's exact `isRepaintBoundary && _wasRepaintBoundary` |

**`run_paint`'s doc comment is stale** and says the opposite ("cross-frame
retention … is deliberately out of scope"). It predates #755, and this slice
corrects it.

## The one structural fact this slice turns on

A boundary's own `OffsetLayer` is pushed by its **parent** and is **outside**
the capture: `capture(boundary_root)` flattens `tree.children(root)`, and
`graft` re-inserts them under a freshly pushed boundary layer. That is why a
boundary that merely MOVED is already free today — the parent re-pushes the
layer with the new offset and grafts unchanged children.

It is exactly Flutter's `_compositeChild`:

```dart
if (child._needsPaint || !child._wasRepaintBoundary) { repaintCompositedChild(child, ...); }
else { if (child._needsCompositedLayerUpdate) { updateLayerProperties(child); } }
childOffsetLayer.offset = offset;   // the parent always refreshes the offset
```

The layers that are NOT free are the boundary's own **effect** layers — the
`OpacityLayer` `paint_subtree_impl` pushes from `render_node.paint_alpha()`
before replaying the fragment. Those sit INSIDE the capture, so a change to
alpha today can only be expressed by repainting the whole subtree.

**This slice makes those leading effect layers refreshable at graft time.**

## Scope correction the issue needs

The issue says "Start with opacity, transform, clip, and physical model."
Against the reference that is wrong, and following it would be a regression.

`grep -rn "updateCompositedLayer" .flutter/packages/flutter/lib` returns
**three** overriders and nothing else:

| Overrider | Boundary predicate |
|---|---|
| `proxy_box.dart:RenderOpacity` | `isRepaintBoundary => alwaysNeedsCompositing` = `child != null && _alpha > 0` |
| `proxy_box.dart:RenderAnimatedOpacityMixin` | `isRepaintBoundary => child != null && _currentlyIsRepaintBoundary` |
| `widgets/image_filter.dart:_ImageFilterRenderObject` | `isRepaintBoundary => alwaysNeedsCompositing` |

`RenderTransform`, the whole `_RenderCustomClip` family (`RenderClipRect`,
`RenderClipRRect`, `RenderClipOval`, `RenderClipPath`), and
`RenderPhysicalModel`/`RenderPhysicalShape` **do not override it and are not
repaint boundaries**. Every one of their setters calls plain `markNeedsPaint`.
They reuse layers through a different mechanism entirely — `oldLayer:` passed
to `context.pushTransform`/`pushClipRect`, which FLUI does not have.

Putting them on this path means making each a repaint boundary, i.e. one
offscreen per clip node — precisely the cost `RepaintBoundary` exists to make
explicit. **So this slice covers opacity, and the issue's scope line is
corrected rather than followed.** Transform/clip/physical-model would need
per-object layer reuse (`oldLayer`), which is a different and larger design.

`_ImageFilterRenderObject` has no FLUI counterpart that is a repaint boundary
today; deferred with the same mechanism available to it.

## Design

### 1. Flag and queue — one queue, a discriminating flag (Flutter's shape)

Flutter uses ONE list (`_nodesNeedingPaint`) plus `_needsCompositedLayerUpdate`.
FLUI matches that: no second queue.

- `RenderFlags::NEEDS_COMPOSITED_LAYER_UPDATE = 1 << 13` (6 and 13 are free),
  with `needs_composited_layer_update()` / `mark_composited_layer_update_flag()`
  / `clear_needs_composited_layer_update()` alongside the paint equivalents.
- `Scheduler::mark_needs_composited_layer_update(tree, id)`:
  - return early if `needs_paint() || needs_composited_layer_update()`
    (Flutter: `if (_needsCompositedLayerUpdate || _needsPaint) return;` — **paint
    wins**);
  - if `is_repaint_boundary_flag() && was_repaint_boundary()` — the SAME
    `owns_retained_layer` predicate `mark_needs_paint` already computes — set
    the flag and `schedule_paint_boundary(id, depth)`;
  - otherwise **delegate to `mark_needs_paint(tree, id)`** (Flutter's documented
    "equivalent to calling markNeedsPaint"), leaving the flag set as Flutter
    does.
- `DirtyKind::CompositedLayerUpdate` (the enum is `pub(super)`, so no public
  break) + `RenderInvalidationHandle::mark_needs_composited_layer_update()`,
  routed through the same mid-phase gate as the other marks so a mark from
  inside paint cannot re-enter the frame (Flutter asserts
  `!owner.debugDoingPaint`; port-check trigger #22 is the same rule).
- `PipelineOwner::mark_needs_composited_layer_update(id)` public accessor.

### 1b. The synchronous setter path — `RenderUpdateImpact::COMPOSITED_LAYER_UPDATE`

The handle verb above covers the cross-thread ticking case. It does **not**
cover the widget-driven one, and Flutter's own canonical example is the
widget-driven one:

```dart
// proxy_box.dart:RenderOpacity.opacity setter
if (didNeedCompositing != alwaysNeedsCompositing) { markNeedsCompositingBitsUpdate(); }
markNeedsCompositedLayerUpdate();          // ← not markNeedsPaint
```

FLUI's `RenderOpacity::set_opacity` (`crates/flui-objects/src/proxy/opacity.rs`)
returns `RenderUpdateImpact::PAINT` unconditionally on any value change, and it
is live production wiring: `crates/flui-widgets/src/paint/opacity.rs`'s
`update_render_object` calls it on every rebuild that changes opacity. Shipping
only the handle side would make `RenderOpacity` pay the new repaint-boundary
cost while its own primary mutation path never takes the cheap arm — a seam
that ships correct and unwired, which is this repository's dominant defect
class, and one this slice's ticking-path acceptance test would not see.

So both seams are wired (ADR-0046 is the designated home for the synchronous
half):

- `RenderUpdateImpact::COMPOSITED_LAYER_UPDATE` — a new bit on the existing
  `struct RenderUpdateImpact(u8)` (`crates/flui-rendering/src/update.rs`).
  Additive: named constants on a newtype, no exhaustiveness break.
  **It must NOT imply `PAINT_BIT`**, unlike `COMPOSITING_BITS`, which is
  defined as `COMPOSITING_BITS_BIT | PAINT_BIT`.
- `apply_render_update_impact` (`pipeline/owner/accessors.rs`) gains the arm
  that calls `mark_needs_composited_layer_update`. **Paint is applied first**,
  so that when a setter reports both (a threshold crossing sets
  `COMPOSITING_BITS`, which implies `PAINT`), the layer-update mark hits its
  own `needs_paint()` early-return and degrades to the repaint — Flutter's
  "paint wins" rule, reached through the existing bit algebra rather than a
  second precedence rule. Pinned by a test.
- `RenderOpacity::set_opacity` emits `COMPOSITED_LAYER_UPDATE` in place of the
  unconditional `PAINT` for a pure value change, and keeps emitting
  `COMPOSITING_BITS` (⇒ `PAINT`) when `needs_compositing()` flips. That is
  Flutter's setter, bit for bit — and the structure-change fallback falls out
  of the existing algebra rather than needing new logic.

### 2. One constructor for a node's own effect layers

`paint_subtree_impl` builds the alpha/transform effect layers inline today.
Extract that to **one** function used by both the paint walk and the update
arm, so the two can never drift:

```rust
fn own_effect_layers(node: &RenderNode, origin: Offset) -> SmallVec<[Layer; 2]>
```

`smallvec` is already a workspace dependency and already used by this crate
(`crates/flui-rendering/Cargo.toml`); `arrayvec` is not a workspace dependency
at all, and this workspace has a standing preference for `SmallVec` over a
fixed-capacity vector type (`flui-foundation/src/notifier_generic.rs`). The
ordering of the two slots is the semantically load-bearing part, so an ordered
list beats a two-field struct here.

(A test that reimplements the predicate is not a pin — both callers must call
the same function.)

### 3. `RetainedSubtree::own_effect_layers: usize`

The count of leading flattened nodes that are the boundary's own effect
layers. They are the head of the pre-order DFS by construction: the parent
pushes `boundary_root`, `paint_subtree_impl` pushes the effect chain first,
then replays the fragment. `close_capture()` returns it alongside
`nested_boundaries`.

### 4. The update arm

At the `child_is_boundary` decision:

```
needs_repaint       = repaint_set.contains(child_id)
needs_layer_update  = layer_update_set.contains(child_id)
```

`repaint_set` = queued nodes whose `needs_paint()` is set; `layer_update_set` =
queued nodes with only `needs_composited_layer_update()`. This is exactly the
`if (node._needsPaint) repaintCompositedChild else updateLayerProperties` split
in `flushPaint`.

- Graft eligibility stays `!needs_repaint` **and** no nested boundary in
  `repaint_set ∪ layer_update_set`. A nested boundary needing an update inside
  a reused outer capture cannot be refreshed (the capture flattens it), so the
  outer declines and repaints — conservative, and the inner then takes its own
  update arm one level down. Same bounded answer `nested_boundaries` already
  gives for the repaint case.
- On graft with `needs_layer_update`: rebuild the leading
  `own_effect_layers` nodes from `own_effect_layers(node, Offset::ZERO)` and
  graft the rest unchanged.
- **Structure guard:** if the rebuilt count differs from
  `subtree.own_effect_layers` (alpha crossed the layered/unlayered threshold,
  a transform appeared), refuse the update and repaint in full. This is the
  issue's "paint remains the fallback when structure changes", and it is the
  FLUI analogue of Flutter's `assert(identical(updatedLayer, childLayer))` plus
  `OpacityLayer.alpha`'s engine-layer invalidation at 255.
- **Offset is never touched by the update** — structurally impossible here,
  because the boundary's offset lives on the layer the PARENT pushes, outside
  the capture. Flutter needs `assert(debugOldOffset == updatedLayer.offset)` to
  enforce what this shape makes unrepresentable.

`clear_needs_composited_layer_update()` goes next to `clear_needs_paint()` in
`paint_subtree_impl` (before the `skip_paint`/`needs_layout` early returns —
Flutter clears it in `_repaintCompositedChild` for exactly that reason), and
the update arm clears it too since that arm never enters `paint_subtree_impl`.
`run_paint`'s residue scan gains the same flag, with the same capture eviction.

### 5. Opacity becomes a repaint boundary

Reachability requires it, and it is a straight port of a contract FLUI dropped:

- `RenderAnimatedOpacity::is_repaint_boundary()` = `is_layered(alpha)` —
  matching its existing `always_needs_compositing()`. Flutter:
  `isRepaintBoundary => child != null && _currentlyIsRepaintBoundary`. FLUI's
  `is_layered` (`0 < alpha < 255`) rather than Flutter's `alpha > 0` is the
  divergence this crate already recorded for `always_needs_compositing` and
  `paint_alpha`; keeping one predicate for all three is the point.
- `RenderOpacity` and `RenderSliverAnimatedOpacity` likewise. (`RenderOpacity`
  having no `always_needs_compositing` override at all is named in
  `animated_opacity.rs` as a pre-existing gap; it is closed here because the
  update path needs it.) Flutter leaves the *static* `RenderSliverOpacity` off
  this path, and so does this slice.
- `RenderAnimatedOpacity::recompute_alpha` swaps `mark_needs_paint()` for
  `mark_needs_composited_layer_update()`, keeping the existing
  send-then-commit ordering and the compositing-bits mark that precedes it.

### 6. Observability

`run_paint`'s span gains `layer_updates = N` beside `dirty_nodes`, and the
update arm emits a `trace!` naming the boundary. That is the issue's "frame
traces distinguish paint from layer-only updates".

## Acceptance (observable, each red without the change)

The outer acceptance test, written first:

1. **A ticking animated opacity updates alpha without repainting the child
   subtree.** Two frames over a tree whose leaf counts its own paints through
   an `Arc<AtomicUsize>` (`RenderRepaintBoundary::paint_count` is a dead
   diagnostic — nothing increments it). Tick the animation between frames.
   Assert the leaf's paint count is unchanged AND the emitted `OpacityLayer`'s
   alpha changed. Both halves are required: the first alone passes if nothing
   painted at all, the second alone passes on a full repaint.

Then:

2. **The widget-driven setter takes the same arm.** Rebuilding an `Opacity`
   widget with a new value updates the layer without repainting the child
   subtree — the `RenderUpdateImpact` path, asserted separately from the
   ticking path, because the two reach the mark through different seams.
3. **Eligibility degrades to paint.** Marking a non-boundary, or a boundary
   that has not painted yet (`!was_repaint_boundary`), behaves exactly as
   `mark_needs_paint` — the nearest boundary ancestor repaints.
4. **Paint wins.** `mark_needs_paint` then `mark_needs_composited_layer_update`
   on one node in one frame ⇒ a full repaint, and the reverse order too.
5. **Structure change falls back to paint.** Alpha crossing into or out of the
   layered range repaints rather than updating.
6. **A nested boundary needing an update refuses the outer graft**, and gets
   its own update one level down. (A flat fixture cannot see this — slice 1's
   review already caught that once.)
7. **Stale handles are rejected.** Removing the subtree evicts the capture;
   the residue scan clears the new flag and evicts alongside it; a detached and
   reattached boundary repaints rather than serving a stale capture.
8. **GPU readback pixel equivalence** — the same tree rendered via a layer-only
   update and via a forced full repaint produce the same pixels.
9. **Benchmark**: `paint/one_layer_update_boundary` beside the existing
   `paint/one_dirty_boundary` on the same tree, at N = 10/100/1000. The noise
   floor on this machine is ±5%, so only a large ratio counts as evidence.
10. `just ci` passes.

Fixture rules carried from slices 1–2: at least two boundaries and a nested
one, or a wrong implementation passes; and every test verified red by
reverting the production line it exists for.

## Divergence to record (Prime Directive rule 1)

Flutter makes each of its three overriders hand-write a near-duplicate
`updateCompositedLayer`. FLUI adds **no trait method**: the fresh layers are
derived from the pre-existing `paint_alpha()` / `paint_transform()` node hooks,
so the update-only path covers exactly the effect surface the full-paint path
already covers, and every current and future implementer of either hook gets it
for free. This is an improvement over the reference and owes a
`## Mapping decisions` entry in `crates/flui-rendering/ARCHITECTURE.md` naming
its one limitation: an effect expressed through some OTHER mechanism (a custom
layer pushed from `paint_raw`) has no update-only path — unchanged from today,
so no regression, but stated rather than assumed.

## Explicitly not in this slice

Transform / clip / physical-model layer updates (they need per-object
`oldLayer` reuse, not this mechanism — see the scope correction above), the
image-filter counterpart, per-object layer reuse for non-boundaries, and
anything to do with damage or the GPU scissor.
