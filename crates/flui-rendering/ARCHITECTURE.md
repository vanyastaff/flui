# flui-rendering Architecture

This document is the per-crate architecture record for `flui-rendering`. It records the module map for this crate, the design decisions taken so far, the current thread-safety surface, the known friction not yet refactored, and the planned cleanups still to pick up.

Deeper write-ups for layout and hit-testing, and the render test harness guide, live alongside this file under [`docs/`](docs/).

---

## Module map

| Area | FLUI module | Notes |
|---|---|---|
| Render object storage and state | [`src/storage/entry.rs`](src/storage/entry.rs), [`src/storage/state/mod.rs`](src/storage/state/mod.rs), [`src/storage/flags.rs`](src/storage/flags.rs), [`src/traits/render_object.rs`](src/traits/render_object.rs) | The render-object base is split: trait surface in `traits/render_object.rs`, owned storage in `storage/entry.rs`, mutable per-frame state in `storage/state.rs`, atomic flags in `storage/flags.rs`. Parent linkage is in [`src/storage/links.rs`](src/storage/links.rs). |
| Pipeline | [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs) | `PipelineOwner` holds the root node and dirty lists. Single-threaded phase serialisation: `run_layout` / `run_compositing` / `run_paint` / `run_semantics`, each living on the matching `PipelineOwner<Phase>` impl block (typestate-enforced ordering). The `debug_doing_layout` / `debug_doing_paint` flags on the owner are a debug-build cross-check; the type system is the load-bearing enforcement. |
| Box protocol | [`src/protocol/box_protocol.rs`](src/protocol/box_protocol.rs), [`src/parent_data/box_parent_data.rs`](src/parent_data/box_parent_data.rs) | `BoxConstraints`, `BoxParentData`, `Size`-based geometry. |
| Sliver protocol | [`src/protocol/sliver_protocol.rs`](src/protocol/sliver_protocol.rs), [`src/parent_data/sliver_parent_data.rs`](src/parent_data/sliver_parent_data.rs) | Sliver protocol for scrollable layout. |
| Child storage | [`src/storage/links.rs`](src/storage/links.rs), [`src/parent_data/container_mixin.rs`](src/parent_data/container_mixin.rs) | Single-child + variable-children storage; the parent stores `Vec<RenderId>`. |
| Concrete render objects | [`flui-objects`](../flui-objects/src/) crate | `Padding`, `Center`, `ColoredBox`, `Flex`, `Opacity`, `SizedBox`, `Transform`, …; this crate keeps only the protocol/pipeline machinery. |
| Compositing layers | `flui-layer` crate | A sibling crate per the layered DAG ([`docs/architecture.md`](../../docs/architecture.md)). |

Render-subtree relocation is deliberately narrow: only `PipelineOwner<Idle>` can detach, attach, or release a
batch. Detach returns an opaque, non-cloneable `DetachedRenderSubtrees` token
bound to the originating owner by private `Rc` identity. Reattach and
finalization release consume that token, returning it inside a typed failure
after mutation-free preflight when owner, epoch, live-node, or topology checks
fail. Finalization release does not delete nodes; it authorizes the existing
deepest-first element unmount so view lifecycle hooks remain canonical.

---

## Mapping decisions

This section records design decisions and why they were taken. Each entry follows the "Accepted trade-offs" format established by [`docs/plans/2026-03-31-custom-render-callback-design.md`](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/docs/plans/2026-03-31-custom-render-callback-design.md): state the rule (or absence of rule), the choice, the alternatives considered, the trade-off accepted.

### Canvas clips belong to one fragment run

Painting a child may replace the parent's canvas, so the boundary is
deterministic: `PaintCx::canvas` state belongs to the current run; `paint_child` seals it and
later drawing starts with a fresh canvas. Effects covering children use the
`with_clip_*` layer scopes.

The composer brackets each nonempty run with save/restore when merging pictures.
Raw command concatenation is insufficient: a clip recorded without an explicit
save remains active during replay and would incorrectly clip the child and the
parent's next run. The cost is two replay commands per nonempty run, while inline
children still share one picture. The facade readback test
`canvas_clip_stays_in_its_run_when_paint_child_splits_the_picture` checks the parent
clip, an unclipped child, and the resumed parent run through a real frame and GPU.

### The set POSITION is published; the set size waits, and the delegates do not wrap

**Rule:** a screen reader announces "item 12 of 100" from the platform's set-position concept.
The two halves are the item's index in its parent and the scrollable's child count; a bridge that
carried them separately would leave each platform to reconcile them, and wrapping every
materialised item in an `IndexedSemantics` widget costs a node per item.

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

**Placement.** An `IndexedSemantics` goes *inside* the row's semantics
container, not outside it: a non-boundary configuration is absorbed by its nearest ANCESTOR
boundary in this assembler, so a wrapper above the container would index the node that forms
above the row — the sliver — instead of the row.

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

**Unasserted:** no test pins this. A test for it asserts on the published AccessKit nodes rather
than the framework configuration, since the near end proves nothing about what a reader receives,
and covers three things: the set size lands on the item nodes and only on them, a lazy row carrying
no wrapper still publishes its derived position, and an explicit index wins over the stamped one
(declared indices the reverse of the stamped ones, so either wrong precedence fails).

`harness_indexed_semantics_reports_its_index_and_only_republishes_on_change`
(`crates/flui-objects/tests/render_object_harness.rs`) pins the render object alone, including
that an unchanged index requests no semantics update.
### A layout stamps the children it laid out; paint and hit-test skip the rest

**Rule:** a multi-child render object that lays out a *subset* of its children — a lazy sliver's
band, anything virtualised — leaves the rest holding an offset and a size from an earlier pass.
Each object could keep the discipline itself by tracking its own child list and painting only
what it laid out.

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
worse trade: an off-screen or kept-alive child should publish no semantics at all. Announcing one
at a rect from a pass that no longer holds sends a screen reader somewhere with nothing on it.

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

**Unasserted:** no test pins this. That covers the paint gate, the hit-test gate and the capture
eviction; the semantics gate is pinned by
`harness_placed_generation_gate_excludes_a_dropped_child_from_semantics`
(`crates/flui-objects/tests/render_object_harness.rs`). A test for any of the three needs **two
frames**: a child never laid out at all has size zero and paints nothing
regardless, so a single-frame version passes with the gate removed. `FrameRun::run_frame_again`
exists for it.

### A composited-layer update patches the enclosing capture; no node is promoted to a boundary

**Choice:** giving the render object its own layer to mutate in place would
require the object to BE a repaint boundary and require ref-counting plus
explicit disposal to manage that layer's lifetime.

FLUI does neither. `RetainedSubtree` is a flat `Vec<RetainedNode>`, so a node's
own effect layers are addressable by index inside the ENCLOSING boundary's
capture (`effect_slots`). An alpha change rebuilds those entries through the
same `own_effect_layers` constructor the paint walk uses, patches them into the
grafted output, and writes them back into the stored capture — with the node
still an ordinary non-boundary.

**Alternatives:** promoting the node to its own repaint boundary — rejected on two grounds, both
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
   **Unasserted:** no test pins this.
3. **The root boundary is never retained, so an effect whose only enclosing
   boundary is the root gets no fast path.** `run_paint` enters through
   `paint_subtree(root)` directly rather than the boundary-child arm that
   creates captures, so `RenderView` — which declares itself a boundary — has
   none. The mark still degrades correctly (the frame repaints), it is simply
   not accelerated. A tree with any `RepaintBoundary` above the effect, which
   includes every per-item boundary in a list, is unaffected.

