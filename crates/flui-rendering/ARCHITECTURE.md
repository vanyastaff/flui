# flui-rendering Architecture

This document is the per-crate template instance for `flui-rendering` as defined by [`docs/PORT.md`](../../docs/PORT.md). It records the Flutter → Rust mapping for this crate, the divergence decisions taken so far, the current thread-safety surface, the known friction not yet refactored, and the planned cleanups that the methodology will pick up next.

The deeper architectural write-ups for individual subsystems (protocol, layout, paint, hit-test) live alongside this file under [`docs/`](docs/) and migration plans under [`migration/`](migration/). The Flutter class hierarchy walk lives in [`flutter-rendering-hierarchy.md`](flutter-rendering-hierarchy.md) as a sibling appendix and is referenced from `## Flutter source mapping` below.

---

## Flutter source mapping

| Flutter source | FLUI module | Notes |
|---|---|---|
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart` | [`src/storage/entry.rs`](src/storage/entry.rs), [`src/storage/state.rs`](src/storage/state.rs), [`src/storage/flags.rs`](src/storage/flags.rs), [`src/traits/render_object.rs`](src/traits/render_object.rs) | The `RenderObject` base class is split: trait surface in `traits/render_object.rs`, owned storage in `storage/entry.rs`, mutable per-frame state in `storage/state.rs`, atomic flags in `storage/flags.rs`. The Flutter `AbstractNode` parent-linkage role is in [`src/storage/links.rs`](src/storage/links.rs). |
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart` `PipelineOwner` (line 1019+) | [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs) | Single-threaded phase serialisation. Flutter's `flushLayout` / `flushCompositingBits` / `flushPaint` / `flushSemantics` map to FLUI's `run_layout` / `run_compositing` / `run_paint` / `run_semantics`, each living on the matching `PipelineOwner<Phase>` impl block (typestate-enforced ordering, Mythos Step 7). Holds the root node and dirty lists. The `debug_doing_layout` / `debug_doing_paint` flags on the owner are the FLUI runtime analog of Flutter's `_debugActiveLayout` / `_debugDoingThisPaint` static asserts (kept as a debug-build cross-check; the type system is the load-bearing enforcement). |
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/box.dart` | [`src/protocol/box_protocol.rs`](src/protocol/box_protocol.rs), [`src/parent_data/box_parent_data.rs`](src/parent_data/box_parent_data.rs) | `BoxConstraints`, `BoxParentData`, `Size`-based geometry. |
| `.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver.dart` | [`src/protocol/sliver_protocol.rs`](src/protocol/sliver_protocol.rs), [`src/parent_data/sliver_parent_data.rs`](src/parent_data/sliver_parent_data.rs) | Sliver protocol for scrollable layout. |
| `RenderObjectWithChildMixin`, `ContainerRenderObjectMixin` (`object.dart` lines 4160-4400+) | [`src/storage/links.rs`](src/storage/links.rs), [`src/parent_data/container_mixin.rs`](src/parent_data/container_mixin.rs) | Single-child + variable-children storage. Flutter uses Dart linked lists; FLUI stores `Vec<RenderId>` on the parent. |
| `proxy_box.dart`, `shifted_box.dart`, `flex.dart` | [`flui-objects`](../flui-objects/src/) crate | Concrete render objects (`Padding`, `Center`, `ColoredBox`, `Flex`, `Opacity`, `SizedBox`, `Transform`, …), extracted to the sibling `flui-objects` crate; this crate keeps only the protocol/pipeline machinery. |
| Layer-related (`layer.dart`, container layers) | `flui-layer` crate | Compositing layers live in a sibling crate per the layered DAG ([`docs/architecture.md`](../../docs/architecture.md)). |

The full Flutter class hierarchy is enumerated in the sibling appendix [`flutter-rendering-hierarchy.md`](flutter-rendering-hierarchy.md) (1352 LOC, generated from a class-name sweep of `.flutter/flutter-master/packages/flutter/lib/src/rendering/`). That file is kept as a search index; it is not part of the template proper.

Render-subtree relocation is deliberately narrower than Flutter's ambient
owner mutation: only `PipelineOwner<Idle>` can detach, attach, or release a
batch. Detach returns an opaque, non-cloneable `DetachedRenderSubtrees` token
bound to the originating owner by private `Rc` identity. Reattach and
finalization release consume that token, returning it inside a typed failure
after mutation-free preflight when owner, epoch, live-node, or topology checks
fail. Finalization release does not delete nodes; it authorizes the existing
deepest-first element unmount so view lifecycle hooks remain canonical.

---

## Mapping decisions

This section records places where the Rust shape diverges from the Dart shape and why. Each entry follows the "Accepted trade-offs" format established by [`docs/plans/2026-03-31-custom-render-callback-design.md`](../../docs/plans/2026-03-31-custom-render-callback-design.md): state the rule (or absence of rule), the choice, the alternatives considered, the trade-off accepted.

### The set POSITION is published; the set size waits, and the delegates do not wrap

**Rule:** a screen reader announces "item 12 of 100" from the platform's set-position concept.
Flutter carries the two halves separately — `SemanticsConfiguration.indexInParent` on the item and
`scrollChildCount` on the scrollable — and leaves each platform bridge to reconcile them; its lazy
delegates supply the first by wrapping every materialised item in an `IndexedSemantics`
(`addSemanticIndexes`, on by default), a `SingleChildRenderObjectWidget`.

**Choice:** AccessKit has the concept directly, so `accesskit_translation` can emit the pair —
`position_in_set` (one-based, converted there from the framework's zero-based index) and
`size_of_set` — and a negative index is dropped rather than published as a nonsensical position.
`IndexedSemantics` and `RenderIndexedSemantics` exist as public widgets; FLUI's delegates do
**not** wrap items in them by default.

**Both halves are published, and both land on the ITEM node.** A first attempt put `size_of_set`
on the enclosing sliver, from its `item_count`. That is wrong twice over: AccessKit's two
properties describe the SAME node, so a total on the container is invisible to a reader querying
the focused row; and a lazy sliver's count is its *render-child* count, which for a delegate that
interleaves non-members is not the number of items a caller indexes, and would announce "item 2 of
5" on a three-item list.

The size therefore rides with the position, minted together by the host from the delegate's
DECLARED item count — the count of members, not of render children — and stamped into the same
parent data. `None` under an unresolved `ItemCount::Unknown`, where the count is only known after a
probe walk and charging every mount for one is not worth it: that degrades to "item 12 of ?", and a
missing total is honest where a wrong one misleads. A caller who wants the total declares
`ItemCount::Exact`, which that variant's own doc already recommends for unrelated reasons.

**Placement differs from the reference.** An `IndexedSemantics` goes *inside* the row's semantics
container here, not outside it: a non-boundary configuration is absorbed by its nearest ANCESTOR
boundary in this assembler, so a wrapper above the container would index the node that forms
above the row — the sliver — instead of the row. Flutter's delegates wrap outside because its
merge runs the other way.

**Now done: the position is derived, not wrapped.** Each item's position comes from the index the
sliver already stamps in `SliverMultiBoxAdaptorParentData` — zero extra nodes, and an index the
band walk keeps in step with the row's real position rather than one captured at build time. (The
wrapper cost was measured before this landed: a three-item `ListView` went from 8 render nodes to
11.)

What blocked it was that a derivation from the *logical* index alone announces non-members as
members — `ListView.separated` interleaves separators at odd logical indices, so the real items
would get positions 1, 3, 5. The fix is a semantic index carried BESIDE the logical one:
`SliverSlot { logical, semantic }` is minted by the host, inherited through however many component
elements sit between it and the child's first render descendant, and stamped together at both
stamp sites. `semantic: None` marks a child that occupies a logical index without being a member.
The two halves are one value precisely because the failure being prevented is their drifting
apart.

`IndexedSemantics` and `RenderIndexedSemantics` remain public and **take precedence**: the stamped
index is applied only where nothing declared one, and only *after* the fragment fold, since an
`IndexedSemantics` inside the row is a non-boundary configuration absorbed by the row during that
fold. Applying it earlier makes the stamp win — `absorb` does not overwrite a value the parent
already holds — which would make hand-indexed content inside a lazy list impossible.

FLUI has no `separated` constructor yet; the threading landed first so that constructor's author
inherits a hook they must answer rather than a 1:1 assumption they must discover. The delegate-
supplied rule (`semantic_index_callback`, `semantic_index_offset`) is still open on #837.

**Replacement tests**, all in `crates/flui-widgets/tests/semantics.rs` and all asserting on the
published AccessKit nodes rather than the framework configuration — the whole chain existed in
pieces before this and connected to nothing, so the near end proves nothing about what a reader
receives:

- `an_indexed_item_publishes_its_position_in_the_set` — the explicit `IndexedSemantics` path, and
  the set size beside it. It asserts the size lands on the ITEM nodes and only on them, so a total
  drifting onto the container — the first attempt's mistake, invisible to a reader querying a row —
  fails rather than passing as "a size is published somewhere".
- `a_lazy_child_publishes_its_position_without_an_indexed_semantics_wrapper` — the derived path,
  on rows carrying no wrapper at all.
- `an_explicit_index_overrides_the_one_the_sliver_stamped` — precedence. Its declared indices are
  the REVERSE of the stamped ones, so it fails whichever way the precedence is wrong; with them
  equal it would pass against both orders.

Plus
`harness_indexed_semantics_reports_its_index_and_only_republishes_on_change`
(`crates/flui-objects/tests/render_object_harness.rs`), including that an unchanged index requests
no semantics update.
### A layout stamps the children it laid out; paint and hit-test skip the rest

**Rule:** a multi-child render object that lays out a *subset* of its children — a lazy sliver's
band, anything virtualised — leaves the rest holding an offset and a size from an earlier pass.
Flutter's slivers keep the discipline per object: `RenderSliverMultiBoxAdaptor` tracks its own
child list and paints only what it laid out.

**Choice:** the pipeline enforces it instead. A layout advances its own `layout_generation` and
stamps it onto every child it laid out (`placed_generation`); the paint driver and the hit-test
walk skip any child carrying a different value. Every multi-child object gets the property
whether or not it remembered to ask for one.

**Why it cannot live in the render object:** `PaintCx` exposes neither parent data nor a child's
logical index, so an object cannot tell paint which of its children this pass placed. That is why
the now-deleted eager grid carried a `laid_out_band` field of its own, and why the lazy slivers
used to rely on the frame evicting out-of-band residents before paint runs (ADR-0017's amendment)
— each a per-object workaround for a property the pipeline can hold once.

That eviction is no longer a backstop for every out-of-band child. Keep-alive (ADR-0056) makes a
held child stay attached and unlaid indefinitely, so this stamp is the **sole** mechanism keeping
it off screen, out of hit-test, and out of the semantics tree — defence-in-depth promoted to
load-bearing. That promotion is why the semantics gate had to land before keep-alive could.

**Three constraints the implementation found, each of which broke a real test:**

1. The stamp keys on `layout_child`, **not** `position_child`. Twelve single-child proxies
   (`Opacity`, `RepaintBoundary`, `Transform`, …) lay their child out and never position it,
   because the offset is implicitly zero; gating on positioning drops every one from paint.
2. The generation advances at the layout **commit**, not at layout entry. Advancing at entry lets
   any early return in between — a protocol error, a poisoned descendant — move the parent forward
   while stamping nobody, which unplaces every child and takes the subtree off screen.
3. The stamp carries the **parent's identity** as well as its counter. The counter is per-parent,
   so every parent that has laid out N times has issued the number N, and a `GlobalKey` relocation
   makes the collision reachable: a child stamped `2` by parent A, reparented onto a B that has
   never laid out, would pass B's first real layout — which also reaches `2` — despite B having
   laid out nothing, and would then paint at A's offset.
4. `placed_generation` starts equal to a parent's initial `layout_generation`, so a child whose
   parent has not laid anything out **yet** counts as placed. The gate may only remove a child a
   parent demonstrably *stopped* laying out; absent evidence it paints. Without this, any parent
   that never lays out its children — served from cache, or laying out by a path of its own —
   hides its whole subtree.

Marked paths: box `layout_child`/`position_child` (typed and erased), sliver the same, and both
cross-protocol methods (`layout_sliver_child` on the box side, `layout_box_child` on the sliver
side). The last two were found by test failures, not by reading.

**All three walks are gated.** Semantics was deferred at first, because
`harness_merge_semantics_collapses_descendant_boundaries` lost a descendant's `is_button` under
the gate — a merge folds a descendant's configuration regardless of geometry, so excluding an
unplaced child loses its label and role as well as its position.

That objection dissolved once the stamp carried the issuing parent's identity (constraint 3
above). A child its parent has **never** laid out reads as placed, because `placed_by` is still
`0`; only a child a parent once laid out and then *stopped* laying out is excluded. The merge
harness has the first kind — a `Single`-arity object mounted with two children — and is untouched.

And for the case the gate exists for, a lazy sliver's out-of-band resident, exclusion is not a
worse trade but the reference's own behaviour: Flutter removes an off-screen or kept-alive child
from the render child list, so it publishes no semantics at all. Announcing one at a rect from a
pass that no longer holds sends a screen reader somewhere with nothing on it.

**What the stamp cannot express, recorded rather than left to be rediscovered.** Because an
unstamped child reads as placed, exclusion is history-dependent: a child laid out and *then*
skipped is dropped, while one skipped from its very first pass was never stamped and is still
announced. Two identical final trees can therefore expose different accessibility trees. The
asymmetry is not removable *at the stamp* — requiring one instead of accepting its absence
regresses `harness_merge_semantics_collapses_descendant_boundaries` (verified: the descendant's
`is_button` is lost), which is the same objection that deferred this gate in the first place.

It is removable one layer up, and now is: `RenderBox::visits_child_for_semantics(child_slot)` lets
a parent drop a child it structurally does not present, independently of layout history. The stamp
answers "did this pass place you"; the visitor answers "do I present you at all". `RenderTheater`
was the reachable case and overrides it from `skip_count` — see
`harness_theater_offstage_from_the_first_pass_publishes_no_semantics`, which is red without it.

**A skip must drop the cached output too.** `run_paint`'s residue scan clears the dirty flag of
any node the descent did not reach — it has always done that, with a warning, for multi-root and
detached subtrees — and this gate makes a third case reach it: a child that received a paint-only
update while unplaced. Clearing the flag while keeping the node's retained capture is what makes
that lossy, because a child placed again under unchanged constraints short-circuits its own layout
and requeues nothing, so the stale capture is grafted and the update is gone for good. The scan
now evicts the capture alongside the flag, which also closes the pre-existing detached-subtree
case it had only been warning about.

**Replacement tests:** `a_child_dropped_from_a_later_layout_pass_stops_painting`,
`…_stops_being_hit`, and `a_skipped_boundary_repaints_rather_than_grafting_a_stale_capture`
(`crates/flui-rendering/tests/placed_generation_gate.rs`), each verified red with its own change
reverted. Both need **two frames**: a child never laid out at all has size zero
and paints nothing regardless, so a single-frame version passes with the gate removed — which the
first draft did. `FrameRun::run_frame_again` is added for it.

### A composited-layer update patches the enclosing capture; no node is promoted to a boundary

**Rule:** Prime Directive rule 1 (behavior is the floor, design is ours) — this
is a deliberate improvement over the reference and owes its accounting here.

**Choice:** Flutter serves `markNeedsCompositedLayerUpdate` by giving the render
object its own layer to mutate in place, which requires the object to BE a
repaint boundary (`proxy_box.dart`: `RenderOpacity.isRepaintBoundary =>
alwaysNeedsCompositing`) and requires `LayerHandle` ref-counting plus
`Layer.dispose` to manage that layer's lifetime.

FLUI does neither. `RetainedSubtree` is a flat `Vec<RetainedNode>`, so a node's
own effect layers are addressable by index inside the ENCLOSING boundary's
capture (`effect_slots`). An alpha change rebuilds those entries through the
same `own_effect_layers` constructor the paint walk uses, patches them into the
grafted output, and writes them back into the stored capture — with the node
still an ordinary non-boundary.

**Alternatives:** port Flutter's promotion — rejected on two grounds, both
measured or verified rather than argued. (a) It buys nothing: the enclosing
boundary's capture is already what gets grafted, so promotion adds an
`OffsetLayer` and a whole retained capture per node to reach a position already
reached; and since a graft is O(retained layers), promoting nodes is what makes
reuse *slower* (`paint/opacity_alpha_change`: 228x on inline content, 1.71x once
every leaf is its own boundary — measured 2026-09-11 alongside the other four
`paint_effects` producers; see "the clip family" below for the full
cross-producer set and the command). (b) It needs `is_repaint_boundary()` to vary at
runtime, which the storage flag does not support — see the `IS_REPAINT_BOUNDARY`
note below.

**Accepted trade-off:** three of them, all narrowing WHEN the fast path applies,
never whether the frame is correct.

1. **An effect a render object still records as a fragment scope from
   `paint` has no update-only path.** `RenderClip<S>`
   (`crates/flui-objects/src/proxy/clip.rs` — `paint_effects`/
   `clip_descriptor`) and `RenderFlow` (`crates/flui-objects/src/layout/flow.rs`
   — `paint_effects`) used to be in this bucket and now report their clip
   through the value instead, so this trade-off has narrowed to whatever a
   producer still pushes from `paint`. Rather than a list this entry would
   have to keep in sync by hand every time a producer moves, enumerate the
   producers directly: `rg -n
   "with_clip_rect\(|with_clip_rrect\(|with_clip_path\(|with_shader_mask\(|with_backdrop_filter\("
   crates/flui-objects/src`. Today that returns the overflow-gated clip
   family (`RenderStack`, `RenderIndexedStack`, `RenderWrap`,
   `RenderViewport`, `RenderShrinkWrappingViewport`, `RenderFittedBox`,
   `RenderConstraintsTransformBox`, `RenderAnimatedSize` — each gated on
   `has_visual_overflow && clip_behavior != Clip::None`, clipping to the
   node's own size), `RenderPhysicalModel`/`RenderPhysicalShape`'s own clip
   around their shadow-and-fill draws, and the `RenderBackdropFilter`/
   `RenderShaderMask` scopes — the two hits that are not clips at all —
   pushed because `PaintEffects` has no field for either effect yet. Re-run
   the command before trusting this sentence; "Deferred producers" below
   records why each is out for now.
2. **A boundary declines to graft while any boundary nested inside it has
   pending work of any kind, including a layer update.** Serving a nested
   boundary's update from an enclosing capture is possible — the layers are
   flattened into it — but it patches only that capture and clears the flag,
   leaving the nested boundary's own capture at the old value for the next
   frame that grafts it to replay. Declining costs the outer boundary's reuse
   on those frames and keeps both captures consistent, because the outer
   re-captures the patched result on its way back out.
   (`an_update_under_nested_boundaries_does_not_leave_the_inner_capture_stale`
   is red without this, restoring the old alpha on the third frame.)
3. **The root boundary is never retained, so an effect whose only enclosing
   boundary is the root gets no fast path.** `run_paint` enters through
   `paint_subtree(root)` directly rather than the boundary-child arm that
   creates captures, so `RenderView` — which declares itself a boundary — has
   none. The mark still degrades correctly (the frame repaints), it is simply
   not accelerated. A tree with any `RepaintBoundary` above the effect, which
   includes every per-item boundary in a list, is unaffected.

**Replacement tests:** `an_alpha_change_updates_the_layer_without_repainting_the_subtree`,
`a_layer_update_is_written_back_into_the_retained_capture` (three frames — a
two-frame version cannot see a missing write-back),
`a_structural_alpha_change_falls_back_to_a_repaint`,
`a_repaint_in_the_same_frame_wins_over_a_layer_update`
(`tests/retained_boundary_layers.rs`), plus pixel equivalence against a forced
repaint in the facade's `tests/composited_layer_update_readback.rs`.

**Where this diverges observably: the STATIC sliver opacity.** Upstream serves an
alpha change with `markNeedsCompositedLayerUpdate()` exactly where the node is a
repaint boundary, and it is one in two places: `RenderOpacity`
(`isRepaintBoundary => alwaysNeedsCompositing`) and `RenderAnimatedOpacityMixin`
(`isRepaintBoundary => child != null && _currentlyIsRepaintBoundary!`), which is
generic over `RenderObject` and so covers the animated case on BOTH protocols —
matching FLUI's `animated_opacity.rs` and `sliver_animated_opacity.rs`, which
already mark layer updates.

The one node it leaves out is the static `RenderSliverOpacity`: it declares
`alwaysNeedsCompositing` but never `isRepaintBoundary` (`isRepaintBoundary` does
not appear in `proxy_sliver.dart` at all), so the mechanism has no path to it and
its setter calls `markNeedsPaint()` — a full subtree repaint on every alpha tick.
Re-derive with `grep -rn "updateCompositedLayer\|isRepaintBoundary"
packages/flutter/lib/src/rendering/` inside `.flutter` at the pinned tag.

`RenderSliverOpacity::set_opacity` reports `COMPOSITED_LAYER_UPDATE` instead.
The promotion the reference needs is exactly what this design removed: a flat
capture makes any node's effect layers addressable inside the ENCLOSING
boundary, so the sliver needs no `isRepaintBoundary` of its own to be served.
The same argument that justifies not promoting the box case is what gives the
sliver case a path the reference does not have.

Behaviour is unchanged in every other respect, and the structural transitions
still repaint (`skip_paint` crossings and compositing-threshold crossings), so
the edge cases upstream handles by repainting unconditionally are handled here
by repainting deliberately. **Oracles:**
`a_sliver_alpha_change_updates_the_layer_without_repainting_the_subtree` and
`a_sliver_layer_update_is_written_back_into_the_retained_capture`
(`tests/retained_boundary_layers.rs`). Net-new, not replacements: upstream has no
test that drives `RenderSliverOpacity.opacity` as a setter, so no Flutter
coverage was dropped here.

They are the first coverage of the update-only path over the Sliver protocol at
all. What that buys is narrower than "the machinery is protocol-agnostic" and
worth stating exactly: `RenderNode::paint_effects()`
(`crates/flui-rendering/src/storage/node.rs`) resolves `size` per protocol
(box → committed `geometry().unwrap_or(Size::ZERO)`, sliver →
`absolute_paint_size()`) and calls the render object's `paint_effects(size)`
the same way in both arms, so every field of the value — opacity included —
now rides the one protocol split, not a dispatch specific to alpha. What
these two tests pin is unchanged: a sliver's `RenderSliver::is_repaint_boundary`
is honoured by the layer-update walk — without it the walk reaches the viewport
paint root and degrades to a plain repaint, which is exactly how both tests fail
when the setter is reverted. The one place the protocols genuinely diverge is the
`size` argument `paint_effects` is called with (`geometry()` for a box,
`absolute_paint_size()` for a sliver); `RenderSliverOpacity`'s `paint_effects`
never sets `transform`, so the transform arm of `own_effect_layers` stays
unexercised for slivers.

**Known gap, pre-existing and not introduced here:** upstream gates the
semantics mark on `alwaysIncludeSemantics`, a field neither FLUI opacity render
object has. Both report semantics unconditionally on a visibility flip.

**And again, further from the reference: `RenderTransform`.** Upstream has no
composited-layer-update path for a transform *at all* — verified at the pinned
tag in `proxy_box.dart`: `RenderTransform`'s `transform`, `origin` and
`alignment` setters each call `markNeedsPaint()` + `markNeedsSemanticsUpdate()`,
`isRepaintBoundary` is never overridden for the class, and its
`alwaysNeedsCompositing` is gated on `filterQuality`, not on the matrix. A
`ScaleTransition` or `RotationTransition` therefore repaints its subtree on
every animation frame upstream.

FLUI reports `COMPOSITED_LAYER_UPDATE` for a matrix change that stays within the
layered range, on the same flat-capture argument as the opacity cases: the
layer is addressable inside the enclosing boundary's capture, so nothing is
promoted. Measured on the benchmark's two shapes — **226x** at 1000 inline
nodes, **1.69x** once every leaf is its own boundary (2026-09-11; see "the
clip family" below for the full cross-producer set and the command).

Two things this costs, both deliberate:

1. **The matrix moved from a `FragmentOp::PushTransform` inside `paint` to the
   `transform` field of `paint_effects`** — the opposite of the choice `RenderFittedBox`
   documents for itself, which keeps its matrix in `paint` so it can open its
   clip *outside* the transform layer. `RenderTransform` has no clip, so it has
   no such ordering constraint, and only the `paint_effects` route is patchable. The
   asymmetry between the two is intentional; a future reader comparing them
   should not "fix" either toward the other.
2. **A pure translation keeps the no-layer fast path** (`paint_effects`
   reports no transform, `paint` applies a plain child offset), so translation ↔
   non-translation is a layer-count change and the setters report `PAINT`
   across it, as they do across singular ↔ non-singular. This is what keeps
   `Transform.translate` and every `SlideTransition` from paying for a
   compositing layer per frame — a cost upstream's unconditional
   `markNeedsPaint` never incurs either.

**Oracles** (net-new; upstream has no test driving these setters, so nothing was
replaced): `a_transform_change_updates_the_layer_without_repainting_the_subtree`
and `a_transform_layer_update_is_written_back_into_the_retained_capture` are the
red-green pair; `a_patched_transform_subtree_matches_a_full_repaint_at_any_size`
is the control that keeps the benchmark's flat update arm from also describing a
patch that dropped the subtree; and
`a_same_frame_layout_change_forces_the_repaint_a_transform_patch_relies_on`
pins the invariant the whole thing rests on — see the next entry.

**And `RenderRotatedBox`.** Upstream's `quarterTurns` setter is `if
(_quarterTurns == value) { return; } _quarterTurns = value;
markNeedsLayout();` (`rotated_box.dart`, 3.44.0 — read `set quarterTurns`
directly rather than grepping for `markNeeds` alone, which finds the call but
hides the equality guard in front of it). The guard compares the RAW value,
not a reduced form, and nothing narrower than that exists: two turns that are
merely unequal — including two that share parity, like `0` and `2` — cost a
full relayout and repaint upstream, exactly as `0` → `1` does. There is no
fast path at all, mod-4 or otherwise.

`RenderRotatedBox::set_quarter_turns` splits on how much of the turn actually
changes, not on whether it changed: an unchanged effective angle (mod 4,
`rem_euclid(4)`) reports `NONE` — the paint matrix and layout are
byte-identical, so nothing downstream needs to react, even though the raw
value is still stored. Same parity (even ↔ even or odd ↔ odd), different
quadrant, reports `COMPOSITED_LAYER_UPDATE | SEMANTICS` — the same route
`RenderTransform` already uses (`own_effect_layers` composes the node's
layers from its one `paint_effects` value — opacity, clip, transform, in
that fixed nesting order). A parity change
(odd ↔ even) reports `LAYOUT`, the pre-existing behaviour: it swaps which axis
the child sees, so the child must be re-laid-out under (un)flipped
constraints.

**Why the captured origin stays valid.** Two things, not one. First,
`perform_layout`, `compute_dry_layout`, all four intrinsic queries, and
`compute_dry_baseline` read `quarter_turns` ONLY through `is_vertical()` (its
parity) — never the exact value — pinned by a dedicated harness test rather
than the type system, since nothing in `quarter_turns: i32` stops a future
method from branching on the exact turn instead:
`harness_rotated_box_layout_is_turn_blind_up_to_parity`
(`flui-objects/tests/render_object_harness.rs`). A same-parity quadrant
change therefore never re-enters `perform_layout` for the rotated box itself
— the setter reports `COMPOSITED_LAYER_UPDATE`, not `LAYOUT`, so the pipeline
never calls it — which answers "did the rotated box's OWN layout move its
origin" trivially: no, it did not run. Second, "did something ELSE inside the
boundary move it" is the general invariant the next entry below states for
`RenderTransform` and pins with
`a_same_frame_layout_change_forces_the_repaint_a_transform_patch_relies_on`:
any same-frame layout change anywhere inside a boundary marks that boundary
needing paint, upgrading a queued `LayerUpdate` to a `Repaint`
(`PaintQueue::enqueue` never downgrades) — so a sibling or ancestor that moved
the rotated box's accumulated position forces a repaint, which recomputes the
conjugation from a live origin. `RenderNode::paint_effects` hands the value
the COMMITTED `geometry()` either way (`storage/node.rs`), and the matrix's
third input, `child_size`, is a field
`perform_layout` caches — the one input `RenderTransform` does not have — so
it cannot move without a relayout either. Neither size argument is in
question; only the origin is, and both halves of that are covered.

**Cost.** A patch is O(effect-owning nodes in the boundary) + O(captured
boundary size) — `layer_patches_for` walks every `effect_slots` entry in the
capture regardless of how many nodes actually asked for an update, and
`graft` allocates a map and a vec per call — against a full re-execution of
`paint()` over the boundary for a repaint. That is a constant-factor win at
the SAME order, not O(1); the benchmark below measures the factor, not the
order. The `| SEMANTICS` bit costs no more than the pre-existing `LAYOUT`
baseline already paid: `run_layout` marks semantics once per walk, on the
walk's dirty root (`pipeline/owner/layout.rs`), and the new route marks the
rotated box itself, which either IS that root (the setter's own caller
dirtied it) or sits under it.

**Two edge cases, both inert rather than unsafe.** A childless rotated box
that receives a same-parity-quadrant-change mark still reports
`COMPOSITED_LAYER_UPDATE`: `layer_patches_for` finds no slot for a target
with no effect layers of its own and refuses the patch, so the enclosing
boundary's FULL captured subtree repaints instead — correct, just
unaccelerated. Not worth a `has_child` gate on the setter: that would trade
this well-understood degrade for a live state read whose own staleness would
need its own argument. (The case is reachable — `RotatedBox::new(n)` seeds an
empty child, so rebuilding such a widget 1→3 takes exactly this route — and
comes out correct through the refusal, which is what
`a_childless_rotated_box_layer_update_falls_back_to_a_repaint_and_clears_the_flag`
pins: the flag is cleared by the fallback repaint, so a later mark is not
self-refused.) And a
setter call before the very first frame is refused outright:
`mark_needs_composited_layer_update` tests `node.needs_paint()`
(`pipeline/scheduler.rs`), not `needs_layout()`, and freshly mounted state
seeds BOTH `NEEDS_LAYOUT` and `NEEDS_PAINT` — so the mark is dropped and the
node is served by its already-scheduled first paint, not by a patch against a
capture that does not exist yet.

**Measured:** `paint/rotated_box_turn_change`, mirroring
`bench_transform_matrix_change`'s two shapes and size list — update/repaint
ratios at N = 1, 10, 100, 1000: inline **1.53x, 3.9x, 25.8x, 222x**; layered
**1.30x, 2.1x, 2.3x, 1.71x** (N = 1 and N = 1000 re-measured 2026-09-11
alongside the other `paint_effects` producers — see "the clip family" below
for the command; N = 10 and N = 100 are unchanged from their first
measurement and were not part of that re-run). Same shape as
`RenderTransform`'s own numbers (226x / 1.69x at N = 1000) for the same
reason: inline leaves merge into one
`PictureLayer`, so the update arm stays flat while the repaint arm grows with
N; layered leaves make the graft itself O(N), narrowing the win to a small
constant factor. The benches install no `SemanticsOwner`, so these ratios
measure the paint side only; the `SEMANTICS` bit's cost is the unchanged
baseline argued above, not something the numbers cover.

**Oracles** (net-new; upstream has no test driving `quarterTurns` as a
setter, so nothing was replaced):
`a_rotated_box_quarter_turn_update_patches_the_layer_and_writes_back`,
`a_rotated_box_parity_change_relayouts_and_swaps_size`, and
`a_childless_rotated_box_layer_update_falls_back_to_a_repaint_and_clears_the_flag`
(`tests/retained_boundary_layers.rs`), plus the unit impact-table tests in
`crates/flui-objects/src/layout/rotated_box.rs` and the layout-parity harness
test named above.

**And now, the clip family: `RenderClip` and `RenderFlow`.** Upstream's gap
is not specific to opacity, transform, or rotated boxes — it is structural,
and the improvement stated above for those three producers applies to any
node's own effects:

> Upstream can serve a layer-property change without a repaint only for a
> node that IS a repaint boundary: `markNeedsCompositedLayerUpdate` degrades
> to `markNeedsPaint` for any other node (`object.dart`),
> `updateLayerProperties` asserts `isRepaintBoundary`, and the class must own
> its layer through a `LayerHandle` and override `updateCompositedLayer` —
> `RenderOpacity` therefore declares `isRepaintBoundary =>
> alwaysNeedsCompositing` to qualify, at the cost of an `OffsetLayer` and a
> retained capture per node. FLUI reads one `PaintEffects` value from any
> node and builds its layers inside the ENCLOSING boundary's flat capture
> through one constructor with two callers, so every effect expressed
> through the value is patchable with no promotion, no layer ownership, and
> no per-class override; the value fixes the nesting (opacity outermost,
> transform innermost, node-local shapes between), which is what lets a
> single per-position layer-kind compare stand as the whole structure guard.
> Re-derive upstream's contract from `markNeedsCompositedLayerUpdate` and
> `updateLayerProperties` in `object.dart` and `RenderOpacity` in
> `proxy_box.dart`, not from a mark grep.

Concretely, for clips: at 3.44.0 `_RenderCustomClip<T>` (`proxy_box.dart`)
extends `RenderProxyBox` and never overrides `isRepaintBoundary`, so it never
qualifies for the promoted path either — its `clipper` setter (`_markNeedsClip`
→ `markNeedsPaint()` + `markNeedsSemanticsUpdate()`) and its `clipBehavior`
setter (`markNeedsPaint()` directly, no semantics) both fall back to a full
subtree repaint on every change, the same degrade as any other non-boundary.
FLUI serves the identical change — a border radius, a clip shape, a path
source token, or `clipBehavior` itself — as a layer patch inside the
enclosing boundary's capture, through `RenderClip<S>::paint_effects` /
`clip_descriptor` (`crates/flui-objects/src/proxy/clip.rs`).

**Per-producer accounting.**

- `RenderClip<S>` (rect / rrect / oval / path): all five setters
  (`set_clip_behavior`, `set_clip_shape`, `set_border_radius`,
  `set_path_clip_source_token`, `set_path_clip_target`) report
  `COMPOSITED_LAYER_UPDATE`, `| SEMANTICS` on the four that change the
  resolved geometry — `set_clip_behavior` does not, because the
  accessibility clip reads `clip_behavior` directly at
  `describe_approximate_paint_clip`'s call site rather than through this
  setter, a pre-existing gap this change does not touch. No setter is
  structural: a `PaintClip` at `Clip::None` is still a layer the pipeline
  builds, and the engine skips pushing it —
  `ClipRectLayer::clips()`/`ClipRRectLayer::clips()`/`ClipPathLayer::clips()`
  (`crates/flui-layer/src/layer/clip_rect.rs` and its rrect/path
  siblings) gate `LayerRender::render`/`cleanup`
  (`crates/flui-engine/src/wgpu/layer_render.rs`), pinned by
  `test_clip_rect_layer_no_clip_is_noop`, `test_clip_rrect_layer_no_clip_is_noop`
  and `test_clip_path_layer_no_clip_is_noop` — so crossing `Clip::None` never
  changes the layer count. A token-driven path clip is reported as
  `PaintClip::PathTarget` (`ClipGeometry::path_target_descriptor`) and
  resolved exactly once, by the walk, never by the setter and never on a
  coordinate query: building the descriptor runs no caller code, and a
  fixed `clip_shape`'s `Arc<Path>` is shared into the descriptor by
  refcount, never copied.
- `RenderFlow`: its clip is gated on `clip_behavior == Clip::None`
  (`paint_effects` returns `PaintEffects::NONE` there, a whole-box
  `PaintClip::Rect` otherwise) rather than reported unconditionally the way
  `RenderClip`'s is — so `set_clip_behavior` stays `PAINT | SEMANTICS`: a
  change across `Clip::None` adds or removes the layer, the one structural
  transition a patch cannot express. This makes `RenderFlow` the production
  type behind the structural-refusal oracle, and it exercises the clip arm
  of `own_effect_layers` on every Flow frame that clips at all; its
  per-child transforms stay in `paint` (Deferred, below). The two
  conventions coexist deliberately — `RenderClip`'s clip is always a layer
  (a property), `RenderFlow`'s is an absence at `Clip::None` (derived from
  the same field it reads for the gate) — and a future reader should not
  "fix" either toward the other.

**Measured** (`cargo bench -p flui-rendering --bench paint -- '_change/(inline|layered)/(update|repaint)/(1|1000)$' --warm-up-time 1 --measurement-time 3`;
this machine, 32 cores, 2026-09-11): at N = 1000, update/repaint ratios are
the same class across every `paint_effects` producer — opacity 228x inline /
1.71x layered, transform 226x / 1.69x, rotated box 222x / 1.71x, **clip
rrect 220x / 1.71x**, **clip path 192x / 1.69x**. At N = 1 the ratios are
the per-node floor every producer shares — clip rrect 1.54x inline / 1.29x
layered, clip path 1.47x / 1.27x (opacity, transform and rotated box:
1.51–1.55x / 1.30–1.32x). The update arm runs ≈0.9–1.1 µs on the inline
shape regardless of producer (layered: ≈160 µs, the graft is O(retained
layers) — every layered leaf is its own repaint boundary, so patching one
still clones the whole retained capture, unlike inline leaves sharing one
`PictureLayer`); the repaint arm runs ≈205–210 µs inline, ≈270–275 µs layered —
the same figures measured when the paint walk switched to reading one
`PaintEffects` value (a structural ≈4–5% rise on the repaint arm, recorded
at that switch), so none of this is new overhead from the clip producers
themselves. The path ratio
sits below the rest for a stated reason, not a regression: `resolve_path_clip`
runs the registered clipper exactly once on BOTH arms — once to rebuild the
one patched layer, once as part of the whole-subtree repaint — adding the
same ~140 ns to each side of the same division, which compresses the ratio
toward 1x and cannot push it below 1x.

**Replacement tests:**
`a_border_radius_change_updates_the_clip_layer_without_repainting_the_subtree`,
`a_clip_layer_update_is_written_back_into_the_retained_capture`,
`a_same_frame_layout_change_forces_the_repaint_a_clip_patch_relies_on`,
`a_flow_clip_behavior_change_is_structural_and_refused`, and
`two_path_clips_under_one_boundary_resolve_in_paint_order`
(`tests/retained_boundary_layers.rs`; `cargo nextest run -p flui-rendering
--locked -E 'test(<name>)'`);
`harness_transform_to_through_a_path_clip_runs_no_registered_clipper`
(`tests/render_object_harness.rs`; `cargo nextest run -p flui-objects
--locked -E 'test(harness_transform_to_through_a_path_clip_runs_no_registered_clipper)'`);
`rebuilding_a_clip_rrect_widget_updates_its_layer`
(`tests/clip_rrect_layer_update.rs`; `cargo nextest run -p flui-widgets
--locked -E 'test(rebuilding_a_clip_rrect_widget_updates_its_layer)'`) —
pins `ClipRRect::update_render_object` forwarding `set_border_radius` and
its impact to the owner; it cannot see which paint arm served the frame,
and says so, pointing at the render-level tests above for that; and the
pixel oracle, `the_clip_update_path_and_a_repaint_produce_the_same_pixels`
plus `a_different_radius_produces_different_pixels`
(`tests/composited_layer_update_readback.rs`; `cargo nextest run -p flui
--features gpu-readback-tests --no-default-features --test
composited_layer_update_readback --locked --test-threads 1`).

**Deferred producers.** Not served by this change, each for a stated reason:

| Producer | Why not now | Trigger | Shape when reopened |
|---|---|---|---|
| `RenderBackdropFilter` filter/blend (`enabled` → `PAINT`) | no widget → no caller | the `BackdropFilter` widget (upstream driver: `FlexibleSpaceBar` blur) | `backdrop_filter:` field at a fixed position (alpha does not commute — position decided at the field) |
| `RenderShaderMask` | no widget; its shader is lane-resolved like a path clipper | the `ShaderMask` widget | `shader_mask:` field with a walk-resolved target, as `PathTarget` |
| `ImageFiltered` | no render object | its port (upstream driver: `StretchingOverscrollIndicator`) | a field; `enabled` → `PAINT` |
| `RenderPhysicalModel`/`RenderPhysicalShape` | shadow + fill are display-list content, and a descriptor clip wraps own draws | `Material` implicit elevation/shape animation (`_MaterialInterior`) | seal a drawing scope-owner's own draw runs into a picture of their own and re-record only that picture on a property change; measure the layered shape first; needs an ADR |
| `RenderFlow` per-child transforms | variable fragment | an animated `Flow` delegate in the catalog | re-record the flow's own fragment (a per-node picture re-record, not a layer patch); the `paint_effects` clip stays the prefix outside it |
| `RenderFittedBox` | NOT nesting (the fixed order is exactly its clip-outside-transform) but its translation fast path (`paint_child_at`, a `Some ↔ None` transform transition) and its overflow-gated layout-derived clip | a per-frame fit animation in the catalog | a per-node fragment re-record for the child offset; the overflow clip stays structural |
| Overflow-gated clips (`RenderStack`/`RenderIndexedStack`, `RenderWrap`, `RenderConstraintsTransformBox`, `RenderAnimatedSize`, box `RenderViewport`/`RenderShrinkWrappingViewport`; rule: `has_visual_overflow && clip_behavior != Clip::None`, rect = own size — enumerate with the `rg` command in trade-off 1 above) | no patchable property | none; re-check the enumeration when one gains a property | n/a |
| `ClipSuperellipse` | no render object, widget, or variant (its layer debug-asserts against `Clip::None`, unlike rect/rrect/path, which only skip the push) | its port | a `PaintClip` variant; note the per-layer `Clip::None` difference |
| `blend` on `PaintOpacity` | zero producers | the first advanced-blend producer | additive field on the `#[non_exhaustive]` struct |

