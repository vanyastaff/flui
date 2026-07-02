//! Rich LayoutContext with ergonomic API for layout operations.
//!
//! This module provides `LayoutContext`, a high-level wrapper around the layout
//! capability traits that offers ergonomic APIs for common layout patterns.
//!
//! # Features
//!
//! - **Constraint Helpers**: Easy access to min/max sizes, tight/loose
//!   constraints
//! - **Child Layout**: Simplified child layout and positioning
//! - **Intrinsic Sizing**: Helper methods for intrinsic dimension queries
//! - **Debugging**: Layout debugging and visualization helpers
//!
//! # Example
//!
//! ```ignore
//! fn perform_layout(&mut self, ctx: &mut LayoutContext<BoxProtocol, Variable, BoxParentData>) {
//!     let constraints = ctx.constraints();
//!
//!     // Layout children with loose constraints
//!     let child_constraints = ctx.loosen();
//!     for i in 0..ctx.child_count() {
//!         let child_size = ctx.layout_child(i, child_constraints.clone());
//!         ctx.position_child(i, Offset::new(px(0.0), y_offset));
//!         y_offset += child_size.height;
//!     }
//!
//!     // Return the computed size
//!     Size::new(constraints.max_width(), y_offset)
//! }
//! ```

use flui_tree::Arity;
use flui_types::{Pixels, Size, geometry::Offset};

use crate::{
    constraints::{BoxConstraints, Constraints, SliverConstraints, SliverGeometry},
    parent_data::ParentData,
    protocol::{BoxChildRef, BoxLayout, ChildLayout, LayoutCapability, LayoutContextApi, Protocol},
    storage::IntrinsicDimension,
    traits::RenderObject,
};

// ============================================================================
// LAYOUT CONTEXT
// ============================================================================

/// Rich layout context with ergonomic API for common layout patterns.
///
/// This context wraps the underlying capability context and provides:
/// - Constraint manipulation helpers
/// - Child layout and positioning utilities
/// - Debugging aids
pub struct LayoutContext<'ctx, P: Protocol, A: Arity, PD: ParentData + Default> {
    /// The underlying layout context from the capability
    inner: <P::Layout as LayoutCapability>::Context<'ctx, A, PD>,
}