**Replacement tests:** `an_alpha_change_updates_the_layer_without_repainting_the_subtree`
and `a_failed_pass_does_not_downgrade_a_real_repaint_to_an_update`
(`tests/retained_boundary_layers.rs`), plus pixel equivalence against a forced
repaint in the facade's `tests/composited_layer_update_readback.rs`. The
write-back into the stored capture (a test for it needs three frames — a
two-frame version cannot see a missing write-back), the repaint fallback for a
structural alpha change, and a same-frame repaint winning over a layer update
outside a failed pass: **Unasserted:** no test pins this.

**The static sliver opacity.** An alpha change is served as a layer update
on every opacity node: `RenderOpacity`, the animated opacity nodes on BOTH
protocols (`animated_opacity.rs` and `sliver_animated_opacity.rs` already mark
layer updates) and the static `RenderSliverOpacity`. The static sliver node
declares `alwaysNeedsCompositing` but is never a repaint boundary, so a setter
that only marks paint would cost a full subtree repaint on every alpha tick.

`RenderSliverOpacity::set_opacity` reports `COMPOSITED_LAYER_UPDATE` instead.
Promoting the node to a boundary is exactly what this design removed: a flat
capture makes any node's effect layers addressable inside the ENCLOSING
boundary, so the sliver needs no boundary of its own to be served.

The structural transitions still repaint (`skip_paint` crossings and
compositing-threshold crossings), so those edge cases are handled by
repainting deliberately. **Unasserted:** no test pins this.

A test over the Sliver protocol would be the first coverage of the update-only
path there at all. What that buys is narrower than "the machinery is
protocol-agnostic" and worth stating exactly: `RenderNode::paint_effects()`
(`crates/flui-rendering/src/storage/node.rs`) resolves `size` per protocol
(box → committed `geometry().unwrap_or(Size::ZERO)`, sliver →
`absolute_paint_size()`) and calls the render object's `paint_effects(size)`
the same way in both arms, so every field of the value — opacity included —
now rides the one protocol split, not a dispatch specific to alpha. What such
a test pins is that a sliver's `RenderSliver::is_repaint_boundary` is honoured
by the layer-update walk — without it the walk reaches the viewport paint root
and degrades to a plain repaint, which is how it fails when the setter is
reverted. The one place the protocols genuinely diverge is the
`size` argument `paint_effects` is called with (`geometry()` for a box,
`absolute_paint_size()` for a sliver); `RenderSliverOpacity`'s `paint_effects`
never sets `transform`, so the transform arm of `own_effect_layers` stays
unexercised for slivers.

**Known gap:** neither opacity render object has an `always_include_semantics`
field. Both report semantics unconditionally on a visibility flip.

**`RenderTransform`.** A transform change is also served as a layer update, so
a `ScaleTransition` or `RotationTransition` does not repaint its subtree on
every animation frame.

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
   compositing layer per frame.

**Tests:** pixel equivalence against a forced repaint,
`the_transform_update_path_and_a_repaint_produce_the_same_pixels`
(`tests/composited_layer_update_readback.rs`). The layer-tree side — the
update skipping the subtree repaint, its write-back into the stored capture, a
patched subtree matching a full repaint at every benchmark size, and the
same-frame layout invariant the next entry states: **Unasserted:** no test pins
this.

**`RenderRotatedBox`.** A setter that compared the RAW value and relaid out
on any inequality would cost a full relayout and repaint for two turns that
merely differ — including two that share parity, like `0` and `2`.

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
parity) — never the exact value. Nothing in `quarter_turns: i32` stops a
future method from branching on the exact turn instead. **Unasserted:** no
test pins this. A same-parity quadrant
change therefore never re-enters `perform_layout` for the rotated box itself
— the setter reports `COMPOSITED_LAYER_UPDATE`, not `LAYOUT`, so the pipeline
never calls it — which answers "did the rotated box's OWN layout move its
origin" trivially: no, it did not run. Second, "did something ELSE inside the
boundary move it" is the general invariant the next entry below states for
`RenderTransform`: any same-frame layout change anywhere inside a boundary marks that boundary
needing paint, upgrading a queued `LayerUpdate` to a `Repaint`
(`PaintQueue::enqueue` never downgrades) — so a sibling or ancestor that moved
the rotated box's accumulated position forces a repaint, which recomputes the
conjugation from a live origin. `RenderNode::paint_effects` hands the value
the COMMITTED `geometry()` either way (`storage/node.rs`), and the matrix's
third input, `child_size`, is a field
`perform_layout` caches — the one input `RenderTransform` does not have — so
it cannot move without a relayout either. Neither size argument is in
question; only the origin is.

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
comes out correct through the refusal: the flag is cleared by the fallback
repaint, so a later mark is not self-refused. **Unasserted:** no test pins
this.) And a
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

**Unasserted:** no test pins this. That covers the same-parity
update patching the layer and writing it back, a parity change relaying out,
the childless fallback above, and the setter's impact table;
`harness_rotated_box_odd_turns_swaps_axes`
(`crates/flui-objects/tests/render_object_harness.rs`) pins only the size
swap of an odd turn at mount.

**The clip family: `RenderClip` and `RenderFlow`.** The design is not specific
to opacity, transform, or rotated boxes; it applies to any node's own effects:

> Serving a layer-property change without a repaint would normally require a
> node that IS a repaint boundary, owning its own layer, at the cost of an
> `OffsetLayer` and a retained capture per node. FLUI reads one
> `PaintEffects` value from any node and builds its layers inside the
> ENCLOSING boundary's flat capture through one constructor with two callers,
> so every effect expressed through the value is patchable with no promotion,
> no layer ownership, and no per-class override; the value fixes the nesting
> (opacity outermost, transform innermost, node-local shapes between), which
> is what lets a single per-position layer-kind compare stand as the whole
> structure guard.

Concretely, for clips: a border radius, a clip shape, a path
source token, or `clipBehavior` itself changes as a layer patch inside the
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
  (`crates/flui-engine/src/layer_render.rs`) — so crossing `Clip::None` never
  changes the layer count. **Unasserted:** no test pins this. A token-driven path clip is reported as
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

**Replacement tests:** the pixel oracle,
`the_clip_update_path_and_a_repaint_produce_the_same_pixels`
(`tests/composited_layer_update_readback.rs`; `cargo nextest run -p flui
--features gpu-readback-tests --no-default-features --test
composited_layer_update_readback --locked --test-threads 1`). The layer-tree
side — a border-radius change updating the clip layer without repainting the
subtree, two path clips under one boundary resolving in paint order, and a
different radius producing different pixels: **Unasserted:** no test pins
this.

**Deferred producers.** Not served by this change, each for a stated reason:

| Producer | Why not now | Trigger | Shape when reopened |
|---|---|---|---|
| `RenderBackdropFilter` filter/blend (`enabled` → `PAINT`) | no widget → no caller | the `BackdropFilter` widget (driver: a `FlexibleSpaceBar` blur) | `backdrop_filter:` field at a fixed position (alpha does not commute — position decided at the field) |
| `RenderShaderMask` | no widget; its shader is lane-resolved like a path clipper | the `ShaderMask` widget | `shader_mask:` field with a walk-resolved target, as `PathTarget` |
| `ImageFiltered` | no render object | its port (driver: a stretching overscroll indicator) | a field; `enabled` → `PAINT` |
| `RenderPhysicalModel`/`RenderPhysicalShape` | shadow + fill are display-list content, and a descriptor clip wraps own draws | `Material` implicit elevation/shape animation (`_MaterialInterior`) | seal a drawing scope-owner's own draw runs into a picture of their own and re-record only that picture on a property change; measure the layered shape first; needs an ADR |
| `RenderFlow` per-child transforms | variable fragment | an animated `Flow` delegate in the catalog | re-record the flow's own fragment (a per-node picture re-record, not a layer patch); the `paint_effects` clip stays the prefix outside it |
| `RenderFittedBox` | NOT nesting (the fixed order is exactly its clip-outside-transform) but its translation fast path (`paint_child_at`, a `Some ↔ None` transform transition) and its overflow-gated layout-derived clip | a per-frame fit animation in the catalog | a per-node fragment re-record for the child offset; the overflow clip stays structural |
| Overflow-gated clips (`RenderStack`/`RenderIndexedStack`, `RenderWrap`, `RenderConstraintsTransformBox`, `RenderAnimatedSize`, box `RenderViewport`/`RenderShrinkWrappingViewport`; rule: `has_visual_overflow && clip_behavior != Clip::None`, rect = own size — enumerate with the `rg` command in trade-off 1 above) | no patchable property | none; re-check the enumeration when one gains a property | n/a |
| `ClipSuperellipse` | no render object, widget, or variant (its layer debug-asserts against `Clip::None`, unlike rect/rrect/path, which only skip the push) | its port | a `PaintClip` variant; note the per-layer `Clip::None` difference |
| `blend` on `PaintOpacity` | zero producers | the first advanced-blend producer | additive field on the `#[non_exhaustive]` struct |