The market survey behind this shape (Prime Directive rule 2) covered
Compose, SwiftUI, Slint, GPUI, and Masonry/Xilem; Compose's
[`Modifier.graphicsLayer`](https://developer.android.com/reference/kotlin/androidx/compose/ui/graphics/graphicsLayer.modifier)
is the closest market analogue — one declarative value per layer (`alpha`,
`clip` + `shape`, the transform fields, `renderEffect`, `blendMode`) whose
property-only changes update the retained layer without re-recording the
display list, the same contract `PaintEffects` gives FLUI.

### `RenderRotatedBox` reports a baseline only for an even turn

**Rule:** Prime Directive rule 1 — a deliberate improvement over the reference,
accounted for here.

**Upstream:** `RenderRotatedBox` has no baseline override at all
(`rotated_box.dart`, 3.44.0: `grep -c aseline` is 0), so its live query
inherits `RenderBox.computeDistanceToActualBaseline`'s `null` for every turn
— a baseline-aligned `Row` places a rotated box at the cross start whatever
the turn, even an unrotated one — and its dry query falls through to
`RenderBox.computeDryBaseline`'s default, which asserts in debug builds
(`box.dart`, `debugCannotComputeDryLayout`) rather than answering.

**Choice:** a baseline is a layout line. An even turn keeps the box's size and
its horizontal axis, so the box takes part in baseline alignment exactly as
its unrotated self would — the child's baseline is forwarded unchanged and the
glyphs flip in place at turn 2. That is the model every draw-time rotation
already uses: `RenderTransform` here (a proxy, via
`forward_single_child_box_queries!`), Compose's `Modifier.rotate`, SwiftUI's
`.rotationEffect`. An odd turn rotates the baseline axis into the vertical, so
there is no horizontal baseline to offer and the box is treated like any child
without one — as upstream treats it at every turn.

Both halves of the query answer from the same predicate: the dry half in
`compute_dry_baseline`, and the live half through
`forwards_baseline_to_only_child`, which returns `!is_vertical()` so the
layout driver walks to the child as it does for a pure proxy. The two must
agree, and the live half is the one that matters: the only consumer of a
rotated box's baseline is a baseline-aligned parent's `perform_layout`, which
asks the live query — a dry-only forward is inert there, and a same-parity
equality pin cannot tell either half's value.

**Alternatives:** match upstream (`None` for every turn) — loses the identity
case for nothing. Forward only at turn 0, or mirror the line to
`height − baseline` at turn 2 — both read the exact turn rather than its
parity, which breaks the layout-turn-blindness the same-parity fast path above
rests on: `set_quarter_turns` would have to report `LAYOUT` for 0↔2 and the
parity pin would need a baseline exception, to serve a case no framework
animation drives (`RotationTransition` drives `Transform::rotation`; a widget
rebuild 0→2 does occur and is what the layer-update route serves). Rejected
on that cost, not on geometry.

**Replacement oracle:**
`harness_rotated_box_baseline_follows_the_child_for_even_turns_and_is_absent_for_odd`
(`crates/flui-objects/tests/render_object_harness.rs`) — a baseline-aligned
`Row` places `RotatedBox(0)` and `RotatedBox(2)` where their child would sit
and `RotatedBox(1)` / `RotatedBox(3)` at the cross start, and asserts the dry
answer by value at all four quadrants; red on the turn-0 offset when the live
forward is refused, red on the turn-1 offset when it is granted for an odd
turn, red on the dry rows when `compute_dry_baseline` drifts to either
"always" answer. Upstream has no baseline test for the class, so nothing was
replaced.

### A transform patch may reuse a captured origin only because layout forces a repaint

**Rule:** `layer_patches_for` rebuilds a node's effect layers at the `origin`
stored *when the capture was taken*, not a live one. An `OpacityLayer` is
position-independent (always `Offset::ZERO`), so an alpha patch is correct
wherever the node sits. A `TransformLayer` is **conjugated by** that origin, so a
transform patch is correct only while the captured origin still matches the
node's live accumulated position.

**Why that holds:** the `mark_needs_layout` flag walk sets `NEEDS_LAYOUT` on
every ancestor up to the relayout boundary, so a flagged node cannot take the
constraints-cache short-circuit — the enclosing repaint boundary re-enters
layout and is recorded as having laid out (`pipeline/owner/layout.rs`), and if
the relayout boundary sits below the boundary that owns the capture, the
dirty-root mark (FLUI's once-per-walk counterpart of the per-object
`markNeedsPaint()` Flutter's `RenderObject.layout` ends with) walks up to it
too. Either
route enqueues `Repaint`, which upgrades the `LayerUpdate` entry the setter
queued (`PaintQueue::enqueue` never downgrades). So any same-frame layout
change inside a boundary — anything that could move the node — reaches it one
way or the other, and the patch path is simply unreachable with a stale
origin.

**Accepted trade-off:** this is a coupling between two subsystems that is stated
nowhere in the code they connect. An optimisation that decouples "the boundary
moved" from "the boundary is marked needing paint" — a plausible future change to
that `mark_needs_paint` loop — silently makes transform patches render about the
wrong point. `a_same_frame_layout_change_forces_the_repaint_a_transform_patch_relies_on`
exists to fail loudly if that happens; replacing the loop body with a no-op fails
it with the layer count and discriminant right and the translation column wrong.

**A patch that panics poisons the frame instead of being refused.**
Unlike a structural shape mismatch — which `layer_patches_for` refuses by
returning `Ok(None)`, falling back to a repaint — a panic while rebuilding a
target's own effect layers (inside `paint_effects`, or the walk's resolution
of a `PaintClip::PathTarget` it reports) cannot be refused the same way: the
fallback repaint would call that same panicking `paint_effects` on the same
node and panic a second time. So it poisons instead
(`RenderError::Poisoned { phase: PoisonPhase::LayerUpdate, .. }`), the frame
is discarded whole, and the queued update survives on the node for the retry.

The captured-origin invariant above now has a clip oracle beside the
transform one:
`a_same_frame_layout_change_forces_the_repaint_a_clip_patch_relies_on`
(`tests/retained_boundary_layers.rs`) pins the identical
same-frame-layout-forces-a-repaint invariant through a clip patch instead of
a transform one. The two differ in one respect worth being precise about: a
clip rect is *translated* by the captured origin (`clip_layer`, same file),
never conjugated by it the way a transform matrix is — but a stale origin is
exactly as wrong for a translation as for a conjugation, so the same
same-frame-layout-marks-the-boundary-needing-paint argument covers both. No
shipped clip producer routes an update-only commit through `paint_effects`'s
`clip` field yet (see trade-off 1 above), so the oracle drives a test double
that reports `COMPOSITED_LAYER_UPDATE` for a rect change on purpose, to
exercise the arm at all.

### A retained capture holds no GPU resource, so device loss cannot strand one

**Rule:** #536 asks that "detach, reattach, and device-loss paths reject stale
handles". The detach and reattach halves apply; the device-loss half does not,
and the reason is worth recording so it is not re-litigated.

**Choice:** nothing to do. The criterion is written against Flutter's model,
where `Layer` owns an `EngineLayer` that IS a GPU resource — which is exactly
why `LayerHandle`, ref-counting and `Layer.dispose` exist there. FLUI's `Layer`
is a pure value: `TextureLayer` and `PlatformViewLayer` hold plain ids into the
engine's own registries, and `PictureLayer` holds an `Arc<DisplayList>` whose
image commands hold `Arc<Vec<u8>>` RGBA bytes. A retained capture is therefore
CPU data end to end.

A device loss destroys GPU state inside `flui-engine`, which rebuilds it from
the layer tree the pipeline hands it every frame — and the tree a graft produces
is the tree a repaint would have produced. There is no handle to reject.

**Accepted trade-off:** if a layer variant ever comes to own a GPU resource
directly, this stops holding and captures would need invalidating on device
loss. The property is not enforced by anything today beyond the layer types
themselves.

### Layout marks semantics once per walk, at the dirty root

**Rule:** Flutter pairs `performLayout()` with `markNeedsSemanticsUpdate()` in *both* of
`RenderObject`'s layout entry points (`rendering/object.dart`, `layoutWithoutResize` and
`layout`), per object. Every node that lays out re-publishes its semantics geometry, which is
what makes a scroll update the accessibility tree at all: a viewport's offset listener requests
layout and nothing else.