impl<'ctx, P: Protocol, A: Arity, PD: ParentData + Default> LayoutContext<'ctx, P, A, PD>
where
    <P::Layout as LayoutCapability>::Context<'ctx, A, PD>: LayoutContextApi<'ctx, P::Layout, A, PD>,
{
    /// Creates a new layout context wrapping the capability context.
    pub fn new(inner: <P::Layout as LayoutCapability>::Context<'ctx, A, PD>) -> Self {
        Self { inner }
    }

    // ════════════════════════════════════════════════════════════════════════
    // CONSTRAINT ACCESS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the layout constraints from parent.
    pub fn constraints(&self) -> &<P::Layout as LayoutCapability>::Constraints {
        self.inner.constraints()
    }

    // ════════════════════════════════════════════════════════════════════════
    // CHILD OPERATIONS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the number of children.
    pub fn child_count(&self) -> usize {
        self.inner.child_count()
    }

    /// Checks if there are any children.
    pub fn has_children(&self) -> bool {
        self.inner.child_count() > 0
    }

    /// Layouts a child with given constraints.
    pub fn layout_child(
        &mut self,
        index: usize,
        constraints: <P::Layout as LayoutCapability>::Constraints,
    ) -> <P::Layout as LayoutCapability>::Geometry {
        self.inner.layout_child(index, constraints)
    }

    /// Positions a child at the given offset.
    pub fn position_child(&mut self, index: usize, offset: Offset) {
        self.inner.position_child(index, offset);
    }

    /// Layouts and positions a child in one call.
    pub fn layout_and_position_child(
        &mut self,
        index: usize,
        constraints: <P::Layout as LayoutCapability>::Constraints,
        offset: Offset,
    ) -> <P::Layout as LayoutCapability>::Geometry {
        let geometry = self.inner.layout_child(index, constraints);
        self.inner.position_child(index, offset);
        geometry
    }

    /// Gets a child's current geometry (after layout).
    pub fn child_geometry(
        &self,
        index: usize,
    ) -> Option<&<P::Layout as LayoutCapability>::Geometry> {
        self.inner.child_geometry(index)
    }

    /// Gets a child's parent data.
    pub fn child_parent_data(&self, index: usize) -> Option<&PD> {
        self.inner.child_parent_data(index)
    }

    /// Gets mutable reference to child's parent data.
    pub fn child_parent_data_mut(&mut self, index: usize) -> Option<&mut PD> {
        self.inner.child_parent_data_mut(index)
    }

    // ════════════════════════════════════════════════════════════════════════
    // ITERATION HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Iterates over all children indices.
    pub fn children(&self) -> impl Iterator<Item = usize> {
        0..self.child_count()
    }

    /// Layouts all children with the same constraints.
    ///
    /// Returns a vector of geometries.
    pub fn layout_all_children(
        &mut self,
        constraints: <P::Layout as LayoutCapability>::Constraints,
    ) -> Vec<<P::Layout as LayoutCapability>::Geometry>
    where
        <P::Layout as LayoutCapability>::Constraints: Clone,
    {
        let count = self.child_count();
        let mut geometries = Vec::with_capacity(count);
        for i in 0..count {
            geometries.push(self.inner.layout_child(i, constraints.clone()));
        }
        geometries
    }

    // ════════════════════════════════════════════════════════════════════════
    // INNER ACCESS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the underlying context for advanced operations.
    pub fn inner(&self) -> &<P::Layout as LayoutCapability>::Context<'ctx, A, PD> {
        &self.inner
    }

    /// Gets mutable access to the underlying context.
    pub fn inner_mut(&mut self) -> &mut <P::Layout as LayoutCapability>::Context<'ctx, A, PD> {
        &mut self.inner
    }
}

// ============================================================================
// BOX-SPECIFIC EXTENSIONS
// ============================================================================

use crate::protocol::{BoxProtocol, SliverProtocol};

impl<'ctx, A: Arity, PD: ParentData + Default> LayoutContext<'ctx, BoxProtocol, A, PD>
where
    <BoxLayout as LayoutCapability>::Context<'ctx, A, PD>: LayoutContextApi<'ctx, BoxLayout, A, PD>,
{
    // ════════════════════════════════════════════════════════════════════════
    // BOX CONSTRAINT HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the minimum width constraint.
    pub fn min_width(&self) -> Pixels {
        self.inner.constraints().min_width
    }

    /// Gets the maximum width constraint.
    pub fn max_width(&self) -> Pixels {
        self.inner.constraints().max_width
    }

    /// Gets the minimum height constraint.
    pub fn min_height(&self) -> Pixels {
        self.inner.constraints().min_height
    }

    /// Gets the maximum height constraint.
    pub fn max_height(&self) -> Pixels {
        self.inner.constraints().max_height
    }

    /// Returns loosened constraints (min set to 0).
    pub fn loosen(&self) -> BoxConstraints {
        self.inner.constraints().loosen()
    }

    /// Returns tightened constraints (min set to max for both dimensions).
    pub fn tighten(&self) -> BoxConstraints {
        let c = self.inner.constraints();
        c.tighten(Some(c.max_width), Some(c.max_height))
    }

    /// Returns constraints with only width tightened.
    pub fn tighten_width(&self, width: Pixels) -> BoxConstraints {
        self.inner.constraints().tighten(Some(width), None)
    }

    /// Returns constraints with only height tightened.
    pub fn tighten_height(&self, height: Pixels) -> BoxConstraints {
        self.inner.constraints().tighten(None, Some(height))
    }

    /// Returns the smallest valid size.
    pub fn smallest(&self) -> Size {
        self.inner.constraints().smallest()
    }

    /// Returns the largest valid size.
    pub fn biggest(&self) -> Size {
        self.inner.constraints().biggest()
    }

    /// Checks if constraints are tight (exact size required).
    pub fn is_tight(&self) -> bool {
        self.inner.constraints().is_tight()
    }

    /// Checks if width is unbounded.
    pub fn has_unbounded_width(&self) -> bool {
        self.inner.constraints().max_width.is_infinite()
    }

    /// Checks if height is unbounded.
    pub fn has_unbounded_height(&self) -> bool {
        self.inner.constraints().max_height.is_infinite()
    }

    /// Constrains a size to these constraints.
    pub fn constrain(&self, size: Size) -> Size {
        self.inner.constraints().constrain(size)
    }

    /// Constrains only width.
    pub fn constrain_width(&self, width: Pixels) -> Pixels {
        self.inner.constraints().constrain_width(width)
    }

    /// Constrains only height.
    pub fn constrain_height(&self, height: Pixels) -> Pixels {
        self.inner.constraints().constrain_height(height)
    }

    // ════════════════════════════════════════════════════════════════════════
    // BOX LAYOUT HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Layouts a single child with parent's constraints and returns size.
    pub fn layout_single_child(&mut self) -> Size {
        if self.child_count() > 0 {
            let constraints = *self.inner.constraints();
            self.inner.layout_child(0, constraints)
        } else {
            Size::ZERO
        }
    }

    /// Layouts a single child with loosened constraints.
    pub fn layout_single_child_loose(&mut self) -> Size {
        if self.child_count() > 0 {
            let constraints = self.loosen();
            self.inner.layout_child(0, constraints)
        } else {
            Size::ZERO
        }
    }

    /// Positions the single child at origin.
    pub fn position_single_child_at_origin(&mut self) {
        if self.child_count() > 0 {
            self.inner.position_child(0, Offset::ZERO);
        }
    }
}