The market survey behind this shape ([`AGENTS.md`](../../AGENTS.md) Design stance, "Look around before settling") covered
Compose, SwiftUI, Slint, GPUI, and Masonry/Xilem; Compose's
[`Modifier.graphicsLayer`](https://developer.android.com/reference/kotlin/androidx/compose/ui/graphics/graphicsLayer.modifier)
is the closest market analogue — one declarative value per layer (`alpha`,
`clip` + `shape`, the transform fields, `renderEffect`, `blendMode`) whose
property-only changes update the retained layer without re-recording the
display list, the same contract `PaintEffects` gives FLUI.

### `RenderRotatedBox` reports a baseline only for an even turn

**Choice:** a baseline is a layout line. An even turn keeps the box's size and
its horizontal axis, so the box takes part in baseline alignment exactly as
its unrotated self would — the child's baseline is forwarded unchanged and the
glyphs flip in place at turn 2. That is the model every draw-time rotation
uses: `RenderTransform` here (a proxy, via
`forward_single_child_box_queries!`), Compose's `Modifier.rotate`, SwiftUI's
`.rotationEffect`. An odd turn rotates the baseline axis into the vertical, so
there is no horizontal baseline to offer and the box is treated like any child
without one. A baseline-aligned `Row` therefore places an even-turn box where
its child would sit and an odd-turn box at the cross start; the dry query
answers the same way rather than asserting.

Both halves of the query answer from the same predicate: the dry half in
`compute_dry_baseline`, and the live half through
`forwards_baseline_to_only_child`, which returns `!is_vertical()` so the
layout driver walks to the child as it does for a pure proxy. The two must
agree, and the live half is the one that matters: the only consumer of a
rotated box's baseline is a baseline-aligned parent's `perform_layout`, which
asks the live query — a dry-only forward is inert there, and a same-parity
equality pin cannot tell either half's value.

**Alternatives:** report no baseline for any turn — loses the identity
case for nothing. Forward only at turn 0, or mirror the line to
`height − baseline` at turn 2 — both read the exact turn rather than its
parity, which breaks the layout-turn-blindness the same-parity fast path above
rests on: `set_quarter_turns` would have to report `LAYOUT` for 0↔2 and the
parity pin would need a baseline exception, to serve a case no framework
animation drives (`RotationTransition` drives `Transform::rotation`; a widget
rebuild 0→2 does occur and is what the layer-update route serves). Rejected
on that cost, not on geometry.

**Unasserted:** no test pins this. A test for it places `RotatedBox(0)` and
`RotatedBox(2)` in a baseline-aligned `Row` where their child would sit and
`RotatedBox(1)` / `RotatedBox(3)` at the cross start, and asserts the dry
answer by value at all four quadrants.

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
dirty-root mark (a once-per-walk paint mark rather than a per-object one)
walks up to it too. Either
route enqueues `Repaint`, which upgrades the `LayerUpdate` entry the setter
queued (`PaintQueue::enqueue` never downgrades). So any same-frame layout
change inside a boundary — anything that could move the node — reaches it one
way or the other, and the patch path is simply unreachable with a stale
origin.

**Accepted trade-off:** this is a coupling between two subsystems that is stated
nowhere in the code they connect. An optimisation that decouples "the boundary
moved" from "the boundary is marked needing paint" — a plausible future change to
that `mark_needs_paint` loop — silently makes transform patches render about the
wrong point. **Unasserted:** no test pins this; a test for it fails with the layer
count and discriminant right and the translation column wrong when the loop body
is replaced with a no-op.

**A patch that panics poisons the frame instead of being refused.**
Unlike a structural shape mismatch — which `layer_patches_for` refuses by
returning `Ok(None)`, falling back to a repaint — a panic while rebuilding a
target's own effect layers (inside `paint_effects`, or the walk's resolution
of a `PaintClip::PathTarget` it reports) cannot be refused the same way: the
fallback repaint would call that same panicking `paint_effects` on the same
node and panic a second time. So it poisons instead
(`RenderError::Poisoned { phase: PoisonPhase::LayerUpdate, .. }`), the frame
is discarded whole, and the queued update survives on the node for the retry.

The captured-origin invariant above holds through a clip patch as well as a
transform one. **Unasserted:** no test pins this. The two differ in one
respect worth being precise about: a
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

**Choice:** nothing to do. The criterion assumes a layer that owns a GPU
resource, which would need handles, ref-counting and explicit disposal. FLUI's `Layer`
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

### Paint certifies a boundary's content token

**Rule:** a repaint boundary's stamped layer carries a `flui_layer::ContentToken`
next to its `RenderId`, and the damage differ (`flui_layer::LayerDiffer`,
ADR-0087 §3) treats an unchanged token as "this boundary's own pixels did not
change". Something has to vouch for that.

**Choice:** the paint walk does, with the same fact the graft already rests on
(absence from the paint queue means unchanged content). At each boundary child,
before its layer is pushed, the boundary keeps the token of its retained
capture when it is absent from the FULL queue (a composited-layer update counts,
since a patched graft changes pixels) and has a capture; otherwise it mints a
new token. The capture stores its token (`RetainedSubtree::content`), so every
eviction path drops the token with it, and a graft that served a layer update
writes the new token back with its patches. The root is never captured, so its
token lives in `PipelineOwner::root_content` under the same rule. Minted tokens
commit with the frame; an errored frame drops them, and the retry, still
queued, mints again. Nested boundaries keep their stamp (token included)
through an enclosing boundary's graft.

**Why not pointer identity of the pictures:** `run_paint` always descends from
the root, and an outer boundary refuses reuse while anything nested in it is
dirty, re-recording its inline pictures. Their `Arc`s change on every such
frame although the content did not, so `Arc::ptr_eq` would report the root
changed on every real frame (`an_outer_boundary_redescended_for_a_nested_repaint_keeps_its_token`
pins the re-recording as well as the kept token).

**Accepted trade-off:** a clean node that paints differently breaks this rule
exactly as it already breaks grafting. A boundary whose capture was refused
(it holds a leader or follower) mints every frame and is always damaged.
Tests: `tests/boundary_content_tokens.rs`.

### Layout marks semantics once per walk, at the dirty root

**Rule:** every node that lays out re-publishes its semantics geometry, which is
what makes a scroll update the accessibility tree at all: a viewport's offset listener requests
layout and nothing else.

**Choice:** the same guarantee, marked **once per layout walk on the dirty root**
(`layout_dirty_root`) rather than once per laid-out node.

**Alternatives considered:** recording every laid-out node in the arena and marking each, which
is the literal reading — rejected on two counts. It is redundant: the arena walks the
subtree of the dirty root, so every node that laid out is already under it, and `try_graft_pass`
re-assembles a marked node's whole subtree. And it is expensive in a way that is easy to miss:
each `add_node_needing_semantics` fires `fire_need_visual_update`, whose production callback asks
the platform to redraw, and the graft resolves every marked node by walking its ancestor chain —
so an N-node relayout would cost N redraw requests and O(N·depth) graft work.