**Choice:** the same guarantee, marked **once per layout walk on the dirty root**
(`layout_dirty_root`) rather than once per laid-out node.

**Alternatives considered:** recording every laid-out node in the arena and marking each, which
is the literal transcription — rejected on two counts. It is redundant: the arena walks the
subtree of the dirty root, so every node that laid out is already under it, and `try_graft_pass`
re-assembles a marked node's whole subtree. And it is expensive in a way the transcription hides:
each `add_node_needing_semantics` fires `fire_need_visual_update`, whose production callback asks
the platform to redraw, and the graft resolves every marked node by walking its ancestor chain —
so an N-node relayout would cost N redraw requests and O(N·depth) graft work.

**Trade-off accepted:** one unconditional call per layout walk. `mark_needs_semantics` is a
no-op while semantics is disabled, so a session with no accessibility client attached pays one
predictable branch. A relayout of a subtree re-assembles that subtree even where a node's own
geometry did not move — the graft's granularity is the anchor, not the node, which is the same
bargain the existing graft already makes.

**Replacement test:** `scrolling_republishes_the_semantics_rects`
(`crates/flui-widgets/tests/semantics.rs`). It scrolls a viewport whose rows are *all* inside the
cache band, so the frame materialises nothing new, and asserts on a build counter that no row
rebuilt — without that assertion the test measures a newly-built row's own semantics mark and
passes with the change reverted, which the first draft did.