// ============================================================================
// BOX CROSS-PROTOCOL EXTENSIONS
// ============================================================================
//
// Separate impl block so the `BoxLayoutCtxErased` bound does not pollute
// the shared-method-name impl above (which would cause E0034 ambiguity
// between `BoxLayoutCtxErased::constraints` and
// `LayoutContextApi::constraints`).

impl<'ctx, A: Arity, PD: ParentData + Default> LayoutContext<'ctx, BoxProtocol, A, PD>
where
    <BoxLayout as LayoutCapability>::Context<'ctx, A, PD>:
        crate::protocol::box_protocol::BoxLayoutCtxErased,
{
    /// Lays out a **sliver** child at `index` with the given
    /// [`SliverConstraints`] and returns its [`SliverGeometry`].
    ///
    /// Delegates to
    /// [`crate::protocol::box_protocol::BoxLayoutCtxErased::layout_sliver_child`]
    /// on the underlying context. In Direct-storage contexts (leaf-only
    /// layout, unit tests without a pipeline-wired sliver callback) the
    /// underlying impl returns [`SliverGeometry::ZERO`]. In the production
    /// pipeline-driven Proxy context the call drives
    /// `layout_sliver_subtree_borrowed` on the pre-acquired sliver-child slot.
    ///
    /// `RenderViewport::perform_layout` (next PR) is the primary consumer.
    pub fn layout_sliver_child(
        &mut self,
        index: usize,
        constraints: SliverConstraints,
    ) -> SliverGeometry {
        crate::protocol::box_protocol::BoxLayoutCtxErased::layout_sliver_child(
            &mut self.inner,
            index,
            constraints,
        )
    }

    /// Returns the last known sliver constraints and geometry for a sliver
    /// child, when the production pipeline has cached them.
    pub fn cached_sliver_child_layout(
        &self,
        index: usize,
    ) -> Option<(SliverConstraints, SliverGeometry)> {
        crate::protocol::box_protocol::BoxLayoutCtxErased::cached_sliver_child_layout(
            &self.inner,
            index,
        )
    }

    /// Returns whether a sliver child is still marked as needing layout.
    pub fn sliver_child_needs_layout(&self, index: usize) -> bool {
        crate::protocol::box_protocol::BoxLayoutCtxErased::sliver_child_needs_layout(
            &self.inner,
            index,
        )
    }

    /// Distance from the top of child `index` to its first baseline of
    /// `baseline` kind, after the child has been laid out in this walk.
    pub fn child_distance_to_actual_baseline(
        &self,
        index: usize,
        baseline: crate::traits::TextBaseline,
    ) -> Option<f32> {
        crate::protocol::box_protocol::BoxLayoutCtxErased::child_distance_to_actual_baseline(
            &self.inner,
            index,
            baseline,
        )
    }

    /// Queries a child's intrinsic dimension from within `perform_layout`.
    ///
    /// On the production pipeline path the call is routed through
    /// `box_intrinsic_query_borrowed` (the same pre-acquired subtree pool used
    /// by the Sliver→Box intrinsic path).  On Direct-storage / test contexts
    /// where no callback is wired, returns `0.0` — the same conservative
    /// fallback as `layout_child` returning `Size::ZERO`.
    ///
    /// Used by `RenderIntrinsicWidth` / `RenderIntrinsicHeight` to measure the
    /// child's preferred extent before committing to a layout size.
    pub fn child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        crate::protocol::box_protocol::BoxLayoutCtxErased::child_intrinsic(
            &mut self.inner,
            index,
            dimension,
            extent,
        )
    }

    /// Convenience: maximum intrinsic width of child `index` for the given
    /// `height` extent.  Returns `0.0` when the intrinsics callback is not wired.
    pub fn child_max_intrinsic_width(&mut self, index: usize, height: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MaxWidth, height)
    }

    /// Convenience: minimum intrinsic width of child `index` for the given
    /// `height` extent.  Returns `0.0` when the intrinsics callback is not wired.
    pub fn child_min_intrinsic_width(&mut self, index: usize, height: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MinWidth, height)
    }

    /// Convenience: maximum intrinsic height of child `index` for the given
    /// `width` extent.  Returns `0.0` when the intrinsics callback is not wired.
    pub fn child_max_intrinsic_height(&mut self, index: usize, width: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MaxHeight, width)
    }

    /// Convenience: minimum intrinsic height of child `index` for the given
    /// `width` extent.  Returns `0.0` when the intrinsics callback is not wired.
    pub fn child_min_intrinsic_height(&mut self, index: usize, width: f32) -> f32 {
        self.child_intrinsic(index, IntrinsicDimension::MinHeight, width)
    }
}

