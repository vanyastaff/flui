# Layout system architecture

The parent passes constraints down, the child returns geometry on success, and
the parent positions it. Layout is synchronous. Its context borrows only the
operations available to this node's protocol, arity and parent-data type.

The [crate architecture](../ARCHITECTURE.md) records invariants and their tests.
The public trait definitions are
[`RenderBox`](../src/traits/render_box.rs) and
[`RenderSliver`](../src/traits/render_sliver.rs); their blanket implementations
bridge typed callers to the erased protocol interfaces used by the pipeline.

## Constraints and geometry

Box layout receives [`BoxConstraints`](../src/constraints/box_constraints.rs)
and returns a logical `Size`. Tight constraints admit one size; loose constraints
allow the child to shrink. An unbounded axis is a measurement allowance, not
permission to publish infinite geometry. Logical geometry uses `f64`; conversions
to backend representations require their own validation.

Sliver layout receives
[`SliverConstraints`](../src/constraints/sliver_constraints.rs) and returns
[`SliverGeometry`](../src/constraints/sliver_geometry.rs). Its scroll, layout,
paint and cache extents describe different portions of the content. Use the
geometry constructor's derived defaults deliberately: spreading `ZERO` over a
partially specified result does not reproduce those defaults.

Both protocols return `RenderResult<Geometry>`. Rejected measurement is an error,
not zero geometry. A genuinely absent child can still produce a successful zero
size, and a node with no baseline can successfully return `None`.

## Authoring a box

`Arity` encodes child-count admission (`Leaf`, `Single`, `Variable` and bounded
forms). `ParentData` describes the metadata this parent stores on its children.
The corresponding [`BoxLayoutContext`](../src/context/layout.rs) supplies
constraints, child measurement, positioning and text access.

A leaf returns its constrained preferred size:

```rust
fn perform_layout(
    &mut self,
    ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
) -> RenderResult<Size> {
    Ok(ctx.constrain(self.preferred_size))
}
```

The complete leaf implementation in the [crate example](../src/lib.rs) is a
compiled doctest. Committed size belongs to pipeline storage; paint reads
`PaintCx::size()` instead of a second render-object copy.

A single-child parent propagates measurement before positioning:

```rust
fn perform_layout(
    &mut self,
    ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
) -> RenderResult<Size> {
    let child_size = ctx.layout_single_child_loose()?;
    ctx.position_single_child_at_origin();
    Ok(ctx.constrain(child_size))
}
```

For padding, derive deflated child constraints and reinflate the successful size;
the actual [`RenderPadding`](../../flui-objects/src/layout/padding.rs)
implementation demonstrates this. Variable-child containers use indexed
measurement and typed parent data. [`RenderFlex`](../../flui-objects/src/layout/flex.rs)
performs the allocation and positioning passes; it propagates rejected child
measurements instead of continuing with fabricated dimensions.

## Intrinsics, dry layout and baselines

Intrinsic queries ask for minimum or maximum width/height without committing a
layout. Dry layout asks for the size under supplied constraints. Baseline queries
return `Option<f64>` inside the result, distinguishing absence from failure.
Their contexts provide fallible child operations through the same pipeline.

[`PipelineOwner`](../src/pipeline/owner/query.rs) memoizes successful query
answers. Layout invalidation clears the dependent cache and can cross a relayout
boundary when an ancestor consumed an intrinsic or dry query. Returning a typed
error does not cache an absent baseline or a zero size in its place.

## Incremental layout and failure

Dirty entries are processed shallowest first. A parent can satisfy a queued
child during its own walk; the subsequent clean child entry is skipped. Clean
constraint-driven subtrees can reuse their accepted geometry. Relayout boundaries
isolate this work, while query dependencies can require ancestor invalidation.

An ordinary `TextLayoutError` propagates through box/sliver layout and geometry
queries. The pipeline retains the failing root and unprocessed dirty batch; the
runtime withholds scene submission and waits for changed input instead of
continuously retrying unchanged invalid text. A pending build or live layout
invalidation resumes it. Independent successful relayouts remain accepted.
This is not arbitrary render-object or whole-tree rollback.

Viewport child positions and content-derived metrics belong to their accepted
pass. Constraint-derived dimensions and their fractional-page mapping remain
accepted across an ordinary child failure; failed measurement cannot publish new
content dimensions. Genuine third-party panic/degraded-pass containment remains
separate. [ADR-0054](../../../docs/adr/ADR-0054-the-viewport-commits-one-result.md)
describes degraded viewport passes;
[ADR-0181](../../../docs/adr/ADR-0181-fallible-text-and-layout-measurement.md)
describes ordinary text rejection and recovery.

## Verifying a render object

Use the [render test harness](TESTING.md) through the public pipeline. Cover loose
and tight constraints, the object's intrinsic/dry/baseline answers, actual paint
and hit-test behavior, and rejection followed by recovery in the same tree.
Every concrete object participates in the catalog's `render_object_harness`.
A behavior fix also requires a case that fails when its production hunk is
reverted; a green geometry getter alone is not proof of the producer contract.