### The hit-test path is driver-owned; the protocol carries no result accumulator

**Rule:** [`AGENTS.md`](../../AGENTS.md) Prime Directive #1 — a contract may be improved, and an
improvement owes a record plus a replacement test. This is that record.

**Choice:** `HitTestCapability::Result` and `::Entry` are vocabulary only. There is no
`ctx.result()`, `result_mut()`, `add_hit(entry)` or `add_self(id)`: the driver
(`PipelineOwner`'s hit-test walk) owns the path and builds each entry from the node's own
`RenderId`. A render object says it was hit by returning `true`, or calls
`ctx.register_self_hit_entry()` to appear in the path without blocking what is behind it.

**The reference's shape:** Flutter's `hitTest` takes a `HitTestResult` and each render object
calls `result.add(BoxHitTestEntry(this, position))`. The accumulator is the protocol.

**Why the divergence is better here, in checkable terms:**

1. **A render object cannot get the id wrong**, because it never supplies one. Flutter's
   `add(BoxHitTestEntry(this, …))` takes the node as an argument; passing the wrong one, or
   adding twice, is expressible and silent.
2. **There is one writer, not N.** The driver knows the node, its transform and its position in
   the walk, so the entry is assembled once from state that cannot disagree with itself. FLUI's
   accumulator was the second writer, and — this is the finding that produced the deletion — it
   was *unread*: `add_self` compiled, ran, and did nothing, because nothing downstream consumed
   the protocol-level result (issue #844).
3. **The trap is gone rather than documented.** The broken call was the discoverable one: it took
   the id you were holding and read like the box-side API. Deleting it makes the wrong call
   impossible instead of warned against.

**Alternatives:**
- *Wire the accumulator so the driver bridge reads it* — the honest alternative, and the one to
  take if a consumer ever appears (a sliver assembling its own path). Rejected now for having no
  consumer: wiring a second writer into the hit path to serve nothing would add exactly the
  disagreement point item 2 removes.
- *Keep the API and document the trap* — rejected; the deleted method's own module already
  documented it in passing ("dead in production") and that stopped nobody.

**Replacement coverage:** `register_self_hit_entry` is exercised end-to-end by the widget-level
hit-test ports that dispatch through a real pipeline — the `Transform`, `ClipPath`, `ClipRect`,
`Wrap` and viewport-order cases in `crates/flui-widgets/tests/parity/`, each asserting a tap
reaches or misses a specific child. The deleted tests asserted a write landed in a structure
nobody read, so they were removed rather than adapted: they could not fail for a reason a user
would notice. `crates/flui-widgets/tests/parity/render_viewport_test.rs` carries the debug trail
of how the dead path was found.

### Lazy-sliver scroll correction keeps the first visible item stationary

**Rule:** Prime Directive rule 1 ("improve where a Flutter contract can be improved, record it, replace the oracle"); [ADR-0051](../../docs/adr/ADR-0051-anchor-stationary-scroll-correction.md).

**Choice:** `Virtualizer::set_measured` / `adapt_default_estimate` report the offset delta of the anchor (the first visible item) whenever an extent above it changes; the consumer sliver accumulates the deltas and emits them as `SliverGeometry::scroll_offset_correction` at the end of the pass, in either scroll direction. The viewport applies the correction and re-runs layout in the same pass, so the anchor never moves on screen.

**Alternatives:** Flutter's `RenderSliverList` retains each resident child's stale `layoutOffset`, walks forward from the first retained child with current sizes, and corrects only at a boundary — growth of a retained-but-invisible child shifts visible content. ADR-0003's original consumer note additionally withheld corrections during a backward scroll; measured on the oracle scene it changed nothing and, where it can act, it is a one-frame anchor drift.

**Accepted trade-off:** the `slivers_test.dart` 'inaccurate scroll offset' windows differ from the oracle's by exactly the growth Flutter shows as a jump (192 px in that scene); the pinned oracle stays `#[ignore]`d as the statement of the declined behaviour and a FLUI oracle stands beside it. Items above a jump that were never resident stay hinted until they enter the band (O(band) layout, ADR-0003), where Flutter's O(distance) walk would be exact.

### Render-tree storage uses a `Slab<RenderNode>` with `RenderId` (NonZeroUsize) keys

**Rule:** strategy clause "Behavior as floor, everything else designed for Rust"; constitution Anti-Patterns list ("`Arc<Mutex<>>` for tree structures — use arena/slotmap"); the ID-offset pattern documented in [`docs/architecture.md`](../../docs/architecture.md).

**Choice:** `RenderTree` stores `Slab<RenderNode>`. `RenderId` is a `NonZeroUsize` newtype that adds `+1` to the slab index, so `Option<RenderId>` niche-optimises to 8 bytes for parent / child references. The slab is reached from one strong root (`PipelineOwner::root_id`) and every other node is reached by walking child IDs in `NodeLinks`.

**Alternatives:** Flutter holds the tree as a graph of Dart references with direct child pointers on every render object. Direct translation would require `Arc<RwLock<RenderObject>>` or `Rc<RefCell<RenderObject>>` for parent/child cycles, which the constitution forbids for tree structures. `typed-arena::Arena` was considered but cannot delete individual entries, which the element reconciler needs.

**Accepted trade-off:** one extra indirection (slab lookup) on the tree-walk hot path, paid back by O(1) insert/delete, deterministic ID stability across mutations, and elimination of `Arc<Mutex<>>` cycles. The same pattern is used by `flui-view`'s `ElementTree`.

### `RenderEntry<P>` owns the render object by value (no lock, no interior mutability)

**Rule:** strategy clause "sync hot path, async на краях" (lock contention on the hot path is functionally async-flavoured); [`docs/PORT.md`](../../docs/PORT.md) Refusal trigger 1 (`RwLock<Box<dyn RenderObject<P>>>` in `perform_layout` / `paint`).

**Choice:** `RenderEntry<P>::render_object` is a plain `Box<dyn RenderObject<P>>` (see [`src/storage/entry.rs`](src/storage/entry.rs)). Mutable access goes through `&mut self`, which the pipeline obtains via `PipelineOwner::render_tree_mut() -> &mut RenderTree` at phase boundaries. Re-entrant access from a parent to a child during layout uses disjoint-borrow primitives on `RenderTree` (`get_two_mut`, `get_many_mut`; the underlying `unsafe` is local and disjoint-keys-invariant — see [Thread safety](#thread-safety)). The Flutter `_debugDoingThisLayout` / `_debugDoingThisPaint` debug asserts are mirrored by `PipelineOwner::debug_doing_layout` / `debug_doing_paint` (see [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs)).

**Alternatives considered (full study in [`docs/plans/2026-05-19-001-feat-flutter-port-methodology-plan.md`](../../docs/plans/2026-05-19-001-feat-flutter-port-methodology-plan.md)):**
- `OnceCell<Box<dyn>>` — rejected. `OnceCell::get()` returns `&T`; the trait still has `&mut self` methods that need mutation, so the lock would have to come back under another name.
- Arity-keyed enum dispatch — rejected. The trait is open-set via the blanket `impl<T: RenderBox + Diagnosticable> RenderObject<P> for T` (see [`src/traits/render_box.rs`](src/traits/render_box.rs)). Closing it to a known enum would force every user-defined render object into a derive-macro discipline and break the widget extensibility story.
- `RenderObjectId` indirection (render object lives in a separate slab keyed by ID) — considered. Adds one extra indirection per access and doubles the lifecycle invariants (insert/delete across two slabs). Equivalent soundness-wise but more moving parts than necessary.
- Inner-mutability split (immutable `Arc<dyn>` config + all mutation moved to `RenderState`) — considered. Largest API change of all the options; would force every concrete render object in `src/objects/` to be refactored. Filed as future work.

**Accepted trade-off:** the layout and update paths must hold `&mut RenderTree` for the duration of the phase. Multi-child layout requires the `get_many_mut` primitive. The borrow checker, not a lock, enforces single-writer-per-frame — closer to Flutter's actual model (single-threaded with debug asserts) than the previous `RwLock`-based shape.

### `set_was_repaint_boundary` removed from the trait surface; bit lives on `RenderState::flags`

**Rule:** [`docs/PORT.md`](../../docs/PORT.md) Refusal trigger 1 (the previous shape required a write lock on the trait object during paint to flip a single bool); strategy clause "Compile-time over runtime" (state bits belong on the bookkeeping layer, not the user-implementable trait surface).

**Choice:** added `RenderFlags::WAS_REPAINT_BOUNDARY` (bit 10 — see [`src/storage/flags.rs`](src/storage/flags.rs)) with `RenderState<P>::set_was_repaint_boundary` / `was_repaint_boundary` accessors. The paint phase at [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs) (`paint_subtree`) writes the bit through an atomic store on `state().flags()` rather than locking the trait object. The trait method `RenderObject::set_was_repaint_boundary` is deleted (see [`src/traits/render_object.rs`](src/traits/render_object.rs)).

**Alternatives:** keep the trait method and live with the per-paint write lock — rejected, this is the canonical refusal-trigger violation. Move the bit to a per-tree side table — rejected, would add a second source-of-truth for state already structured around `RenderState<P>`.

**Accepted trade-off:** subclasses that wanted to override `set_was_repaint_boundary` (none currently do) lose the hook. The flag's owner is now framework code, not user code. This mirrors Flutter's actual model where `_wasRepaintBoundary` is a private field on `RenderObject` (`object.dart` line 3560) that no subclass overrides.

### `unsafe impl Send + Sync for RenderTree` removed

**Rule:** constitution Principle III ("zero unsafe in widget/app layer; `unsafe` only in `flui-platform`, `flui-painting`, `flui-engine`"); the prior `unsafe impl` was a soundness carve-out documented in [`docs/plans/2026-03-31-core-crates-hardening.md`](../../docs/plans/2026-03-31-core-crates-hardening.md) Task 7.

**Choice:** removed the `unsafe impl Send for RenderTree {}` / `unsafe impl Sync for RenderTree {}` block at the bottom of [`src/storage/tree.rs`](src/storage/tree.rs). The transitive Send+Sync chain still holds via auto-derivation: `Slab<RenderNode>` is auto-`Send + Sync` because `RenderNode` is; `RenderEntry<P>` holds `Box<dyn RenderObject<P>>` and the trait requires `Send + Sync + 'static`; `RenderState<P>` is built on atomics and `Option<T>` fields for geometry/constraints; `NodeLinks` is POD.

**Alternatives:** keep the unsafe impl as defensive cruft — rejected, the safety justification was load-bearing only because of `RwLock`'s interior mutability; with that gone, no unsafe carve-out is needed.

**Accepted trade-off:** net unsafe deletion, one fewer place where the carry-cost of a soundness comment exists.

### Third-party trait calls wrapped in `catch_unwind`; phases return `RenderResult<()>`

**Rule:** design verdict Section 7 ("Partial failure recovery: A render object that panics inside `perform_layout` or `paint` poisons that node only. The pipeline catches via `std::panic::catch_unwind`, marks the node as `RenderError::Poisoned`, drops the in-flight frame, and lets the caller decide.") and Section 10 (the `Poisoned { render_object, phase }` error variant). Mythos Step 12.

**Choice:** every third-party trait call site has its call wrapped in `std::panic::catch_unwind(AssertUnwindSafe(|| ...))`. A panicking render object surfaces as `RenderError::Poisoned { render_object, phase }` rather than aborting the process. Specifically:

- `RenderEntry::layout` ([`src/storage/entry.rs`](src/storage/entry.rs)) wraps `render_object.perform_layout_raw(...)` and returns `RenderResult<ProtocolGeometry<P>>`. On the panic path, state is left untouched (`NEEDS_LAYOUT` stays set) so the next frame can retry. The retry is not unbounded: the pipeline counts consecutive layout failures per node and poisons nodes that fail structurally or exhaust the budget ([`src/pipeline/owner/poison.rs`](src/pipeline/owner/poison.rs)); a poisoned node is skipped in later walks until `mark_needs_layout` freshly invalidates it.
- `PipelineOwner::<PaintPhase>::paint_subtree_impl` ([`src/pipeline/owner/paint.rs`](src/pipeline/owner/paint.rs)) wraps `render_node.paint_raw(&mut recorder, ...)` (the fragment recorder) together with the node's own effect-layer build, `own_effect_layers(render_node.paint_effects(), origin)`, in ONE `catch_unwind`. Both run only after the walk's three return gates (`skip_paint`, `needs_layout`, the sliver visibility cull): a gated-out node never builds a `PaintEffects` descriptor it will not use. Building it there matters: reached outside those gates, a node whose `needs_layout` flag is still set carries stale geometry — its last committed size, or `Size::ZERO` if it has never been laid out — so its descriptor would be built against a size this pass never computed. A panic in either half — the node's own `paint_effects`, or the walk's resolution of a `PaintClip::PathTarget` inside `own_effect_layers`'s clip arm — surfaces as `Poisoned { phase: PoisonPhase::Paint, .. }`, one poison point per node rather than two.
- `PipelineOwner::<PaintPhase>::layer_patches_for` ([`src/pipeline/owner/paint.rs`](src/pipeline/owner/paint.rs)), the composited-layer-update patch arm, wraps the same `own_effect_layers(node.paint_effects(), slots.origin)` rebuild — run once per target being patched into a retained boundary's capture — in its own `catch_unwind`. A panic surfaces as `Poisoned { phase: PoisonPhase::LayerUpdate, .. }` and the function returns `Err` instead of `Ok(None)`: the whole frame is discarded and the queued composited-layer-update request survives on the node for the retry. Falling back to a repaint instead (the way a structural shape mismatch does, via `Ok(None)`) was rejected — the repaint would call the same panicking `paint_effects` on the same node and panic a second time.

The phase entry points (`run_layout` / `run_compositing` / `run_paint` / `run_semantics`) now return `RenderResult<()>`. `run_frame` returns `(PipelineOwner<Idle>, RenderResult<Option<LayerTree>>)` -- the owner **always** comes back at Idle so frame-loop callers can mutex-replace through it on both success and error paths.

`Poisoned`'s `phase` is a [`PoisonPhase`](src/error.rs) (`Layout` | `Paint` | `LayerUpdate`, rendered `"layout"` / `"paint"` / `"layer-update"` by its `Display` impl) — the enum itself is the authoritative, closed set; nothing today produces a fourth variant.

This whole per-node partial-failure-recovery mechanism diverges deliberately from Flutter, and in the opposite direction from what a first read of `object.dart` suggests. `RenderObject._paintWithContext` (3.44.0) wraps only the call to `paint(context, offset)` in `try { … } catch (e, stack) { _reportException('paint', e, stack); }`; `_reportException` forwards to `FlutterError.reportError` and returns — it does **not** rethrow. `PipelineOwner.flushPaint` then continues its depth-sorted walk of `_nodesNeedingPaint` with the next dirty node: one node's exception is isolated to that node, and the rest of the frame still paints and reaches the compositor. `_debugDoingThisPaint` is a debug-only reentrancy assert (catches painting the same node twice in one pass) — it plays no part in exception handling, and does not abort anything. The one gap in Flutter's own isolation is `PaintingContext.updateLayerProperties`, the composited-layer-update-only arm `flushPaint` takes instead of a full repaint: it calls `child.updateCompositedLayer(oldLayer: childLayer)` with no surrounding `try`/`catch` at all, so an exception there is not caught the way `paint`'s is.

FLUI diverges on both arms, in the stricter direction: a panic in `paint_effects` (`PoisonPhase::Paint`) or in `layer_patches_for`'s rebuild (`PoisonPhase::LayerUpdate`) discards the WHOLE frame as `Poisoned` rather than isolating the one node — the dirty queue survives, so the node is retried next frame, but nothing from the poisoned frame reaches the compositor. The trade-off: no partially painted frame is ever presented (Flutter's per-node isolation can and does present one), at the cost that a node stuck panicking blocks the whole frame from completing until it is replaced or stops panicking.

`RenderObject<P>::debug_name(&self) -> &'static str` is the static identifier embedded in `RenderError::Poisoned`. Its default body monomorphizes per concrete impl via `core::any::type_name::<Self>()`; calling through `&dyn RenderObject<P>` yields the concrete type name because the vtable carries the monomorphized stub.

**Alternatives:**

- **Process-wide `panic::set_hook`** -- rejected, leaks pipeline concerns into global process state and can't differentiate phase-of-origin.
- **Cache `debug_name` on `RenderEntry<P>` at insertion** -- considered. Would avoid one vtable dispatch per error case. Not adopted because the dispatch happens only on the failure path (cold by definition), and the cache adds a `&'static str` field that pollutes every `RenderEntry<P>` in the common case.
- **Return `(PipelineOwner<Idle>, RenderError)` tuple on error** (shape (a) in the Mythos spec) -- rejected, awkward to compose; pattern-matching on `(_, Result<_>)` is cleaner than splitting the success and error tuples.

**Accepted trade-off:** `AssertUnwindSafe` is documented inline at each wrapper. The render object's internal state may be torn after a panic; the pipeline treats the node as poisoned and lets the caller drop or replace it. Process-level safety is preserved; the render tree itself is not corrupted.

**Note:** `hit_test_raw` is part of the `RenderObject<P>` trait, but the current pipeline owner does not invoke it directly -- hit testing is dispatched at the `RenderView` layer outside the frame pipeline. The catch_unwind helper around hit_test will land when hit testing is wired through the pipeline.

### Multi-source design references in this crate

Strategy clause "Behavior as floor, everything else designed for Rust" treats Flutter as the **semantic** floor, not the design. The structural shape of individual components in this crate has been informed by multiple Rust-side audited references as recorded in prior plans:

- `slab::Slab` storage pattern with `+1/-1` ID offset — internal precedent in [`src/storage/tree.rs`](src/storage/tree.rs); the offset rationale lives in [`docs/architecture.md`](../../docs/architecture.md).
- `Weak<RwLock<PipelineOwner>>` parent back-reference replacing a raw pointer — [`docs/plans/2026-03-31-core-crates-hardening.md`](../../docs/plans/2026-03-31-core-crates-hardening.md) Task 7.
- Lock-free atomic dirty tracking (`AtomicRenderFlags` + `AtomicOffset`; geometry/constraints as `Option<T>` mutated via `&mut RenderState`) — documented in [`src/storage/state.rs`](src/storage/state.rs) module docstring.
- Multi-source design references (GPUI, Iced, Makepad, Vello, Skia) — [`docs/plans/2026-03-31-engine-hardening.md`](../../docs/plans/2026-03-31-engine-hardening.md) precedent for citing reference codebases beyond Flutter when the structural pattern fits Rust idioms better.

---

## Thread safety

`flui-rendering` runs in the render pipeline; per strategy clause "sync hot path", the hot frame loop is single-threaded. Sync primitives in this crate are limited to shared-infrastructure objects and lock-free atomics on per-node state. No primitive sits inside `perform_layout` / `paint` on a per-node basis.

| Site | Primitive | Category | Notes |
|---|---|---|---|
| `RenderEntry<P>::render_object` (`src/storage/entry.rs`) | plain `Box<dyn RenderObject<P>>` | Owned by value | Mutable access via `&mut self` from `&mut RenderTree`. The previous `RwLock<Box<dyn>>` was the canonical refusal-trigger violation; removed by the U2 exemplar refactor. |
| `RenderState<P>::flags` (`src/storage/state.rs`) | `AtomicRenderFlags` (wrapping `AtomicU32`) | Lock-free atomics | Bit-level dirty flags + boundary bits. `Acquire/Release` ordering. The new `WAS_REPAINT_BOUNDARY` bit lives here. |
| `RenderState<P>::geometry`, `constraints` (`src/storage/state.rs`) | `Option<ProtocolGeometry<P>>` / `Option<ProtocolConstraints<P>>` | Mutable via `&mut self` | Set and cleared via `&mut RenderState` during layout; no lock required. |
| `RenderState<P>::offset` (`src/storage/state.rs`) | `AtomicOffset` | Lock-free atomics | Paint position. |
| `RenderTree::owner` (`src/storage/tree.rs:65`) | `Option<Arc<RwLock<PipelineOwner>>>` | Shared infrastructure | Allowed per [`docs/PORT.md`](../../docs/PORT.md) lock-decision table. Off the per-node hot path. |
| `PipelineOwner` parent/back-references throughout [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs) | `Arc<RwLock<PipelineOwner>>`, `Weak<RwLock<PipelineOwner>>` | Shared infrastructure | Soundness-rewrite precedent ([core-crates-hardening Task 7](../../docs/plans/2026-03-31-core-crates-hardening.md)). |
| `RenderTree::nodes` (`src/storage/tree.rs:59`) | `Slab<RenderNode>` | Auto-derived Send+Sync | No `unsafe impl` needed after U2. |
| Viewport listener list (`ScrollableViewportOffset::listeners`, `src/view/viewport_offset.rs`) | `RwLock<Vec<…>>` | Listener registry | Off layout/paint hot path. `FixedViewportOffset`'s former listener list was deleted as speculative API (a fixed offset never notifies). |

Two rows left this table because their sites left the crate: the mouse tracker
lives in `flui-interaction` (`src/routing/mouse_tracker.rs`) and the render-view
error builder in `flui-view` (`src/view/error.rs`); each is accounted for in its
owning crate.

`NodePtr` in `src/pipeline/owner/subtree_arena.rs` is a plain raw-pointer newtype for the disjoint-subtree-borrow substrate ([`SubtreeArena`]) — `!Send + !Sync` by the language default, no manual impl. Confinement to the constructing thread is structural (`SubtreeArena` itself is `!Send + !Sync`, pinned by `static_assertions::assert_not_impl_any!`); there is no runtime thread check (`check_thread` and the pointer's former `unsafe impl Send/Sync` were both deleted once `PipelineCell`/dropped `Send + Sync` bounds made confinement type-enforced). Re-entrancy primitives `RenderTree::get_two_mut` and `get_parent_and_children_mut` (both in `src/storage/tree.rs`) are implemented and shipped; their unsafe is local to each function with unit-testable disjoint-keys invariants.

---

## Friction log

Known sites that do not yet match the methodology but are not violations of the current refusal triggers. Each entry names the site and the next planned step.

- **`PipelineOwner` paint-loop downcasts to `Box<dyn ContainerLayer>`** ([`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs)) — the paint phase uses `Box<dyn ContainerLayer>` returned from `RenderObject::paint`. This is correct for compositing-layer heterogeneity but worth periodic audit to ensure the cost stays at the boundary, not in the per-frame inner loop.
- **`docs/PROTOCOL_ARCHITECTURE.md` predates this template** ([`docs/PROTOCOL_ARCHITECTURE.md`](docs/PROTOCOL_ARCHITECTURE.md)) — a deeper design write-up that overlaps with `## Flutter source mapping` above for protocol-specific concerns. Not migrated under this template in U3; remains as a companion document.
- **`docs/LAYOUT_SYSTEM.md`, `docs/PAINT_SYSTEM.md`, `docs/HIT_TEST_SYSTEM.md`** — subsystem-level deep-dives. Not part of the template surface. Stay as companion documents.

---

## Shipped infrastructure (formerly "Outstanding refactors")

These items were listed as pending in earlier drafts; all are now shipped.

### `RenderTree::get_two_mut` / `get_parent_and_children_mut` — SHIPPED

**File:** [`src/storage/tree.rs`](src/storage/tree.rs) (`get_two_mut`, `get_parent_and_children_mut`).

Tree-aware disjoint-borrow primitives. `get_two_mut(a, b)` returns `(&mut RenderNode, &mut RenderNode)` for two distinct keys; `get_parent_and_children_mut` generalises to a parent + N children. The unsafe is local to each function with a disjoint-keys assertion and is unit-tested.

### `layout_dirty_root` + `layout_subtree_borrowed` — SHIPPED

**Files:** [`src/pipeline/owner/layout.rs`](src/pipeline/owner/layout.rs) (`layout_dirty_root`), [`src/pipeline/owner/subtree_arena.rs`](src/pipeline/owner/subtree_arena.rs) (`layout_subtree_borrowed`).

`layout_dirty_root` is the dispatcher: it obtains disjoint `&mut`s via `SubtreeArena`, constructs a typed `BoxLayoutCtx` with children + callback, and calls `perform_layout_raw` through the erased view. The pipeline-driven path was built directly into this entry point; the phantom stubs that earlier documentation described were never real functions.

### `layout_leaf_only` — SHIPPED

**File:** [`src/storage/entry.rs:296`](src/storage/entry.rs).

The leaf-only layout method is implemented and exercised through the test harness and the pipeline path for pure-leaf objects.

### Move `RenderEntry<P>::clear_needs_paint` / `clear_needs_layout` to `RenderState<P>` — DONE

**File:** [`src/storage/entry.rs`](src/storage/entry.rs).

The forwarding wrappers left over from the previous lock-based API are deleted; every call site clears the flags through `entry.state().clear_needs_*()` directly, so the only API surface is `RenderState`.

### Criterion benchmarks for Mythos Step 14 (deferred -- needs workload generator)

**Files:** new `crates/flui-rendering/benches/frame_throughput.rs`.

**Goal:** Mythos Step 14 prescribed profiling a 1000-node and a 10,000-node frame to verify (a) no `Arc::clone` in the paint loop, (b) cache layout of `RenderEntry<P>`, (c) regressions vs pre-refactor numbers. Today the static memory-footprint assertions landed in `pipeline/dirty.rs` and `storage/state/tests.rs` (see Mythos Step 14 commit); the runtime benchmarks did not.

**Shape:** add a `benches/frame_throughput.rs` Criterion benchmark that:
- Builds a synthetic render tree of N nodes (parametric, e.g. N ∈ {100, 1000, 10000}).
- Marks the root dirty and runs one full `run_frame`.
- Measures wall-clock time, peak memory, and (with `cargo flamegraph`) hot-loop hot spots.

Criterion is already in `flui-rendering` dev-dependencies. The bench harness needs a workload generator (`fn build_flex_tree(depth: u32, children: u32) -> ...`) that produces realistic structures from the `flui-objects` catalog.

**Why deferred:** the workload generator + benchmark is its own scope of work and is best landed when there are real performance questions to answer (a frame is dropping, a particular operation feels slow, etc.). Premature optimisation guidance landed without observed evidence wastes effort.

**Dependencies:** none beyond existing dev-deps.

### Loom test coverage (deferred — miri and proptest already shipped)

**Files:** new `crates/flui-rendering/tests/loom_handle.rs`.

**Note:** `proptest` is already a dev-dependency and is used in `src/virtualization/tests.rs`. The miri half is LANDED: CI's advisory `miri` job runs `cargo +nightly miri test -p flui-rendering --lib pipeline::owner`, interpreting every unit test under `pipeline::owner` — the raw-pointer `SubtreeArena` substrate and the disjoint-borrow layout walks over it. Widening the filter to `storage::tree`'s own `get_two_mut` / `get_parent_and_children_mut` unit tests remains open alongside loom. The remaining deferred test class:

- **Loom tests** for `AtomicRenderFlags` set/clear/read interleaving + private dirty-channel send/recv sequencing across attachment epochs. Needs the `loom` crate gated on `#[cfg(loom)]`.

**Shape:** a new file under `crates/flui-rendering/tests/` plus a dev-dependency.

**Dependencies:** none beyond crate dev-deps.

### Migrate `docs/` companion architecture docs onto template-adjacent shape — DONE

**File:** [`docs/PROTOCOL_ARCHITECTURE.md`](docs/PROTOCOL_ARCHITECTURE.md), [`docs/LAYOUT_SYSTEM.md`](docs/LAYOUT_SYSTEM.md), [`docs/PAINT_SYSTEM.md`](docs/PAINT_SYSTEM.md), [`docs/HIT_TEST_SYSTEM.md`](docs/HIT_TEST_SYSTEM.md), [`docs/ROADMAP.md`](docs/ROADMAP.md).

These deep-dives stay as companion documents (not under the per-crate template directly); each now opens with a "See also" header line pointing back to this file, linking them into the methodology index.

---

## Notes

- **R12 lint promotion path is symbolic for Trigger 1.** [`docs/PORT.md`](../../docs/PORT.md) reactive-lint-promotion rule names `[workspace.lints.clippy]` as the first-promotion mechanism. The clippy lint vocabulary cannot today express "field of type `RwLock<X>` where `X` is a trait object locked in method `foo`". The grep regression in [`scripts/port-check.sh`](../../scripts/port-check.sh) is the durable enforcement layer; the clippy-promotion column waits for ecosystem expressivity (`dylint` plugin or a future clippy feature).