// ============================================================================
// SLIVER CROSS-PROTOCOL EXTENSIONS
// ============================================================================

impl<'ctx, A: Arity, PD: ParentData + Default> LayoutContext<'ctx, SliverProtocol, A, PD>
where
    <crate::protocol::SliverLayout as LayoutCapability>::Context<'ctx, A, PD>:
        crate::protocol::sliver_protocol::SliverLayoutCtxErased,
{
    /// Lays out a **Box** child at `index` with the given
    /// [`BoxConstraints`] and returns its [`Size`].
    ///
    /// This is the reverse bridge of
    /// [`Self::layout_sliver_child`]: Sliver render objects such as
    /// `RenderSliverToBoxAdapter` can host Box children and still drive the
    /// normal Box subtree layout walk through the pipeline.
    pub fn layout_box_child(&mut self, index: usize, constraints: BoxConstraints) -> Size {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::layout_box_child(
            &mut self.inner,
            index,
            constraints,
        )
    }

    /// On-demand build + layout of a **Box** child at `index`, materializing it
    /// via `build` when it does not yet exist — the re-entrant build contract
    /// (ADR-0003 Decision 2). A lazy sliver (e.g. a virtualized `SliverList`)
    /// drives this during its own layout to build only the visible-plus-cache
    /// band rather than every child up front.
    ///
    /// `logical_index` is the item index in the data source (e.g. position in the
    /// virtual list). Distinct from `index` (the dense child-slot). The backend
    /// stamps `logical_index` into the freshly-inserted child's parent-data so
    /// the consumer can reconcile it on the next pass.
    ///
    /// Returns a [`ChildLayout<BoxChildRef>`]: `Ready(handle)` when the child is
    /// laid out in this pass (the handle carries its id + size), `Scheduled` when
    /// queued for a later pass (the v1 next-frame backend), `NoChild` when `build`
    /// declines (end of an unknown-length source), or `Unwired` when this context
    /// has no build backend. `build(index)` is called at most once, only when a
    /// child must be created, and may return `None` to decline.
    pub fn build_and_layout_box_child(
        &mut self,
        index: usize,
        logical_index: usize,
        constraints: BoxConstraints,
        build: &mut dyn FnMut(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>>,
    ) -> ChildLayout<BoxChildRef> {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::build_and_layout_box_child(
            &mut self.inner,
            index,
            logical_index,
            constraints,
            build,
        )
    }

    /// Queries a **Box** child intrinsic dimension from a Sliver parent.
    pub fn box_child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f32,
    ) -> f32 {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::box_child_intrinsic(
            &mut self.inner,
            index,
            dimension,
            extent,
        )
    }

    /// Convenience wrapper for the child's maximum intrinsic height.
    pub fn box_child_max_intrinsic_height(&mut self, index: usize, width: f32) -> f32 {
        self.box_child_intrinsic(index, IntrinsicDimension::MaxHeight, width)
    }

    /// Convenience wrapper for the child's maximum intrinsic width.
    pub fn box_child_max_intrinsic_width(&mut self, index: usize, height: f32) -> f32 {
        self.box_child_intrinsic(index, IntrinsicDimension::MaxWidth, height)
    }

    /// Enqueues a deferred removal for the Box child with the given
    /// [`RenderId`](flui_foundation::RenderId). Applied after the current layout
    /// walk releases its borrows (same discipline as
    /// [`Self::build_and_layout_box_child`]). No-op when the context carries no
    /// remove sink (Direct storage / test contexts).
    pub fn dispose_box_child(&mut self, id: flui_foundation::RenderId) {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::dispose_box_child(
            &mut self.inner,
            id,
        )
    }

    /// Returns the [`RenderId`](flui_foundation::RenderId) of the Box child at
    /// dense slot `index`, if it exists. Used together with
    /// [`Self::dispose_box_child`] to evict off-band children by id.
    pub fn child_id(&self, index: usize) -> Option<flui_foundation::RenderId> {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::child_id(&self.inner, index)
    }

    /// Records a child-build request for `logical_index` under this sliver
    /// (U4.2 request-strategy seam).
    ///
    /// Unlike [`Self::build_and_layout_box_child`], no render object is
    /// supplied — the element tree (U4.3) decides what to build and where to
    /// insert it.  The request is deposited into the arena's
    /// `pending_child_requests` sink; after the walk releases its borrows,
    /// the pipeline moves it to
    /// [`PipelineOwner::take_pending_child_requests`](crate::pipeline::PipelineOwner::take_pending_child_requests)
    /// for the binding layer.
    ///
    /// Return type is [`ChildLayout<BoxChildRef>`]: `Scheduled` in v1
    /// (next-frame policy); a true-mid-pass backend may return
    /// `Ready(BoxChildRef)` without a breaking change — ADR-0003 Decision 2(c)
    /// forbids narrowing the return type to `Infallible`/`Scheduled` only.
    /// Returns `Unwired` when this context carries no request sink.
    pub fn request_child_build(
        &mut self,
        logical_index: usize,
    ) -> crate::protocol::ChildLayout<crate::protocol::BoxChildRef> {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::request_child_build(
            &mut self.inner,
            logical_index,
        )
    }

    /// Emits the retained logical-index band `[first, last)` for this
    /// element-owned sliver.
    ///
    /// `RenderSliverList` calls this once per layout pass after
    /// `walk_virtualizer_band` returns. The pipeline moves the signal to
    /// `PipelineOwner::take_pending_retain_bands`; the binding layer (U4.3)
    /// drives `SparseChildren::retain_band` from it, evicting out-of-band lazy
    /// children on the element side and bypassing `dispose_box_child` to avoid
    /// the ABA double-remove.
    pub fn emit_retain_band(&mut self, first: usize, last: usize) {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::emit_retain_band(
            &mut self.inner,
            first,
            last,
        )
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_foundation::RenderId;
    use flui_tree::Leaf;
    use flui_types::geometry::px;

    use super::*;
    use crate::parent_data::{BoxParentData, SliverParentData};
    use crate::protocol::box_protocol::{BoxLayoutCtx, ChildState};
    use crate::protocol::sliver_protocol::SliverLayoutCtx;

    #[test]
    fn test_layout_context_compiles() {
        // This test just verifies the module compiles — empty body is enough
        // because failure surfaces at `cargo build`, not at assert time.
    }

    fn box_constraints() -> BoxConstraints {
        BoxConstraints::new(px(10.0), px(200.0), px(20.0), px(100.0))
    }

    // ------------------------------------------------------------------
    // Generic LayoutContext<P, A, PD> forwarding
    // ------------------------------------------------------------------

    #[test]
    fn generic_layout_context_forwards_constraints_and_child_presence() {
        let ctx: LayoutContext<'_, BoxProtocol, Leaf, BoxParentData> =
            LayoutContext::new(BoxLayoutCtx::new(box_constraints()));

        assert_eq!(*ctx.constraints(), box_constraints());
        assert_eq!(ctx.child_count(), 0);
        assert!(!ctx.has_children());
        assert_eq!(ctx.children().collect::<Vec<_>>(), Vec::<usize>::new());
    }

    #[test]
    fn generic_layout_context_child_dispatch_and_write_through() {
        let child_a = RenderId::new(1);
        let child_b = RenderId::new(2);
        let child_ids = [child_a, child_b];
        let mut children = vec![
            ChildState::<BoxParentData>::new(child_a),
            ChildState::<BoxParentData>::new(child_b),
        ];
        let layout_child_callback = |id: RenderId, _c: BoxConstraints| -> Size {
            if id == child_a {
                Size::new(px(11.0), px(22.0))
            } else {
                Size::new(px(33.0), px(44.0))
            }
        };

        let mut ctx: LayoutContext<'_, BoxProtocol, Leaf, BoxParentData> =
            LayoutContext::new(BoxLayoutCtx::with_layout_callback(
                box_constraints(),
                &mut children,
                &child_ids,
                &layout_child_callback,
            ));

        assert!(ctx.has_children());
        assert_eq!(ctx.child_count(), 2);

        let size = ctx.layout_child(0, box_constraints());
        assert_eq!(size, Size::new(px(11.0), px(22.0)));
        assert_eq!(ctx.child_geometry(0), Some(&Size::new(px(11.0), px(22.0))));

        ctx.position_child(0, Offset::new(px(1.0), px(2.0)));

        let geom =
            ctx.layout_and_position_child(1, box_constraints(), Offset::new(px(3.0), px(4.0)));
        assert_eq!(geom, Size::new(px(33.0), px(44.0)));

        ctx.child_parent_data_mut(0).unwrap().offset = Offset::new(px(9.0), px(9.0));
        assert_eq!(
            ctx.child_parent_data(0).unwrap().offset,
            Offset::new(px(9.0), px(9.0))
        );

        let all = ctx.layout_all_children(box_constraints());
        assert_eq!(all.len(), 2);

        assert_eq!(ctx.inner().child_count(), 2);
        assert_eq!(LayoutContextApi::child_count(ctx.inner_mut()), 2);

        drop(ctx);
        // The write-through offset must have reached the backing storage.
        assert_eq!(children[0].offset, Offset::new(px(1.0), px(2.0)));
        assert_eq!(children[1].offset, Offset::new(px(3.0), px(4.0)));
    }

    // ------------------------------------------------------------------
    // Box-specific constraint helpers
    // ------------------------------------------------------------------

    #[test]
    fn box_constraint_accessors_and_bounds_checks() {
        let ctx: LayoutContext<'_, BoxProtocol, Leaf, BoxParentData> =
            LayoutContext::new(BoxLayoutCtx::new(box_constraints()));

        assert_eq!(ctx.min_width(), px(10.0));
        assert_eq!(ctx.max_width(), px(200.0));
        assert_eq!(ctx.min_height(), px(20.0));
        assert_eq!(ctx.max_height(), px(100.0));
        assert!(!ctx.is_tight());
        assert!(!ctx.has_unbounded_width());
        assert!(!ctx.has_unbounded_height());

        assert_eq!(ctx.smallest(), Size::new(px(10.0), px(20.0)));
        assert_eq!(ctx.biggest(), Size::new(px(200.0), px(100.0)));
    }

    #[test]
    fn box_constraint_transforms_loosen_and_tighten() {
        let ctx: LayoutContext<'_, BoxProtocol, Leaf, BoxParentData> =
            LayoutContext::new(BoxLayoutCtx::new(box_constraints()));

        let loosened = ctx.loosen();
        assert_eq!(loosened.min_width, px(0.0));
        assert_eq!(loosened.min_height, px(0.0));
        assert_eq!(loosened.max_width, px(200.0));

        let tightened = ctx.tighten();
        assert!(tightened.is_tight());
        assert_eq!(tightened.min_width, px(200.0));
        assert_eq!(tightened.min_height, px(100.0));

        let width_tight = ctx.tighten_width(px(50.0));
        assert_eq!(width_tight.min_width, px(50.0));
        assert_eq!(width_tight.max_width, px(50.0));
        assert_eq!(width_tight.min_height, px(20.0), "height untouched");

        let height_tight = ctx.tighten_height(px(60.0));
        assert_eq!(height_tight.min_height, px(60.0));
        assert_eq!(height_tight.max_height, px(60.0));
        assert_eq!(height_tight.min_width, px(10.0), "width untouched");
    }

    #[test]
    fn box_constraint_clamp_helpers() {
        let ctx: LayoutContext<'_, BoxProtocol, Leaf, BoxParentData> =
            LayoutContext::new(BoxLayoutCtx::new(box_constraints()));

        assert_eq!(
            ctx.constrain(Size::new(px(5.0), px(500.0))),
            Size::new(px(10.0), px(100.0)),
            "clamps below-min width up and above-max height down"
        );
        assert_eq!(ctx.constrain_width(px(500.0)), px(200.0));
        assert_eq!(ctx.constrain_height(px(1.0)), px(20.0));
    }

    // ------------------------------------------------------------------
    // Box-specific single-child layout helpers
    // ------------------------------------------------------------------

    #[test]
    fn layout_single_child_helpers_are_zero_size_and_noop_without_children() {
        let mut ctx: LayoutContext<'_, BoxProtocol, Leaf, BoxParentData> =
            LayoutContext::new(BoxLayoutCtx::new(box_constraints()));

        assert_eq!(ctx.layout_single_child(), Size::ZERO);
        assert_eq!(ctx.layout_single_child_loose(), Size::ZERO);
        ctx.position_single_child_at_origin(); // must not panic with no children
    }

    #[test]
    fn layout_single_child_lays_out_and_positions_the_first_child() {
        let child_id = RenderId::new(1);
        let child_ids = [child_id];
        let mut children = vec![ChildState::<BoxParentData>::new(child_id)];
        let layout_child_callback = |_id: RenderId, c: BoxConstraints| -> Size { c.biggest() };

        let mut ctx: LayoutContext<'_, BoxProtocol, Leaf, BoxParentData> =
            LayoutContext::new(BoxLayoutCtx::with_layout_callback(
                box_constraints(),
                &mut children,
                &child_ids,
                &layout_child_callback,
            ));

        assert_eq!(ctx.layout_single_child(), box_constraints().biggest());
        ctx.position_single_child_at_origin();

        drop(ctx);
        assert_eq!(children[0].offset, Offset::ZERO);
    }

    #[test]
    fn layout_single_child_loose_lays_out_under_loosened_constraints() {
        let child_id = RenderId::new(1);
        let child_ids = [child_id];
        let mut children = vec![ChildState::<BoxParentData>::new(child_id)];
        let layout_child_callback = |_id: RenderId, c: BoxConstraints| -> Size { c.smallest() };

        let mut ctx: LayoutContext<'_, BoxProtocol, Leaf, BoxParentData> =
            LayoutContext::new(BoxLayoutCtx::with_layout_callback(
                box_constraints(),
                &mut children,
                &child_ids,
                &layout_child_callback,
            ));

        // Loosened min is zero, so `smallest()` under the loosened
        // constraints must be Size::ZERO -- distinct from the un-loosened
        // constraints' smallest() of (10, 20).
        assert_eq!(ctx.layout_single_child_loose(), Size::ZERO);
    }

    // ------------------------------------------------------------------
    // Box cross-protocol extensions (Direct-mode fallback contract)
    // ------------------------------------------------------------------

    #[test]
    fn box_cross_protocol_direct_mode_fallbacks() {
        let mut ctx: LayoutContext<'_, BoxProtocol, Leaf, BoxParentData> =
            LayoutContext::new(BoxLayoutCtx::new(box_constraints()));

        assert_eq!(
            ctx.layout_sliver_child(0, SliverConstraints::default()),
            SliverGeometry::ZERO
        );
        assert_eq!(ctx.cached_sliver_child_layout(0), None);
        assert!(
            ctx.sliver_child_needs_layout(0),
            "Direct-mode conservatively reports needs-layout=true"
        );
        assert_eq!(
            ctx.child_distance_to_actual_baseline(0, crate::traits::TextBaseline::Alphabetic),
            None
        );
        assert_eq!(
            ctx.child_intrinsic(0, IntrinsicDimension::MinWidth, 10.0),
            0.0
        );
        assert_eq!(ctx.child_max_intrinsic_width(0, 10.0), 0.0);
        assert_eq!(ctx.child_min_intrinsic_width(0, 10.0), 0.0);
        assert_eq!(ctx.child_max_intrinsic_height(0, 10.0), 0.0);
        assert_eq!(ctx.child_min_intrinsic_height(0, 10.0), 0.0);
    }

    // ------------------------------------------------------------------
    // Sliver cross-protocol extensions (Direct-mode fallback contract)
    // ------------------------------------------------------------------

    #[test]
    fn sliver_cross_protocol_direct_mode_fallbacks() {
        let mut ctx: LayoutContext<'_, SliverProtocol, Leaf, SliverParentData> =
            LayoutContext::new(SliverLayoutCtx::new(SliverConstraints::default()));

        assert_eq!(
            ctx.layout_box_child(0, BoxConstraints::tight(Size::ZERO)),
            Size::ZERO
        );
        assert_eq!(
            ctx.box_child_intrinsic(0, IntrinsicDimension::MaxHeight, 5.0),
            0.0
        );
        assert_eq!(ctx.box_child_max_intrinsic_height(0, 5.0), 0.0);
        assert_eq!(ctx.box_child_max_intrinsic_width(0, 5.0), 0.0);
        assert_eq!(ctx.child_id(0), None);

        // No-op / no-panic without a wired backend.
        ctx.dispose_box_child(RenderId::new(1));
        ctx.emit_retain_band(0, 1);

        let mut never_builds = |_idx: usize| -> Option<Box<dyn RenderObject<BoxProtocol>>> {
            panic!("no build backend is wired -- this must not run")
        };
        assert_eq!(
            ctx.build_and_layout_box_child(
                0,
                0,
                BoxConstraints::tight(Size::ZERO),
                &mut never_builds
            ),
            ChildLayout::Unwired
        );
        assert_eq!(ctx.request_child_build(0), ChildLayout::Unwired);
    }
}