**Trade-off accepted:** one unconditional call per layout walk. `mark_needs_semantics` is a
no-op while semantics is disabled, so a session with no accessibility client attached pays one
predictable branch. A relayout of a subtree re-assembles that subtree even where a node's own
geometry did not move — the graft's granularity is the anchor, not the node, which is the same
bargain the existing graft already makes.

**Unasserted:** no test pins this. A test for it scrolls a viewport whose rows are *all* inside
the cache band, so the frame materialises nothing new, and asserts on a build counter that no row
rebuilt — without that assertion it measures a newly-built row's own semantics mark and passes
with the change reverted.

### The hit-test path is driver-owned; the protocol carries no result accumulator

**Choice:** `HitTestCapability::Result` and `::Entry` are vocabulary only. There is no
`ctx.result()`, `result_mut()`, `add_hit(entry)` or `add_self(id)`: the driver
(`PipelineOwner`'s hit-test walk) owns the path and builds each entry from the node's own
`RenderId`. A render object says it was hit by returning `true`, or calls
`ctx.register_self_hit_entry()` to appear in the path without blocking what is behind it.

**The alternative shape:** a result accumulator passed to `hit_test`, where each render object
adds its own entry.

**Why the driver-owned path is better, in checkable terms:**

1. **A render object cannot get the id wrong**, because it never supplies one. An API that takes
   the node as an argument makes passing the wrong one, or adding twice, expressible and silent.
2. **There is one writer, not N.** The driver knows the node, its transform and its position in
   the walk, so the entry is assembled once from state that cannot disagree with itself. The
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

**Replacement coverage:** `register_self_hit_entry` is exercised end-to-end by
`mouse_region_cursor_reaches_the_window_through_the_realm` (`crates/flui-testing/tests/realm_driver.rs`):
`RenderMouseRegion` enters the hit path only through it, and the test asserts the hovered region's
cursor reaches the window. The deleted tests asserted a write landed in a structure nobody read, so
they were removed rather than adapted: they could not fail for a reason a user would notice.

### Lazy-sliver scroll correction keeps the first visible item stationary

**Rule:** [ADR-0051](../../docs/adr/ADR-0051-anchor-stationary-scroll-correction.md).

**Choice:** `Virtualizer::set_measured` / `adapt_default_estimate` report the offset delta of the anchor (the first visible item) whenever an extent above it changes; the consumer sliver accumulates the deltas and emits them as `SliverGeometry::scroll_offset_correction` at the end of the pass, in either scroll direction. The viewport applies the correction and re-runs layout in the same pass, so the anchor never moves on screen.

**Alternatives:** retaining each resident child's stale layout offset, walking forward from the first retained child with current sizes, and correcting only at a boundary — growth of a retained-but-invisible child then shifts visible content. ADR-0003's original consumer note additionally withheld corrections during a backward scroll; measured on the pinned scene it changed nothing and, where it can act, it is a one-frame anchor drift.

**Accepted trade-off:** the pinned 'inaccurate scroll offset' windows differ from a boundary-only correction by exactly the growth it shows as a jump (192 px in that scene); the pinned test stays `#[ignore]`d as the statement of the declined behaviour and a FLUI test stands beside it. Items above a jump that were never resident stay hinted until they enter the band (O(band) layout, ADR-0003), where an O(distance) walk would be exact.

### Render-tree storage uses a `Slab<RenderNode>` with `RenderId` (NonZeroUsize) keys

**Rule:** strategy clause "Behavior as floor, everything else designed for Rust"; constitution Anti-Patterns list ("`Arc<Mutex<>>` for tree structures — use arena/slotmap"); the ID-offset pattern documented in [`docs/architecture.md`](../../docs/architecture.md).

**Choice:** `RenderTree` stores `Slab<RenderNode>`. `RenderId` is a `NonZeroUsize` newtype that adds `+1` to the slab index, so `Option<RenderId>` niche-optimises to 8 bytes for parent / child references. The slab is reached from one strong root (`PipelineOwner::root_id`) and every other node is reached by walking child IDs in `NodeLinks`.

**Alternatives:** a graph of references with direct child pointers on every render object would require `Arc<RwLock<RenderObject>>` or `Rc<RefCell<RenderObject>>` for parent/child cycles, which the constitution forbids for tree structures. `typed-arena::Arena` was considered but cannot delete individual entries, which the element reconciler needs.

**Accepted trade-off:** one extra indirection (slab lookup) on the tree-walk hot path, paid back by O(1) insert/delete, deterministic ID stability across mutations, and elimination of `Arc<Mutex<>>` cycles. The same pattern is used by `flui-view`'s `ElementTree`.

### `RenderEntry<P>` owns the render object by value (no lock, no interior mutability)

**Rule:** strategy clause "sync hot path, async на краях" (lock contention on the hot path is functionally async-flavoured); no lock on per-node render storage touched during `perform_layout` / `paint` (no `RwLock<Box<dyn RenderObject<P>>>`).

**Choice:** `RenderEntry<P>::render_object` is a plain `Box<dyn RenderObject<P>>` (see [`src/storage/entry.rs`](src/storage/entry.rs)). Mutable access goes through `&mut self`, which the pipeline obtains via `PipelineOwner::render_tree_mut() -> &mut RenderTree` at phase boundaries. Re-entrant access from a parent to a child during layout uses disjoint-borrow primitives on `RenderTree` (`get_two_mut`, `get_many_mut`; the underlying `unsafe` is local and disjoint-keys-invariant — see [Thread safety](#thread-safety)). Debug-build re-entrancy checks live in `PipelineOwner::debug_doing_layout` / `debug_doing_paint` (see [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs)).

**Alternatives considered:**
- `OnceCell<Box<dyn>>` — rejected. `OnceCell::get()` returns `&T`; the trait still has `&mut self` methods that need mutation, so the lock would have to come back under another name.
- Arity-keyed enum dispatch — rejected. The trait is open-set via the blanket `impl<T: RenderBox + Diagnosticable> RenderObject<P> for T` (see [`src/traits/render_box.rs`](src/traits/render_box.rs)). Closing it to a known enum would force every user-defined render object into a derive-macro discipline and break the widget extensibility story.
- `RenderObjectId` indirection (render object lives in a separate slab keyed by ID) — considered. Adds one extra indirection per access and doubles the lifecycle invariants (insert/delete across two slabs). Equivalent soundness-wise but more moving parts than necessary.
- Inner-mutability split (immutable `Arc<dyn>` config + all mutation moved to `RenderState`) — considered. Largest API change of all the options; would force every concrete render object in `flui-objects` to be refactored. Filed as future work.

**Accepted trade-off:** the layout and update paths must hold `&mut RenderTree` for the duration of the phase. Multi-child layout requires the `get_many_mut` primitive. The borrow checker, not a lock, enforces single-writer-per-frame — single-threaded with debug asserts, in place of the previous `RwLock`-based shape.

### `set_was_repaint_boundary` removed from the trait surface; bit lives on `RenderState::flags`

**Rule:** no lock on per-node render storage touched during paint (the previous shape required a write lock on the trait object during paint to flip a single bool); strategy clause "Compile-time over runtime" (state bits belong on the bookkeeping layer, not the user-implementable trait surface).

**Choice:** added `RenderFlags::WAS_REPAINT_BOUNDARY` (bit 10 — see [`src/storage/flags.rs`](src/storage/flags.rs)) with `RenderState<P>::set_was_repaint_boundary` / `was_repaint_boundary` accessors. The paint phase at [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs) (`paint_subtree`) writes the bit through an atomic store on `state().flags()` rather than locking the trait object. The trait method `RenderObject::set_was_repaint_boundary` is deleted (see [`src/traits/render_object.rs`](src/traits/render_object.rs)).

**Alternatives:** keep the trait method and live with the per-paint write lock — rejected, this is the canonical refusal-trigger violation. Move the bit to a per-tree side table — rejected, would add a second source-of-truth for state already structured around `RenderState<P>`.

**Accepted trade-off:** subclasses that wanted to override `set_was_repaint_boundary` (none currently do) lose the hook. The flag's owner is now framework code, not user code.
### `unsafe impl Send + Sync for RenderTree` removed

**Rule:** constitution Principle III ("zero unsafe in widget/app layer; `unsafe` only in `flui-platform`, `flui-painting`, `flui-engine`"); the prior `unsafe impl` was a soundness carve-out documented in [`docs/plans/2026-03-31-core-crates-hardening.md`](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/docs/plans/2026-03-31-core-crates-hardening.md) Task 7.

**Choice:** removed the `unsafe impl Send for RenderTree {}` / `unsafe impl Sync for RenderTree {}` block at the bottom of [`src/storage/tree.rs`](src/storage/tree.rs). The transitive Send+Sync chain still holds via auto-derivation: `Slab<RenderNode>` is auto-`Send + Sync` because `RenderNode` is; `RenderEntry<P>` holds `Box<dyn RenderObject<P>>` and the trait requires `Send + Sync + 'static`; `RenderState<P>` is built on atomics and `Option<T>` fields for geometry/constraints; `NodeLinks` is POD.

**Alternatives:** keep the unsafe impl as defensive cruft — rejected, the safety justification was load-bearing only because of `RwLock`'s interior mutability; with that gone, no unsafe carve-out is needed.

**Accepted trade-off:** net unsafe deletion, one fewer place where the carry-cost of a soundness comment exists.

### Third-party trait calls wrapped in `catch_unwind`; phases return `RenderResult<()>`

**Rule:** design verdict Section 7 ("Partial failure recovery: A render object that panics inside `perform_layout` or `paint` poisons that node only. The pipeline catches via `std::panic::catch_unwind`, marks the node as `RenderError::Poisoned`, drops the in-flight frame, and lets the caller decide.") and Section 10 (the `Poisoned { render_object, phase }` error variant).

**Choice:** every third-party trait call site has its call wrapped in `std::panic::catch_unwind(AssertUnwindSafe(|| ...))`. A panicking render object surfaces as `RenderError::Poisoned { render_object, phase }` rather than aborting the process. Specifically:

- `RenderEntry::layout` ([`src/storage/entry.rs`](src/storage/entry.rs)) wraps `render_object.perform_layout_raw(...)` and returns `RenderResult<ProtocolGeometry<P>>`. On the panic path, state is left untouched (`NEEDS_LAYOUT` stays set) so the next frame can retry. The retry is not unbounded: the pipeline counts consecutive layout failures per node and poisons nodes that fail structurally or exhaust the budget ([`src/pipeline/owner/poison.rs`](src/pipeline/owner/poison.rs)); a poisoned node is skipped in later walks until `mark_needs_layout` freshly invalidates it. What a reader can tell apart afterwards, and where: a poisoned node that once committed keeps that geometry — `RenderNode::geometry_box()` is `Some(last committed)` — while one that never succeeded reads `None` and is served as `Size::ZERO`; the parent that consumed the stand-in carries `RenderNode::geometry_degraded()`; and the node's own layout-call counter shows the poison skipped the re-attempt. Pinned by `a_poisoned_leaf_stands_in_with_its_last_committed_size_not_zero` (`tests/layout_poison.rs`), whose doc states the two mutations that turn it red. The never-committed half (`None`, served as `Size::ZERO`): **Unasserted:** no test pins this.
- `PipelineOwner::<PaintPhase>::paint_subtree_impl` ([`src/pipeline/owner/paint.rs`](src/pipeline/owner/paint.rs)) wraps `render_node.paint_raw(&mut recorder, ...)` (the fragment recorder) together with the node's own effect-layer build, `own_effect_layers(render_node.paint_effects(), origin)`, in ONE `catch_unwind`. Both run only after the walk's three return gates (`skip_paint`, `needs_layout`, the sliver visibility cull): a gated-out node never builds a `PaintEffects` descriptor it will not use. Building it there matters: reached outside those gates, a node whose `needs_layout` flag is still set carries stale geometry — its last committed size, or `Size::ZERO` if it has never been laid out — so its descriptor would be built against a size this pass never computed. A panic in either half — the node's own `paint_effects`, or the walk's resolution of a `PaintClip::PathTarget` inside `own_effect_layers`'s clip arm — surfaces as `Poisoned { phase: PoisonPhase::Paint, .. }`, one poison point per node rather than two.
- `PipelineOwner::<PaintPhase>::layer_patches_for` ([`src/pipeline/owner/paint.rs`](src/pipeline/owner/paint.rs)), the composited-layer-update patch arm, wraps the same `own_effect_layers(node.paint_effects(), slots.origin)` rebuild — run once per target being patched into a retained boundary's capture — in its own `catch_unwind`. A panic surfaces as `Poisoned { phase: PoisonPhase::LayerUpdate, .. }` and the function returns `Err` instead of `Ok(None)`: the whole frame is discarded and the queued composited-layer-update request survives on the node for the retry. Falling back to a repaint instead (the way a structural shape mismatch does, via `Ok(None)`) was rejected — the repaint would call the same panicking `paint_effects` on the same node and panic a second time.

The phase entry points (`run_layout` / `run_compositing` / `run_paint` / `run_semantics`) now return `RenderResult<()>`. `run_frame` returns `(PipelineOwner<Idle>, RenderResult<Option<LayerTree>>)` -- the owner **always** comes back at Idle so frame-loop callers can mutex-replace through it on both success and error paths.

`Poisoned`'s `phase` is a [`PoisonPhase`](src/error.rs) (`Layout` | `Paint` | `LayerUpdate`, rendered `"layout"` / `"paint"` / `"layer-update"` by its `Display` impl) — the enum itself is the authoritative, closed set; nothing today produces a fourth variant.

Isolation is deliberately strict: a panic in `paint_effects` (`PoisonPhase::Paint`) or in `layer_patches_for`'s rebuild (`PoisonPhase::LayerUpdate`) discards the WHOLE frame as `Poisoned` rather than isolating the one node — the dirty queue survives, so the node is retried next frame, but nothing from the poisoned frame reaches the compositor. The trade-off: no partially painted frame is ever presented, at the cost that a node stuck panicking blocks the whole frame from completing until it is replaced or stops panicking.

`RenderObject<P>::debug_name(&self) -> &'static str` is the static identifier embedded in `RenderError::Poisoned`. Its default body monomorphizes per concrete impl via `core::any::type_name::<Self>()`; calling through `&dyn RenderObject<P>` yields the concrete type name because the vtable carries the monomorphized stub.

**Alternatives:**

- **Process-wide `panic::set_hook`** -- rejected, leaks pipeline concerns into global process state and can't differentiate phase-of-origin.
- **Cache `debug_name` on `RenderEntry<P>` at insertion** -- considered. Would avoid one vtable dispatch per error case. Not adopted because the dispatch happens only on the failure path (cold by definition), and the cache adds a `&'static str` field that pollutes every `RenderEntry<P>` in the common case.
- **Return `(PipelineOwner<Idle>, RenderError)` tuple on error** (shape (a) in the original design) -- rejected, awkward to compose; pattern-matching on `(_, Result<_>)` is cleaner than splitting the success and error tuples.

**Accepted trade-off:** `AssertUnwindSafe` is documented inline at each wrapper. The render object's internal state may be torn after a panic; the pipeline treats the node as poisoned and lets the caller drop or replace it. Process-level safety is preserved; the render tree itself is not corrupted.

**Note:** `hit_test_raw` is part of the `RenderObject<P>` trait, but the current pipeline owner does not invoke it directly -- hit testing is dispatched at the `RenderView` layer outside the frame pipeline. The catch_unwind helper around hit_test will land when hit testing is wired through the pipeline.

### Phase ORDERING lives in the type system; one runtime COMPLETENESS gate remains

**Rule:** phase discipline enforced only by runtime debug asserts and driver convention would let a driver call the paint flush with layout work still queued; the invariant belongs in types.

**Choice:** FLUI lifts ORDERING into the type system entirely. Each `run_*` method lives only on its phase's impl block (`PipelineOwner<Layout>::run_layout`, `PipelineOwner<PaintPhase>::run_paint`, …) and the phase transitions are by-value `rebind_phase` moves, so `run_paint` cannot even be named on an owner that has not come back from the layout phase — an out-of-order call is error[E0599], not a runtime condition. `PipelineOwner::run_frame` is the only sequencing authority in-tree.

What the type system cannot express is frame COMPLETENESS. A caller may legally drive the phases by hand — `into_layout().into_compositing().into_paint()` — and skip `run_layout`; the manual phase chain with empty queues is designed behavior (the benches drive the transitions this way; the paint-only frame-2 tests and `tests/paint_before_layout.rs` drive the chain all the way to `run_paint`). So `run_paint` keeps ONE runtime gate: on entry, before any paint work is served, `scheduler.has_layout_work()` returns `Err(RenderError::PaintBeforeLayout)`. The gate reads the scheduler's `needs_layout` QUEUE, not the per-node `NEEDS_LAYOUT` flag — a flag-only mark without an owner-side enqueue is the stale-geometry signal the paint walk's own needs-layout skip handles per node (see the catch-unwind entry above), not a frame-level contract breach. The variant is frame-level by design: a paint `Err` aborts the whole frame, and per-node needs-layout gating is the walk's skip rule, not an error.

The remaining phase-misuse variants — `LayoutDuringPaint`, `LayoutDetached`, `PaintDetached`, `PhaseOrderViolation` — are deliberately UNWIRED, each documented in `src/error.rs` with the reason: the typestate machine forecloses its misuse site structurally (a layout-phase value cannot reach paint-phase code; detached subtrees are evicted from the dirty queues at relocation, and a stale id is a walk no-op or `NodeNotFound` downstream), and a variant no path can construct reads as a guard that guards nothing. They stay pre-added — the enum is already fully `#[non_exhaustive]`, so raising one later is not a breaking change — keeping the error surface stable for a future runtime seam (a paint-triggered relayout, a mid-pass detach).

**Alternatives considered:**

- **Hardening `into_compositing` to prove layout ran** (draining or asserting the queue in the transition) — rejected: it refuses the legitimate empty-queue chain the benches rely on, and the transition is a pure `rebind_phase` by design; completeness is the phase entry's obligation, not the transition's.
- **A per-node `PaintBeforeLayout` carrying the node's `RenderId`** — rejected: the paint walk already skips needs-layout nodes (stale-geometry gate) and the residue scan reports unreached nodes; a paint `Err` aborts the whole frame, so a per-node variant shape would suggest recovery granularity the pipeline does not have.

**Accepted trade-off:** the variant name predates the wiring and reads per-node ("paint performed before layout"); its semantics is frame-level, which the variant's Display string ("paint phase began with layout work still pending") and doc state. Renaming the variant would be a breaking change to a public error enum for a cosmetic gain.

### Multi-source design references in this crate

The structural shape of individual components in this crate is designed for Rust and has been informed by multiple Rust-side audited references as recorded in prior plans:

- `slab::Slab` storage pattern with `+1/-1` ID offset — internal precedent in [`src/storage/tree.rs`](src/storage/tree.rs); the offset rationale lives in [`docs/architecture.md`](../../docs/architecture.md).
- `Weak<RwLock<PipelineOwner>>` parent back-reference replacing a raw pointer — [`docs/plans/2026-03-31-core-crates-hardening.md`](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/docs/plans/2026-03-31-core-crates-hardening.md) Task 7.
- Lock-free atomic dirty tracking (`AtomicRenderFlags`); the offset lives in an `OffsetCell` (`Cell<Offset>`, two `f64` components; the tree is `!Send + !Sync`); geometry/constraints as `Option<T>` mutated via `&mut RenderState`) — documented in [`src/storage/state/mod.rs`](src/storage/state/mod.rs) module docstring.
- Multi-source design references (GPUI, Iced, Makepad, Vello, Skia) — [`docs/plans/2026-03-31-engine-hardening.md`](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/docs/plans/2026-03-31-engine-hardening.md) precedent for citing reference codebases when the structural pattern fits Rust idioms better.

---

### Secondary child queries read parent data through an erased per-child accessor

**Rule.** The three query contexts — `BoxDryLayoutCtx`, `BoxIntrinsicsCtx`,
`BoxDryBaselineCtx` — expose `child_parent_data(i) -> Option<&dyn ParentData>` and
`child_parent_data_as::<T>(i)`, backed by a per-node slice the query driver fills from each
child's own parent data (with harness seeds overlaid in test builds). `perform_layout` keeps
its typed `BoxLayoutContext<Arity, PD>::child_parent_data`. Multi-child containers keep sizing
math in one ctx-free routine that takes a measuring closure (`RenderFlex::compute_sizes`), called
by both `perform_layout` (with `layout_child`) and `compute_dry_layout` (with
`child_dry_layout`), so dry and committed sizes cannot drift apart.

**Alternatives.** Making the query contexts generic over `PD` would change ~120 `compute_*`
override signatures and still downcast inside the driver, which holds `dyn` nodes.

**Trade-off.** Typed parent data on the hot path, erased in the query contexts (touched only by
multi-child containers); the container downcasts to the type it declared itself.

### Dry contexts query child intrinsics through the safe take-out walk

**Rule.** `BoxDryLayoutCtx` and `BoxDryBaselineCtx` expose the same `child_intrinsic` /
`child_{min,max}_intrinsic_{width,height}` accessors as the layout context. Each context
dispatches one `#[non_exhaustive]` request enum per context (`DryLayoutChildRequest`:
`DryLayout`, `Intrinsic`, `Baseline`; `DryBaselineChildRequest`), because a context on the
borrowed slot map can hold only one `&mut`-capturing child callback. The dry driver answers
`Intrinsic` with the same take-out `intrinsic_query` it already uses (the queried child is a
different node from the one taken out), sharing the per-node intrinsic cache with the layout
path. `RenderIntrinsicWidth`/`RenderIntrinsicHeight` build child constraints in one helper
parameterized by an intrinsic closure and call it from all three passes:
IntrinsicWidth forces width to the intrinsic whenever
width is not tight, queries with the raw cross-axis maximum, and steps before clamping.

**Why.** Approximating the intrinsic with a loose dry layout gives the wrong answer for exactly
the width-filling children these proxies exist for, breaking dry == committed.

**Alternatives.** One shared request enum for all contexts (dead arms per context); reaching
into the borrowed-arena intrinsic path from the dry driver (imports its aliasing obligations
for nothing).

### Containers record their reported baseline during layout

**Rule.** `compute_distance_to_actual_baseline(&self, baseline)` takes no child channel. A
container computes its own baseline while positioning children in `perform_layout` — using the
layout context's `child_distance_to_actual_baseline` and the offsets it just assigned — and
serves it from a field. `RenderFlex` records both baseline kinds (`reported_baselines`):
horizontal reports the highest child baseline plus its cross offset, vertical the first child
with a baseline plus its main offset. Nesting composes because an inner container's recorded
value is what the outer one reads. Dry baseline shares the positioning math through
`compute_child_offsets` rather than duplicating it.

**Cost.** FLUI pays an eager read of each child's baseline per layout rather than computing
the baseline lazily on first query and memoizing it.

**Alternatives.** A child-query channel on `actual_baseline_raw` would change a widely
implemented signature and need the driver to reconstruct child offsets that containers keep in
their own fields; a lazy memoized query would import `&mut` aliasing into a read that is `&self`
today.

### A follower hit-tests at its last composited position

**Rule.** `PipelineOwner` keeps `last_follower_offsets: FxHashMap<RenderId, Offset>` and
`last_hidden_follower_ids`, per-frame byproducts like the retained layer tree and link registry.
During paint the fragment composer records the `RenderId → LayerId` pair of each
`Layer::Follower` it pushes. After paint, each follower's offset is resolved with the same
`flui_layer::resolve_follower_offset` the GPU path uses; a follower that resolves to `None`
(unlinked, `show_when_unlinked == false`) is recorded as hidden. The hit-test walk, gated on the
tables being non-empty, pushes `Matrix4::translation(r)` on the result's transform stack and
shifts the position by `-r` for a follower's subtree, and skips a hidden follower's subtree.
`RenderFollowerLayer::hit_test` stays a plain structural forward; `hit_test_transform`'s
signature is unchanged.

**Staleness.** Hit-testing uses the last completed composite, so it is one frame stale. The offset is
computed twice (engine for pixels, rendering for hit-test) because a single computation would
need the downstream engine to write into the upstream owner; the logic lives once in
`resolve_follower_offset`. Translation only, like the render path.

### Layout contexts lend the realm's text context, one measurement at a time

**Rule.** A `PipelineOwner` holds the realm's `TextContextHandle`
(`Rc<RefCell<flui_painting::TextContext>>`), a constructor argument: `PipelineOwner::new` and
`new_with_capacity` take it, and there is no `Default` (ADR-0092 §10 step 3). A pipeline with no
realm behind it (a hot-reload plugin image, a test) passes `TextContextHandle::standalone`, a
context over a collection of its own. A frame driver that moves the owner out of its slot for a
typestate transition calls `take_idle`, whose placeholder shares the handle, so the slot a
transition that unwinds leaves still measures through it. The layout walk passes the
cell to every box node it lays out or measures — leaves through `layout_leaf_only`, parents
through `ErasedBoxLayoutCtx`, box intrinsics asked by a box or a sliver parent — and the
intrinsic, dry-layout and dry-baseline query walks pass it to `intrinsic_raw`,
`dry_layout_raw` and `dry_baseline_raw`. A render object sees only a `TextCx`, a scoped
`&mut TextContext` taken from `&mut` context (`BoxLayoutContext::text`,
`BoxIntrinsicsCtx::text`, `BoxDryLayoutCtx::text`, `BoxDryBaselineCtx::text`), so it cannot
lay out a child or query one while it holds the loan. The raw methods and
`BoxLayoutCtxErased::text_source` carry the cell as a `TextSource`, a `Copy` token whose cell
only this crate can borrow, so a direct `RenderObject` implementation passes it on but cannot
hold a loan across a child query. Nothing builds a context implicitly: every layout, intrinsic
and dry-query context is constructed with a `TextSource`, and `layout_leaf_only` takes one.
Slivers get no text accessor: nothing that measures text is a sliver.

**Why a channel.** The realm owns its text context (no ambient, engine-wide font
collection), so the context has to reach the render object through the pipeline that lays it
out.

**Alternatives.** Threading `&mut TextContext` down from the realm would change
`PipelineOwner::run_frame`, `run_layout` and every binding and harness that drives them, and
the realm reaches its presentations through `&self`. A lock would put contention on every
measurement. The `RefCell` sits between the realm and its pipelines, borrowed once per
measurement on the owner thread; a second borrow at the same time is a `BUG:` panic, which
only a measurement that synchronously drives another could cause (a `PipelineCell` checkout is
not re-entrant). A `RefMut` drops on unwind, so a panicking layout releases the loan before the
walk's `catch_unwind` turns it into `Poisoned`. Locked by
`a_layout_that_panics_while_holding_the_text_context_releases_it`,
`intrinsic_and_dry_queries_measure_through_the_pipelines_context` and
`a_taken_pipeline_leaves_an_owner_that_measures_through_the_same_context`
(`tests/text_context.rs`), and the `compile_fail` doctests on `PipelineOwner::new`.

### A font collection change re-lays out what measured text, found by its loans, not by an opt-in mixin

**Rule.** Every loan of the text context a walk makes records the node it was made for: the
walk mints each node's `TextSource` from a crate-private `TextLender` (the context's cell and
the pipeline's `TextMeasurers`, a set of `RenderId`s), and `lend` inserts the node before it
borrows. `PipelineOwner::apply_font_change` reads the collection's generation through a
`try_borrow` of the context: while the context is lent (a drain run from inside a
measurement) it does nothing and the change waits for the next drain, rather than panic. When
the generation differs from the one the pipeline last applied, it takes the set and marks each node still in the tree for layout and paint, through the ordinary
`mark_needs_layout` (which clears cached intrinsics up the ancestor walk and requests a visual
update) and `mark_needs_paint`. `drain_pending_dirty` calls it last, and both frame entries
drain before their first phase: `run_frame` and the runtime's pending-work gate. A test's
`TextContextHandle::source` records nothing.

**Why.** A face registered after text was laid out can change what that text measures to, and
the node that measured it cannot notice: the painter's cache heals only when asked to lay out
again. The pipeline already hands out every loan, so it knows exactly which nodes depend on the
collection, including a third-party render object that measures through `ctx.text()` with no
code of its own.

**Divergence from Flutter.** Checked at flutter `1be6586f8`: `RelayoutWhenSystemFontsChangeMixin`
(`rendering/object.dart:4718-4784`) is opted into per render-object class, subscribes each
attached object to the process-wide `PaintingBinding.systemFonts` notifier
(`painting/binding.dart:173-205`, fed by the engine's `'fontsChange'` system message), and on a
notification schedules a frame callback that calls `markNeedsLayout` in the next frame's
transient-callback phase. That `loadFontFromList` sends `'fontsChange'` is recalled, not
checked. FLUI differs on purpose:

- nothing opts in: measuring through the context is what registers a node, so no text render
  object can forget the mixin;
- no global notifier: each pipeline compares its collection's generation at its next drain,
  before build and layout, the same boundary Flutter's transient callbacks run at;
- paint is marked explicitly, since a repaint boundary's retained layer would otherwise keep
  the old glyphs of a node whose size did not change;
- the cost moves to the frame path: one hash-set insert per measurement, where Flutter pays one
  listener per attached text object.

**Accepted trade-off.** The record is sticky: a node that measured once stays recorded until the
next change takes the set, so a node that stopped measuring text is laid out once more than it
needs, at worst. A removed node's id is skipped (render ids are generational, so a reused slot
never matches it), and each drain drops the removed ids once the set holds more than twice the
live tree: an app that never registers a font keeps a set bounded by its live nodes, not by every
text node it ever built. Locked by `font_change_contract` (`tests/text_context.rs`), whose
`the_record_stays_bounded_without_a_font_change` row rebuilds a paragraph 500 times.


## Thread safety

`flui-rendering` runs in the render pipeline; per strategy clause "sync hot path", the hot frame loop is single-threaded. Sync primitives in this crate are limited to shared-infrastructure objects and lock-free atomics on per-node state. No primitive sits inside `perform_layout` / `paint` on a per-node basis.

| Site | Primitive | Category | Notes |
|---|---|---|---|
| `RenderEntry<P>::render_object` (`src/storage/entry.rs`) | plain `Box<dyn RenderObject<P>>` | Owned by value | Mutable access via `&mut self` from `&mut RenderTree`. The previous `RwLock<Box<dyn>>` was the canonical refusal-trigger violation; removed by the U2 exemplar refactor. |
| `RenderState<P>::flags` (`src/storage/state/mod.rs`) | `AtomicRenderFlags` (wrapping `AtomicU32`) | Lock-free atomics | Bit-level dirty flags + boundary bits. `Acquire/Release` ordering. The new `WAS_REPAINT_BOUNDARY` bit lives here. |
| `RenderState<P>::geometry`, `constraints` (`src/storage/state/mod.rs`) | `Option<ProtocolGeometry<P>>` / `Option<ProtocolConstraints<P>>` | Mutable via `&mut self` | Set and cleared via `&mut RenderState` during layout; no lock required. |
| `RenderState<P>::offset` (`src/storage/state/offset.rs`) | `OffsetCell` | `Cell` (single-threaded tree) | Paint position. |
| `RenderTree::owner` (`src/storage/tree.rs:65`) | `Option<Arc<RwLock<PipelineOwner>>>` | Shared infrastructure | Allowed: locks may guard shared infrastructure. Off the per-node hot path. |
| `PipelineOwner` parent/back-references throughout [`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs) | `Arc<RwLock<PipelineOwner>>`, `Weak<RwLock<PipelineOwner>>` | Shared infrastructure | Soundness-rewrite precedent ([core-crates-hardening Task 7](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/docs/plans/2026-03-31-core-crates-hardening.md)). |
| `RenderTree::nodes` (`src/storage/tree.rs:59`) | `Slab<RenderNode>` | Auto-derived Send+Sync | No `unsafe impl` needed after U2. |
| Viewport listener list (`ScrollableViewportOffset::listeners`, `src/view/viewport_offset.rs`) | `RwLock<Vec<…>>` | Listener registry | Off layout/paint hot path. `FixedViewportOffset`'s former listener list was deleted as speculative API (a fixed offset never notifies). |
| `PipelineOwner::text` (`src/pipeline/text_context.rs`) | `Rc<RefCell<TextContext>>` | Owner-thread shared infrastructure | The realm's text context, shared by its presentations' pipelines. Borrowed once per measurement, never across a child's layout; `!Send`, like the owner. See "Layout contexts lend the realm's text context". |

Two rows left this table because their sites left the crate: the mouse tracker
lives in `flui-interaction` (`crates/flui-interaction/src/routing/mouse_tracker.rs`) and
the render-view error builder in `flui-view` (`crates/flui-view/src/view/error.rs`); each is accounted for in its
owning crate.

`NodePtr` in `src/pipeline/owner/subtree_arena.rs` is a plain raw-pointer newtype for the disjoint-subtree-borrow substrate ([`SubtreeArena`]) — `!Send + !Sync` by the language default, no manual impl. Confinement to the constructing thread is structural (`SubtreeArena` itself is `!Send + !Sync`, pinned by `static_assertions::assert_not_impl_any!`); there is no runtime thread check (`check_thread` and the pointer's former `unsafe impl Send/Sync` were both deleted once `PipelineCell`/dropped `Send + Sync` bounds made confinement type-enforced). Re-entrancy primitives `RenderTree::get_two_mut` and `get_parent_and_children_mut` (both in `src/storage/tree.rs`) are implemented and shipped; their unsafe is local to each function with unit-testable disjoint-keys invariants.

---

## Friction log

Known sites that do not yet match the intended design but do not break a current rule. Each entry names the site and the next planned step.

- **`PipelineOwner` paint-loop downcasts to `Box<dyn ContainerLayer>`** ([`src/pipeline/owner/mod.rs`](src/pipeline/owner/mod.rs)) — the paint phase uses `Box<dyn ContainerLayer>` returned from `RenderObject::paint`. This is correct for compositing-layer heterogeneity but worth periodic audit to ensure the cost stays at the boundary, not in the per-frame inner loop.
- **`docs/LAYOUT_SYSTEM.md`, `docs/HIT_TEST_SYSTEM.md`** — subsystem-level deep-dives. Not part of the template surface. Stay as companion documents.

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

### Phase counters — SHIPPED

**Files:** [`src/pipeline/owner/counters.rs`](src/pipeline/owner/counters.rs) (`PipelineCounters`), `PipelineOwner::counters` in [`src/pipeline/owner/accessors.rs`](src/pipeline/owner/accessors.rs).

`PipelineOwner::counters()` returns work totals since the owner was constructed. They are monotonic because a frame is not one call on the owner: the layout↔build fixpoint runs `run_layout` several times before the frame's `run_frame`, and the owner has no frame-begin hook. A frame driver reads the counters before and after its frame and takes `after.since(before)`, the same way `layout_roots_total` has always been used. `flui-testing`'s `HeadlessBinding::last_frame_report` is that driver headlessly.

| Counter | Counted where | What it counts |
|---|---|---|
| `layout_passes` | `run_layout`, after `take_layout_batch_shallow_first` | non-empty layout batches; a pass with no dirty entry adds nothing |
| `layout_roots` | the scheduler's `layout_drained_total` | dirty layout entries drained, including ones skipped as already clean |
| `nodes_laid_out` | `SubtreeArena`'s box and sliver record sites, drained in `layout_dirty_root` | nodes past the clean-child short-circuit; a cache hit and an intrinsic query do not count |
| `nodes_painted` | `FragmentComposer`, next to `paint_raw` | nodes whose `paint_raw` ran; a grafted boundary is not a paint |
| `layers_produced` | `FragmentComposer::seal_picture`, `push_layer_node`, and `graft`'s patched indices | layers created fresh this pass, including a layer a composited-layer update patches into a grafted boundary |
| `layers_reused` | `FragmentComposer::graft`'s unpatched indices | layers cloned unchanged from a clean boundary's retained output |
| `semantics_nodes_updated` | `run_semantics`, from `SemanticsOwner::flush`'s return | nodes in the delivered accessibility update; 0 when the diff is empty |
| `frames_produced` | `run_frame` | frames that committed a layer tree |

The composer's counts are folded into the owner only on `run_paint`'s commit path, so a paint pass that fails partway adds nothing. Every field is a plain integer (a `Cell` on the `!Send` layout arena): no atomics, no locks. `perf_counters_are_live_on_a_full_reassemble` (`crates/flui-widgets/tests/perf.rs`) pins that every counter but `layers_reused` moves on a full frame (not that each of a counter's
increment sites does), and `perf_scrolling_a_10k_list_one_screen_lays_out_only_the_band` bounds `nodes_laid_out` by the band; the exact per-counter rules above are unasserted.

### Criterion frame benchmarks (deferred -- needs workload generator)

**Files:** new `crates/flui-rendering/benches/frame_throughput.rs`.

**Goal:** profile a 1000-node and a 10,000-node frame to verify (a) no `Arc::clone` in the paint loop, (b) cache layout of `RenderEntry<P>`, (c) regressions vs pre-refactor numbers. Today the static memory-footprint assertions landed in `pipeline/dirty.rs` and `storage/state/tests.rs`; the runtime benchmarks did not.

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

**File:** [`docs/LAYOUT_SYSTEM.md`](docs/LAYOUT_SYSTEM.md), [`docs/HIT_TEST_SYSTEM.md`](docs/HIT_TEST_SYSTEM.md).

These deep-dives stay as companion documents (not under the per-crate template directly); each now opens with a "See also" header line pointing back to this file, linking them into the methodology index.

---

## Notes

- **No lint yet for a lock on per-node render storage.** The clippy lint vocabulary cannot today express "field of type `RwLock<X>` where `X` is a trait object locked in method `foo`", so the rule is held by the storage shape and review; promoting it to a lint waits for ecosystem expressivity (`dylint` plugin or a future clippy feature).
