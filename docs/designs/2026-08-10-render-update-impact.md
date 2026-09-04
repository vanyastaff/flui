# Render update impact — implementation design

Companion to
[ADR-0046](../adr/ADR-0046-render-update-impact-contract.md) and
[issue #534](https://github.com/vanyastaff/flui/issues/534). This design is the
implementation contract for replacing `RenderBehavior`'s unconditional
layout-and-paint invalidation with precise, composable update effects.

- **Status:** Approved
- **Date:** 2026-08-10
- **Semver posture:** atomic breaking migration in an active-development
  workspace; no compatibility layer
- **Maintainer-grade verdict:** **ACCEPTABLE** — `flui-rendering` owns the
  impact type, canonical phase-mark walks, and dirty application; `flui-view`
  owns typed update dispatch and Active-element invariant checks

## Goals

1. Make `RenderView::update_render_object` report exactly which pipeline work
   its mutation requires.
2. Preserve Flutter's setter-level decisions, including false
   `should_relayout`/`should_repaint` results and independent semantics work.
3. Apply the combined report once, after the mutable render-object borrow has
   ended.
4. Force every implementation to choose an impact during one atomic trait
   migration.
5. Keep the update hot path allocation-free and keep invalid bit patterns
   outside the public API.

## Non-goals

- Changing the existing layout, paint, or pipeline-phase algorithms. This
  design does add the missing canonical compositing-bits mark walk described
  below; raw changed-node queue insertion is not preserved as that algorithm.
- Adding a generic invalidation protocol to `BuildContext`.
- Replacing the asynchronous `DirtyKind`/`DirtyRequest` channel used by
  `RenderInvalidationHandle`.
- Completing custom-painter semantics assembly. This change preserves and
  schedules the existing `should_rebuild_semantics` contract; the existing
  `SemanticsBuilder` limitation remains separately documented.
- Changing runtime topology, workspace layers, or crate dependencies.

## Required companion contracts

Removing the blanket layout/paint mark exposes three existing responsibilities
that must remain explicit rather than being accidentally supplied by every
render-view rebuild.

1. `ParentDataView::apply_parent_data` mutates only configuration-owned fields
   of an existing typed value and returns its exact impact. Initial absence
   installs `create_parent_data`; a type mismatch is a `BUG` panic. Layout-owned
   offsets, sibling/container links, and table coordinates survive updates.
2. `PipelineOwner` couples render-child membership mutation to
   `LAYOUT | COMPOSITING_BITS | SEMANTICS`. Cross-parent adoption invalidates
   both parents; removal captures the old surviving parent before deletion.
   Pure sibling reorder is `LAYOUT`. Reconciliation compares `old_ids` with
   the final sequence and calls the encapsulated reorder hook only when they
   differ, leaving same-order updates clean.
3. `HeadlessBinding` retains the last successfully committed `LayerTree` when
   the current frame returns no new tree. Bootstrap passes its frame output to
   `bind_tree_with_committed_layer_tree` atomically; rebind without a seed
   clears old output. `did_paint_last_frame` and `painted_frame_count` expose
   whether fresh work occurred, so retained pixels cannot fake a paint-impact
   test.

These are necessary consequences of precise invalidation, not unrelated
pipeline changes: the former blanket mark hid structural layout work and made
every headless observation frame repaint even when nothing changed.

The companion public seams are:

```rust
impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    pub fn adopt_render_child(&mut self, parent_id: RenderId, child_id: RenderId);
    pub fn drop_render_child(&mut self, parent_id: RenderId, child_id: RenderId);
    pub fn note_render_children_reordered(&mut self, parent_id: RenderId);
}

impl HeadlessBinding {
    pub fn bind_tree_with_committed_layer_tree(
        &mut self,
        build_owner: BuildOwner,
        tree: ElementTree,
        pipeline_owner: PipelineCell,
        committed_layer_tree: Option<LayerTree>,
    );
    pub fn did_paint_last_frame(&self) -> bool;
    pub fn painted_frame_count(&self) -> u64;
}
```

## Public API contract

### Canonical value in `flui-rendering`

Add a private `update.rs` module and re-export its type from the crate root. The
public surface is exactly:

```rust
#[must_use = "render update impacts must be applied to the PipelineOwner"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RenderUpdateImpact(u8);

impl RenderUpdateImpact {
    pub const NONE: Self;
    pub const PAINT: Self;
    pub const LAYOUT: Self;
    pub const COMPOSITING_BITS: Self;
    pub const SEMANTICS: Self;

    pub const fn is_none(self) -> bool;
    pub const fn needs_layout(self) -> bool;
    pub const fn needs_paint(self) -> bool;
    pub const fn needs_compositing_bits_update(self) -> bool;
    pub const fn needs_semantics_update(self) -> bool;
    pub const fn union(self, other: Self) -> Self;
}

impl core::ops::BitOr for RenderUpdateImpact {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output;
}

impl core::ops::BitOrAssign for RenderUpdateImpact {
    fn bitor_assign(&mut self, rhs: Self);
}
```

The private bits are normative: layout is `1 << 0`, paint is `1 << 1`,
compositing is `1 << 2`, and semantics is `1 << 3`. `LAYOUT` is
`layout | paint`; `COMPOSITING_BITS` is `compositing | paint`.

There is deliberately no `from_bits`, `bits`, primitive conversion,
`PartialOrd`, `Ord`, `Result`, or public bit constant. `Default` equals `NONE`.
The representation is copyable because it carries facts about one completed
mutation, not ownership or deferred execution.

Private implementation bits use one bit per independent fact:

```rust
const LAYOUT_BIT: u8 = 1 << 0;
const PAINT_BIT: u8 = 1 << 1;
const COMPOSITING_BITS_BIT: u8 = 1 << 2;
const SEMANTICS_BIT: u8 = 1 << 3;
```

The named constants are constructed internally as:

| Constant | Private bits | Rationale |
|---|---:|---|
| `NONE` | `0000` | No observable render-pipeline change. |
| `PAINT` | `0010` | Repaint only. |
| `LAYOUT` | `0011` | Layout and eventual paint are one supported invariant. |
| `COMPOSITING_BITS` | `0110` | Compositing changes also require paint. |
| `SEMANTICS` | `1000` | Accessibility work is independent of visual work. |

`union` performs private-bit OR. `BitOr` delegates to `union`, and
`BitOrAssign` assigns that result. No operation can remove an impact.

### Opaque custom-path source identity in `flui-objects`

Callback-backed path configuration uses this separate public identity seam:

```rust
#[derive(Clone)]
pub struct ClipSourceToken(Arc<PrivateMarker>);

impl ClipSourceToken {
    pub fn fresh() -> Self;
}

impl Debug for ClipSourceToken { /* opaque output */ }
impl PartialEq for ClipSourceToken { /* Arc::ptr_eq */ }
impl Eq for ClipSourceToken {}

impl RenderClipPath {
    pub fn with_path_clip_source_token(self, token: ClipSourceToken) -> Self;
    pub fn set_path_clip_source_token(&mut self, token: &ClipSourceToken) -> RenderUpdateImpact;
    pub fn set_path_clip_target(&mut self, target: Option<PathClipTarget>) -> RenderUpdateImpact;
}

impl RenderPhysicalShape {
    pub fn with_path_clip_source_token(self, token: ClipSourceToken) -> Self;
    pub fn set_path_clip_source_token(&mut self, token: &ClipSourceToken) -> RenderUpdateImpact;
    pub fn set_path_clip_target(&mut self, target: Option<PathClipTarget>) -> RenderUpdateImpact;
}

impl RenderShaderMask {
    pub fn set_shader_target(&mut self, target: Option<ShaderMaskTarget>) -> RenderUpdateImpact;
}
```

`PrivateMarker` and the `Arc` field are private. There is no raw constructor or
accessor, pointer-derived public value, `Copy`, `Hash`, `Default`, global
counter, or exhaustion panic. `ClipPath::new` and `PhysicalShape::new` allocate
a fresh token; cloning either widget preserves it. The setter borrows and clones
only on replacement. A token change contributes `PAINT | SEMANTICS`.

### Reachable-state truth table

The private constructor makes states such as layout-without-paint and
compositing-without-paint unreachable. The following are all distinct states
constructible from the public constants; expressions that differ only by an
already-implied `PAINT` collapse to the same row.

| Public expression | Layout query | Compositing query | Paint query | Semantics query |
|---|:---:|:---:|:---:|:---:|
| `NONE` | no | no | no | no |
| `PAINT` | no | no | yes | no |
| `LAYOUT` | yes | no | yes | no |
| `COMPOSITING_BITS` | no | yes | yes | no |
| `SEMANTICS` | no | no | no | yes |
| `PAINT | SEMANTICS` | no | no | yes | yes |
| `LAYOUT | SEMANTICS` | yes | no | yes | yes |
| `COMPOSITING_BITS | SEMANTICS` | no | yes | yes | yes |
| `LAYOUT | COMPOSITING_BITS` | yes | yes | yes | no |
| `LAYOUT | COMPOSITING_BITS | SEMANTICS` | yes | yes | yes | yes |

### `PipelineOwner` application seam

Add these methods to the phase-agnostic
`impl<Phase: PipelinePhase> PipelineOwner<Phase>` block:

```rust
pub fn mark_needs_compositing_bits_update(&mut self, render_id: RenderId);

pub fn mark_needs_semantics(&mut self, render_id: RenderId);

pub fn apply_render_update_impact(
    &mut self,
    render_id: RenderId,
    impact: RenderUpdateImpact,
);
```

All three methods return `()`. `apply_render_update_impact` is the canonical
cross-crate applicator; no parallel applicator exists in `flui-view`.

`mark_needs_compositing_bits_update` owns Flutter-compatible compositing
propagation. Starting at `render_id`, it marks/walks the necessary ancestor
chain and queues the responsible root. The former
`add_node_needing_compositing_bits_update(render_id, depth)` raw operation was
removed: calling such an insertion for the changed node bypasses ancestor
propagation and is insufficient for this contract.
Every owner-side compositing request, including replay of
`DirtyKind::Compositing` in `drain_pending_dirty`, routes through the canonical
method.

The canonical behavior is exactly Flutter's
`RenderObject.markNeedsCompositingBitsUpdate` at
`packages/flutter/lib/src/rendering/object.dart:3209-3227`. Starting at the
target node:

1. If its `NEEDS_COMPOSITING_BITS_UPDATE` flag is already set, return.
2. Set that flag on the target.
3. If it has a parent whose flag is already set, return. The parent's existing
   responsible queue entry already covers the path; the target remains marked
   so the compositing traversal reaches it.
4. If it has a parent, recurse to that parent only when
   `(!target.was_repaint_boundary() || !target.is_repaint_boundary_flag())`
   and the parent is not currently a repaint boundary. Return after recursing.
5. Otherwise queue the target at its authoritative live depth. A parentless
   target also reaches this step.

Equivalent pseudocode, intentionally written as behavior rather than a
borrow-checker-specific implementation, is:

```rust
fn mark(target_id) {
    let Some(target) = tree.get(target_id) else { return };
    if target.needs_compositing_bits_update() {
        return;
    }
    target.mark_needs_compositing_bits_update();

    if let Some(parent_id) = target.parent() {
        let parent = tree.get(parent_id).expect("BUG: live child must have a live parent");
        if parent.needs_compositing_bits_update() {
            return;
        }
        if (!target.was_repaint_boundary() || !target.is_repaint_boundary_flag())
            && !parent.is_repaint_boundary_flag()
        {
            mark(parent_id);
            return;
        }
    }

    queue(target_id, target.depth());
}
```

The owner implementation may use an iterative ancestor loop to satisfy Rust
borrowing, but each stop, flag write, and queue decision must be observationally
identical. In particular, both newly introduced and lost repaint boundaries
walk through non-boundary ancestors, while an established target boundary
stops and queues itself.

`mark_needs_semantics` owns the semantics-enabled check, authoritative live
depth lookup, and queue insertion. Semantics dirtiness is queue-only today.
Raw semantics queue insertion is owner-private and is insufficient as the
public update boundary because it makes callers duplicate the enabled and
authoritative-depth policy.
`RenderFlags::NEEDS_SEMANTICS`, `RenderNode::mark_semantics_flag`, and the
matching query are legacy/future scaffolding, not a second source of current
dirty truth: `run_semantics` consumes the queue and does not clear that flag.
The new canonical method must not set it. Remove that flag/accessor surface in
this migration only if a mechanical call-site audit proves it is unused and
safe to delete; otherwise document it as unused by the canonical path.

### `RenderView` contract

Change the required trait method to:

```rust
fn update_render_object(
    &self,
    ctx: &RenderObjectContext<'_>,
    render_object: &mut Self::RenderObject,
) -> RenderUpdateImpact;
```

There is no default body. The return value is the union of the impacts from
every changed field. The context and render object are still borrowed exactly
as before; no owner or dirty handle is added to the signature.

### Exports

The same canonical type is reachable through all four ergonomic paths:

```rust
flui_rendering::RenderUpdateImpact
flui_rendering::prelude::RenderUpdateImpact
flui_view::RenderUpdateImpact
flui_view::prelude::RenderUpdateImpact
```

`flui-view` uses `pub use flui_rendering::RenderUpdateImpact`; it does not
declare a second type. The three `PipelineOwner` operations remain inherent
methods, so no extension trait or extra prelude export is needed.

## Application algorithm and precedence

The owner method first returns for `NONE`. An absent/stale `RenderId` schedules
no work, consistent with the existing phase-agnostic dirty methods. Framework
dispatch prevalidates its stronger invariants before this method is reached.
The canonical compositing and semantics mark methods own their respective
walk/enabled/depth policy; the applicator does not reproduce it.

The normative algorithm is:

```rust
if impact.is_none() {
    return;
}

if impact.needs_layout() {
    self.mark_needs_layout(render_id);
}
if impact.needs_compositing_bits_update() {
    self.mark_needs_compositing_bits_update(render_id);
}
if impact.needs_paint() && !impact.needs_layout() {
    self.mark_needs_paint(render_id);
}
if impact.needs_semantics_update() {
    self.mark_needs_semantics(render_id);
}
```

The ordering and de-duplication rules are normative:

| Requested impact | Immediate owner operations, in order | Why |
|---|---|---|
| `NONE` | none | Avoid even the depth lookup. |
| `PAINT` | paint | The cheapest visual update. |
| `LAYOUT` | layout | Successful layout schedules eventual paint; do not perform a duplicate immediate paint walk. |
| `COMPOSITING_BITS` | canonical compositing ancestor walk, paint | Layer requirements and retained paint output must agree. |
| `SEMANTICS` | canonical semantics mark | The mark method owns enablement and depth; enabling semantics later seeds the root. |
| `LAYOUT | COMPOSITING_BITS` | layout, canonical compositing ancestor walk | Layout owns eventual paint; compositing still has its own propagation and queue. |
| Any visual impact `| SEMANTICS` | the visual operations above, then semantics if enabled | Accessibility remains independent and runs after visual dirtiness is registered. |

Every branch runs at most once even if several setters returned the same
constant. Dirty-set dedup remains a second line of defense, not the primary
composition mechanism.

## `RenderBehavior` transaction and invariants

`RenderBehavior::on_update` replaces the current permissive nested `if let`
chain and removes its call to `mark_render_needs_layout_and_paint`.

Lifecycle inspection found no legitimate non-Active `on_update` path.
Ordinary reconciliation updates an Active reused element. Both GlobalKey
retake paths in `element_tree.rs` call `activate_subtree` before
`element.update`; the ordinary reconciler reaches `ElementTree::update` only
for a reused live child. Therefore a missing owner is not a detached no-op; it
is a broken Active-element invariant. The owner is required first:

```rust
let pipeline_owner = core.pipeline_owner().expect(
    "BUG: active RenderBehavior must have a PipelineOwner during update",
);
```

With an owner present, these failures are impossible framework states and use
the exact invariant-oriented messages below:

| Failure | Required message |
|---|---|
| `core.pipeline_owner()` is `None` | `BUG: active RenderBehavior must have a PipelineOwner during update` |
| `self.render_id` is `None` | `BUG: RenderBehavior with a PipelineOwner must own a RenderId during update` |
| The owned ID has no live node | `BUG: RenderBehavior's RenderId must resolve to a live node during update` |
| The live node is not `V::RenderObject` | `BUG: RenderBehavior's live node must match RenderView::RenderObject during update` |

Mutation and application use separate `PipelineCell::with_mut` closures. The
first returns the impact, proving the mutable render-object borrow has ended
before the owner is borrowed again:

```rust
let pipeline_owner = core.pipeline_owner().expect(
    "BUG: active RenderBehavior must have a PipelineOwner during update",
);
let render_id = self.render_id.expect(
    "BUG: RenderBehavior with a PipelineOwner must own a RenderId during update",
);
let ctx = RenderObjectContext::new(owner.interaction_dispatch.as_ref());

let impact = pipeline_owner.with_mut(|pipeline_owner| {
    let node = pipeline_owner
        .render_tree_mut()
        .get_mut(render_id)
        .expect("BUG: RenderBehavior's RenderId must resolve to a live node during update");
    let render_object = node
        .downcast_render_object_mut::<V::RenderObject>()
        .expect(
            "BUG: RenderBehavior's live node must match RenderView::RenderObject during update",
        );
    core.view().update_render_object(&ctx, render_object)
});
pipeline_owner.with_mut(|pipeline_owner| {
    pipeline_owner.apply_render_update_impact(render_id, impact);
});
```

Validation happens before mutation. A type mismatch therefore cannot partially
update a node. No clone, box, lock, or mutable accumulator crosses the
render-object borrow.

## Parent-data update contract

Parent-data reconciliation must not independently mark layout on every reused
child. The typed API is therefore:

```rust
pub trait ParentDataView: Clone + 'static + Sized {
    type ParentData: ParentDataConfig;

    fn apply_parent_data(
        &self,
        existing: &mut Self::ParentData,
    ) -> RenderUpdateImpact;
}
```

The method is required. Implementations compare and mutate only fields owned
by widget configuration: `LayoutId` owns `id`, `Flexible`/`Expanded` own
`flex` and `fit`, `Positioned` owns its six positioning fields, and
`TableCell` owns vertical alignment. They preserve layout-owned offset,
row/column placement, and container/sibling metadata. Unchanged configuration
returns `NONE`; a changed configuration returns `LAYOUT`.

The object-safe element dispatch downcasts existing parent data to the
associated concrete type with a `BUG` invariant message, invokes the typed
hook, ends the child-node borrow, and applies the impact to the render parent.
When parent data is absent during initial attachment, it installs
`create_parent_data` without treating initialization as an update.

Two additive public, defaulted object-safe seams route that typed operation:
`ElementBase::apply_parent_data_config` is the element-tree bridge, and the
unified element forwards through
`ElementBehavior::apply_parent_data_config`. The second hook is required
because `ElementKind::Proxy` erases ordinary proxy and typed parent-data
elements behind the same trait object; behavior dispatch is the final point
that still knows `V::ParentData`. Both defaults return `NONE`, the traits stay
unsealed, and `ParentDataBehavior` alone performs the typed downcast.

```rust
pub trait ElementBase {
    fn apply_parent_data_config(
        &self,
        parent_data: &mut dyn ParentData,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}

pub trait ElementBehavior<V, A> {
    fn apply_parent_data_config(
        &self,
        core: &ElementCore<V, A>,
        parent_data: &mut dyn ParentData,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}
```

## Setter migration contract

The render object owns field equality and delegate comparison, so its setter
returns `RenderUpdateImpact`. The view updater owns composition across fields.
A typical updater becomes:

```rust
fn update_render_object(
    &self,
    _ctx: &RenderObjectContext<'_>,
    render_object: &mut Self::RenderObject,
) -> RenderUpdateImpact {
    render_object.set_first(self.first)
        | render_object.set_second(self.second)
}
```

Setters must still install the new value and migrate subscriptions even when
the returned impact is `NONE`. `NONE` means no pipeline pass is needed, not
that the setter may skip required ownership/lifecycle work.

### Normative migration table

| Setter/update family | Condition | Returned impact |
|---|---|---|
| Custom single-child layout delegate | Same instance, or same concrete type with false `should_relayout` | `NONE` |
| Custom single-child layout delegate | Concrete type changes or `should_relayout` is true | `LAYOUT` |
| Custom multi-child layout delegate | Same instance, or same concrete type with false `should_relayout` | `NONE` |
| Custom multi-child layout delegate | Concrete type changes or `should_relayout` is true | `LAYOUT` |
| Flow delegate | Concrete type changes or `should_relayout` is true | `LAYOUT` |
| Flow delegate | No relayout and `should_repaint` is true | `PAINT` |
| Flow delegate | Neither delegate predicate requests work | `NONE` |
| Flow clip behavior | Value changes | `PAINT | SEMANTICS` |
| Sliver grid delegate, eager and lazy variants | Concrete type changes or `should_relayout` is true | `LAYOUT` |
| Sliver grid delegate, eager and lazy variants | Otherwise | `NONE` |
| Lazy list/grid owner-local builder | Every replacement | `LAYOUT` (opaque closure behavior cannot be compared; resident children are refreshed) |
| Custom painter or foreground painter | See the independent-decision table below | Union of `PAINT` and `SEMANTICS` |
| Custom paint preferred size | Value changes | `LAYOUT` |
| `RenderClip*` clip behavior | Value changes | `PAINT` |
| Rounded/path/custom clip source | Typed source token/data changes or `should_reclip` is true | `PAINT | SEMANTICS` |
| Fitted-box fit | Transition from or to `ScaleDown` | `LAYOUT` |
| Fitted-box fit/alignment | Other effective change | `PAINT` |
| Fitted-box or unconstrained-box overflow clip | Effective value changes | `PAINT | SEMANTICS` |
| Physical-model/physical-shape clip behavior | Effective value changes | `PAINT` |
| Physical-model shape/border radius or physical-shape source token | Effective value changes | `PAINT | SEMANTICS` |
| Material effective shape configuration | Value changes per `ShapeBorderClipper::shouldReclip` equivalence | replace the owner-lane clipper and return `PAINT | SEMANTICS`; unchanged returns `NONE` and preserves the registered clipper |
| Sliver opacity | Alpha changes | `PAINT`, plus `COMPOSITING_BITS` only when the layer predicate changes and `SEMANTICS` only when visibility changes |
| Layout geometry: constraints, padding, alignment used by layout, flex/wrap/stack/table geometry, text shaping/metrics, viewport/grid/list extents | Effective value changes | `LAYOUT` |
| Paint-only data: color, decoration, shader, image pixels/fit/alignment when intrinsic geometry is unchanged | Effective value changes | `PAINT` |
| Transform or fractional-translation geometry | Effective value changes | `PAINT | SEMANTICS` |
| Fractional-translation hit-test policy only | Effective value changes | `NONE` |
| Image intrinsic dimensions, forced width/height, or scale | Effective value changes | `LAYOUT` |
| Image source | Identity changes but intrinsic geometry does not / intrinsic geometry changes | `PAINT` / `LAYOUT` |
| A field that can change `always_needs_compositing`/repaint-boundary status | Predicate changes | `COMPOSITING_BITS`, unioned with any independent semantics effect |
| The same composited field while its compositing predicate stays unchanged | Visible output changes | `PAINT`, unioned with any independent semantics effect |
| Pure semantics configuration, exclusion, or interaction state represented in semantics | Effective value changes | `SEMANTICS` |
| Existing `flui_painting::text_painter::Invalidation` result | `None` / `Paint` / `Layout` | `NONE` / `PAINT` / `LAYOUT` |
| Fixture or updater with no observable render-object mutation | Always | `NONE` |
| Any setter | Effective value is unchanged | `NONE` |

The broad family rows are classification rules, not permission to guess. Each
setter is cross-checked against the corresponding Flutter setter before its
impact is committed. When one field affects several independent phases, the
setter returns their union.

### Custom painter decisions are independent

Do not derive semantics work from the paint verdict. For each background and
foreground painter independently:

| Old → new painter | Paint contribution | Semantics contribution |
|---|---|---|
| `None → None` | none | none |
| `None → Some` | `PAINT` | `SEMANTICS` |
| `Some → None` | `PAINT` | `SEMANTICS` |
| Concrete type changes | `PAINT` | `SEMANTICS` |
| Same concrete type | `PAINT` iff `new.should_repaint(old)` | `SEMANTICS` iff `new.should_rebuild_semantics(old)` |

Thus a painter can correctly return paint only, semantics only, both, or
neither. Background and foreground results are unioned with each other and
with the preferred-size result.

### Compositing-sensitive examples

Opacity and physical-model setters must compare the old and new compositing
predicate before overwriting the field. If the predicate changes, return
`COMPOSITING_BITS`; its implied paint bit avoids a second constant. Visibility
transitions that alter semantics also union `SEMANTICS`. A composited value
change that does not change the predicate returns `PAINT` (or the existing
more-specific composited-layer behavior when represented by FLUI); it must not
schedule the compositing-bits walk merely because a layer exists.

### Clips are not one uniform category

Flutter distinguishes changing *how* an existing clip is rendered from
changing the clip geometry:

- `clip_behavior` changes paint output only for the ordinary `RenderClip*`
  objects, so they return `PAINT`;
- border-radius, path-target, or delegate/clipper geometry changes alter the
  approximate semantics clip too, so they return `PAINT | SEMANTICS`;
- `RenderFlow::clip_behavior` and fitted-box overflow clipping affect both
  paint and semantics in their Flutter counterparts and therefore return the
  union.

This distinction must remain visible in setter tests. A shared method name is
not evidence that all owning render-object types have the same impact.

## Atomic implementation ripple

### `flui-rendering`

1. Add `src/update.rs`, root/prelude exports, unit tests for constants,
   queries, union, `BitOr`, `BitOrAssign`, and `Default`.
2. Add canonical `PipelineOwner::mark_needs_compositing_bits_update` and
   `PipelineOwner::mark_needs_semantics` methods beside the existing dirty
   methods in `pipeline/owner/accessors.rs`. The former owns ancestor
   propagation; the latter owns enabled/depth/queue policy and stays
   queue-only.
3. Add `PipelineOwner::apply_render_update_impact` in the same phase-agnostic
   block, calling only canonical mark methods. Change
   `drain_pending_dirty`'s `DirtyKind::Compositing` arm to call the same
   compositing method rather than the raw add-node primitive.
4. Audit the legacy `NEEDS_SEMANTICS` flag/accessors and remove them only if
   mechanically safe; never set the flag from the new path.
5. Add owner tests covering every reachable truth-table state, stale ID,
   disabled semantics, depth lookup, compositing ancestor propagation,
   application order, and layout's immediate-paint suppression followed by
   eventual paint after successful layout.

### `flui-view`

1. Change the required `RenderView` method and its documentation/example.
2. Re-export the canonical type from the root and prelude.
3. Rewrite `RenderBehavior::on_update` with strict prevalidation and scoped
   borrows; delete `mark_render_needs_layout_and_paint` when its last caller is
   gone.
4. Update all production implementations and all local test fixtures. No
   implementation receives a temporary blanket `LAYOUT` return merely to make
   the trait compile.

### `flui-objects`, `flui-widgets`, and downstream workspace consumers

1. Convert every public render-object configuration setter from `bool`, `()`,
   or `DelegateChange` to `RenderUpdateImpact` when its effective change
   affects layout, paint, compositing, or semantics. Hit-test-only and internal
   bookkeeping setters may retain change flags; storing a new clip/shader
   target on a render object is phase-affecting and therefore returns the
   canonical impact.
   Remove `DelegateChange` if no non-update consumer remains.
2. Update setter unit tests to assert the exact impact, including unchanged
   values and independent combinations.
3. Update every `RenderView` implementation in `flui-widgets`,
   `flui-material`, examples, and test fixtures to return the union of its
   setter results.
4. Preserve non-widget invalidation paths such as `Listenable` callbacks and
   `RenderInvalidationHandle`; they continue to mark the owner directly because no
   `RenderView` update is running at those times.

### Public setter and aggregate surface audit

Every public addition is used by a cross-crate consumer or deliberately forms
the canonical render-object configuration surface; test-only helpers remain in
test modules. The migration map is:

| Owner | Public additions or changed setters | Contract |
|---|---|---|
| `flui-rendering::PipelineOwner` | `adopt_render_child`, `drop_render_child`, `note_render_children_reordered`, `mark_needs_compositing_bits_update`, `mark_needs_semantics`, `apply_render_update_impact` | Canonical structural and phase scheduling seams described above. |
| `flui-testing::HeadlessBinding` | `bind_tree_with_committed_layer_tree`, `did_paint_last_frame`, `painted_frame_count` | Retained committed output plus independent fresh-paint evidence. |
| `flui-view` traits | `ElementBase::apply_parent_data_config`, `ElementBehavior::apply_parent_data_config`; changed `ParentDataView::apply_parent_data` and `RenderView::update_render_object` | Object-safe typed parent-data dispatch and required precise update reporting. |
| `flui-objects` clip family | `ClipSourceToken::fresh`, `PathClipConfiguration`, both render types' typed source/configuration builders and setters | Opaque callback-source identity plus value-comparable built-in shape configuration; token setters borrow and clone only on change. |
| `flui-objects` layout | align/center/wrap/flex `update_*`; baseline, fitted-box, fractional-translation, padding, sized-box, transform, custom-layout and flow `set_*` methods | Preserve render-owned state and return exact impact rather than whole-object replacement. |
| `flui-objects` paint/content | image, custom-paint, decoration, opacity, physical-model, shader/backdrop, leader/follower, semantics-configuration, paragraph and editable `set_*` methods | Field equality and independent paint/layout/compositing/semantics decisions remain authoritative in the render object. |
| `flui-objects` sliver/virtualization | persistent-header, fill-viewport, fixed-extent, grid, lazy grid/list, sliver-opacity `set_*`, viewport offset/order/axis/cache setters, and `Virtualizer::set_default_estimate` | Exact extent/delegate/opacity changes while preserving measured virtualized state; cache setters remain `const`. |

No listed method is public solely for the framework implementation: the
render-object setters are the supported direct-authoring surface already used
by harnesses and external render-object composition. The temporary public
`RenderTheater::laid_out_child_count` and
`ElementBase::note_render_children_changed` were removed rather than retained
as migration artifacts.

Before declaring the migration complete, run:

```text
rg -n "fn update_render_object" crates examples
```

and inspect every hit. The absence of a default implementation is the compile
guard, but macro-generated, test-only, and example implementations remain part
of the atomic ripple.

## Tests and evidence

### Value and owner tests

- `RenderUpdateImpact::default().is_none()`.
- Every constant has the truth-table query results above.
- Union is commutative, associative, idempotent, and has `NONE` as identity.
- `|` and `|=` match `union`.
- `LAYOUT` and `COMPOSITING_BITS` always report paint.
- Applying `LAYOUT` queues layout but does not immediately queue paint.
- After that assertion, a successful `run_layout` queues the required paint,
  proving de-duplication did not lose eventual paint.
- Applying `COMPOSITING_BITS` queues compositing and paint.
- An established target repaint boundary (`was_repaint_boundary == true`,
  `is_repaint_boundary_flag == true`) is marked and queues itself without
  dirtying its parent.
- A newly introduced target boundary (`false → true`) walks through
  non-boundary ancestors and queues the responsible ancestor.
- A target that lost boundary status (`true → false`) walks upward through
  non-boundary ancestors and queues the responsible ancestor.
- A non-boundary child under a current repaint-boundary parent is marked and
  queues itself without dirtying the parent.
- When the parent is already compositing-dirty, marking the child sets the
  child's flag but adds no queue entry; the existing responsible queue remains
  authoritative and repeated marking is idempotent.
- A parentless target is marked and queues itself at its live depth.
- A stale compositing target ID is a no-op.
- The `RenderInvalidationHandle`/`DirtyKind::Compositing` replay path passes the same
  boundary cases, proving it does not use raw changed-node queue insertion.
- Applying a union queues each requested phase once and in the required order.
- A semantics impact is ignored while semantics are disabled; enabling
  semantics later still seeds the root through the existing path.
- Applying `SEMANTICS` while enabled changes the semantics queue without
  setting `NEEDS_SEMANTICS`; the queue is the sole dirty truth and is consumed
  normally.
- An absent/stale direct-owner ID schedules nothing.
- Child adoption/detachment/removal queues layout, compositing, and enabled
  semantics once for the surviving parent; cross-parent adoption covers both
  parents, and removal captures the old parent before freeing the child.
- A pure sibling reorder queues layout only, while an identical old/final
  child-ID sequence leaves all queues and flags clean.
- Active and inactive GlobalKey cross-parent moves assert both parents'
  membership invalidation, shared ancestor queue de-duplication, and no
  immediate paint.
- Lazy layout-time `DeferredMutation::Insert` directly asserts child and
  parent layout scheduling, canonical compositing/semantics work, and no
  immediate parent paint.

### Framework invariant tests

- An Active `RenderBehavior::on_update` without a pipeline owner panics with
  `BUG: active RenderBehavior must have a PipelineOwner during update`.
- An owner with no behavior `RenderId` panics with the required `BUG` message.
- A missing owned node panics with the required `BUG` message.
- A mismatched concrete render-object type panics before mutation with the
  required `BUG` message.
- A valid update mutates first and applies the returned impact after the node
  borrow ends.
- `NONE` produces no owner dirty work, proving the blanket invalidation is
  gone.
- Bootstrap output is visible immediately after binding; an idle `NONE` frame
  retains the identical committed tree while reporting
  `did_paint_last_frame == false` and an unchanged paint count.
- A dirty frame replaces committed output, rebind without a seed clears it,
  and a gesture-only binding has no committed output.

### Setter and parity tests

- Unignore
  `custom_single_child_layout_test::a_false_should_relayout_prevents_relayout_pin`.
- Unignore
  `custom_multi_child_layout_test::a_false_should_relayout_prevents_relayout_pin`.
- Add true/false delegate tests for single-child, multi-child, flow, and grid.
- Add custom-painter tests for all four same-type combinations:
  neither, paint only, semantics only, and both; also cover `None` transitions
  and concrete type changes.
- Assert flow delegate precedence: relayout wins over repaint; repaint is used
  only when relayout is false.
- Assert ordinary clip-behavior changes are paint-only and clip-source changes
  are paint plus semantics.
- Assert `ClipSourceToken::fresh` differs across constructions, clone
  preserves identity, its raw representation cannot be constructed or read,
  and both `ClipPath` and `PhysicalShape` return `NONE` for cloned sources but
  `PAINT | SEMANTICS` for separately constructed sources.
- Assert sliver-opacity stable visible alpha changes paint, layer-predicate
  transitions add compositing, visibility transitions add semantics, and an
  identical alpha is `NONE`.
- Assert fitted-box fit transitions involving `ScaleDown` lay out, other fit
  and alignment changes paint, and fitted/unconstrained overflow clips add
  semantics.
- Assert physical-model shape/border-radius and physical-shape source changes
  return `PAINT | SEMANTICS`.
- Assert identical semantics configuration and options return `NONE`, while
  every changed option, including exclude and block-user-actions, contributes
  `SEMANTICS`.
- Assert transform/fractional geometry is paint plus semantics while a
  hit-test-only policy change is `NONE`, without resetting child layout state.
- Assert image source/fit/alignment changes distinguish paint from intrinsic
  geometry layout, and that identical `RawImage` configuration is `NONE`.
- Assert unchanged values return `NONE` for every migrated setter family.
- Assert real configuration changes still schedule every downstream phase
  required by the corresponding Flutter setter.

### Mutation evidence

Target the new boundary with `cargo-mutants` after the normal tests are green.
At minimum, tests must kill mutations that:

- replace a delegate's `NONE` with `LAYOUT`;
- replace `RenderBehavior`'s returned impact with blanket `LAYOUT`;
- discard one side of a custom-painter paint/semantics union;
- remove the `!impact.needs_layout()` paint guard;
- remove paint from `COMPOSITING_BITS`;
- invert a delegate's `should_relayout` or `should_repaint` decision.

Report the targeted mutation denominator and every timeout/exclusion. A
surviving mutation in this list is not acceptable evidence.

The bounded implementation run covered 246 generated mutants across the
impact algebra, owner application/structural seams, render update dispatch,
typed parent data, delegates, custom paint, fitted-box retained caches, clips,
semantics, and opacity. The initial result was 158 caught, 42 missed, 46
unviable, and zero timeouts. Targeted regression tests closed every meaningful
miss; the final classification is 186 caught, 14 residual, 46 unviable, and
zero timeouts (93% of viable mutants caught). Five residuals are true
equivalences: three disjoint-bit algebra substitutions and two
`NONE`-to-`Default::default()` substitutions. The other nine are real,
unchanged sliver-grid geometry-field deletion gaps pulled in by the
function-name filter; they are explicitly out of this delegate-setter and
render-update contract rather than being mislabeled equivalent. All setter,
application, retained-cache,
clip, semantics, opacity, and parent-data mutants in the intended scope were
caught after the focused reruns. Raw output is intentionally kept outside the
repository under `/tmp/flui-mutants-534.*`.

A supplemental API-tail run generated 18 mutants for
`set_path_clip_source_token`, `set_path_clip_target`, and
`set_shader_target`: 10 were caught, eight were unviable, and none survived or
timed out. Its output is under `/tmp/flui-mutants-534-final-target`.

Two disposable-source manual probes cover transformations that the generated
mutants do not synthesize. Replacing the returned impact with blanket
`LAYOUT` made `render_behavior_applies_an_exact_union_once` fail on the
unexpected layout queue, and inverting both eager and lazy grid delegates'
`should_relayout` decisions made the shared delegate matrix fail on the
same-type/unchanged case. The probes ran in temporary copies outside the
repository and left the verified source tree unchanged.

### Verification commands

Run narrow gates while iterating, then the repository gate:

```text
cargo nextest run -p flui-rendering -p flui-view -p flui-objects -p flui-widgets
cargo test -p flui-widgets --doc
cargo clippy -p flui-rendering -p flui-view -p flui-objects -p flui-widgets --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
just port-check-verbose
just ci
taplo fmt --check
typos
```

The render/layout/lifecycle Definition of Done also requires checking the
implemented setter mappings against the Flutter files named below. A green
gate alone does not prove parity.

## Breaking-change and compatibility policy

This is deliberately source-breaking:

- `RenderView::update_render_object` changes return type and has no default;
- every downstream trait implementation must classify its mutation;
- migrated setter return types replace `bool`, `()`, and `DelegateChange`
  where those values represented update impact;
- `#[must_use]` turns silently discarded results into warnings, which are
  errors under workspace policy.
- `ParentDataView::apply_parent_data` changes from a replacement-style unit
  hook to a required impact-returning hook; downstream implementations must
  preserve layout-owned fields and classify their configuration changes.
- The defaulted public `ElementBase::apply_parent_data_config` and
  `ElementBehavior::apply_parent_data_config` methods are additive. A
  downstream same-name method can require qualified syntax; this accepted
  pre-1.0 collision risk ships with the `0.3` migration.
- The canonical owner child-membership methods and headless committed-output
  binding method are additive, but raw render-tree membership edits are no
  longer valid framework mutation seams.
- `ClipSourceToken` and its typed builders/setters are additive, replacing
  the unshipped raw-`usize` draft. The token intentionally exposes no raw
  identity and does not implement `Copy`, `Hash`, or `Default`.
- `SemanticsConfiguration` now supports semantic equality; action handlers are
  equal only when their `Arc` identities match. This lets identical annotation
  updates return `NONE` without treating different callbacks as interchangeable.

The new type and owner method are additive APIs, but they do not make the
overall migration additive. The workspace is pre-release and permits the
atomic break. Do not ship a deprecated unit-returning trait, adapter trait,
conversion from `DirtyKind`, blanket `LAYOUT` fallback, or deferred-migration
placeholder.

No `docs/runtime-contract.toml` entry changes: this does not add a runtime
owner, global capability, scheduling topology, or root-export family monitored
by that registry. No `docs/workspace-layers.toml` change is needed: dependency
directions are unchanged.

## Flutter reference checklist

Reference checkout: `/mnt/data/dev/flutter`, commit
`f2d640ef01561447051f582059295a68ca2046ae` (2026-08-09).

| Behavior | Flutter source |
|---|---|
| Element invokes updater without blanket invalidation | `packages/flutter/lib/src/widgets/framework.dart`, `RenderObjectElement._performRebuild` |
| Layout, paint, compositing, semantics dirty semantics | `packages/flutter/lib/src/rendering/object.dart` |
| Independent painter paint/semantics decisions | `packages/flutter/lib/src/rendering/custom_paint.dart`, `_didUpdatePainter` |
| Flow delegate precedence and clip effects | `packages/flutter/lib/src/rendering/flow.dart`, `delegate` and `clipBehavior` setters |
| Grid delegate relayout decision | `packages/flutter/lib/src/rendering/sliver_grid.dart`, `gridDelegate` setter |
| Custom clip source vs. clip behavior | `packages/flutter/lib/src/rendering/proxy_box.dart`, `_RenderCustomClip` |
| Opacity/compositing transitions and proxy setters | `packages/flutter/lib/src/rendering/proxy_box.dart` |

For every migrated setter not explicitly listed, locate and read its Flutter
counterpart before assigning the impact. Record any intentional FLUI
divergence in code documentation or an ADR rather than hiding it in a broader
constant.
