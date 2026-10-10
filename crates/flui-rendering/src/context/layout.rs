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
//!         ctx.position_child(i, Offset::new(0.0, y_offset));
//!         y_offset += child_size.height;
//!     }
//!
//!     // Return the computed size
//!     Size::new(constraints.max_width(), y_offset)
//! }
//! ```

use flui_foundation::Arity;
use flui_foundation::geometry::{Offset, Size};

use crate::{
    constraints::{BoxConstraints, Constraints, SliverConstraints, SliverGeometry},
    parent_data::ParentData,
    protocol::{BoxLayout, ChildLayout, LayoutCapability, LayoutContextApi, Protocol},
    storage::IntrinsicDimension,
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

impl<P: Protocol, A: Arity, PD: ParentData + Default> std::fmt::Debug
    for LayoutContext<'_, P, A, PD>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `inner` is a capability-GAT context (may hold live driver callbacks).
        f.debug_struct("LayoutContext").finish_non_exhaustive()
    }
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
    ) -> crate::error::RenderResult<<P::Layout as LayoutCapability>::Geometry> {
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
    ) -> crate::error::RenderResult<<P::Layout as LayoutCapability>::Geometry> {
        let geometry = self.inner.layout_child(index, constraints)?;
        self.inner.position_child(index, offset);
        Ok(geometry)
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
    ) -> crate::error::RenderResult<Vec<<P::Layout as LayoutCapability>::Geometry>>
    where
        <P::Layout as LayoutCapability>::Constraints: Clone,
    {
        let count = self.child_count();
        let mut geometries = Vec::with_capacity(count);
        for i in 0..count {
            geometries.push(self.inner.layout_child(i, constraints.clone())?);
        }
        Ok(geometries)
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
    /// Whether a descendant's layout was degraded during this node's pass —
    /// a layout that failed and handed its caller a stand-in, or a poisoned
    /// node that served one — at any depth.
    ///
    /// A parent that must not act on a stand-in asks before it commits: the
    /// viewports publish no scroll dimensions from a degraded pass, so a
    /// broken child cannot move the user's scroll offset. Contexts the
    /// production walk did not build (direct and test contexts) answer
    /// `false`.
    #[must_use]
    pub fn descendant_layout_degraded(&self) -> bool {
        crate::protocol::box_protocol::BoxLayoutCtxErased::descendant_layout_degraded(&self.inner)
    }

    /// The text context to measure with: the UI runtime's, lent through the
    /// pipeline for as long as the returned [`TextCx`](crate::TextCx)
    /// lives.
    ///
    /// Taken from `&mut self`, so a render object cannot lay out a child
    /// while it holds the loan.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::RenderError::TextContextBusy`] when an
    /// independently held loan already uses the runtime's shared resource.
    pub fn text(&mut self) -> crate::error::RenderResult<crate::TextCx<'_>> {
        self.inner.text()
    }

    // ════════════════════════════════════════════════════════════════════════
    // BOX CONSTRAINT HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the minimum width constraint.
    pub fn min_width(&self) -> f64 {
        self.inner.constraints().min_width
    }

    /// Gets the maximum width constraint.
    pub fn max_width(&self) -> f64 {
        self.inner.constraints().max_width
    }

    /// Gets the minimum height constraint.
    pub fn min_height(&self) -> f64 {
        self.inner.constraints().min_height
    }

    /// Gets the maximum height constraint.
    pub fn max_height(&self) -> f64 {
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
    pub fn tighten_width(&self, width: f64) -> BoxConstraints {
        self.inner.constraints().tighten(Some(width), None)
    }

    /// Returns constraints with only height tightened.
    pub fn tighten_height(&self, height: f64) -> BoxConstraints {
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
    pub fn constrain_width(&self, width: f64) -> f64 {
        self.inner.constraints().constrain_width(width)
    }

    /// Constrains only height.
    pub fn constrain_height(&self, height: f64) -> f64 {
        self.inner.constraints().constrain_height(height)
    }

    // ════════════════════════════════════════════════════════════════════════
    // BOX LAYOUT HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Layouts a single child with parent's constraints and returns size.
    pub fn layout_single_child(&mut self) -> crate::error::RenderResult<Size> {
        if self.child_count() > 0 {
            let constraints = *self.inner.constraints();
            self.inner.layout_child(0, constraints)
        } else {
            Ok(Size::ZERO)
        }
    }

    /// Layouts a single child with loosened constraints.
    pub fn layout_single_child_loose(&mut self) -> crate::error::RenderResult<Size> {
        if self.child_count() > 0 {
            let constraints = self.loosen();
            self.inner.layout_child(0, constraints)
        } else {
            Ok(Size::ZERO)
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
    /// `RenderViewport::perform_layout` is the primary consumer.
    pub fn layout_sliver_child(
        &mut self,
        index: usize,
        constraints: SliverConstraints,
    ) -> crate::error::RenderResult<SliverGeometry> {
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

    /// Whether the sliver child at `index` committed its geometry in a
    /// degraded pass — a descendant of it failed and its caller continued on
    /// a stand-in. A parent that caches child geometry must re-layout such a
    /// child rather than serve the cache, or the broken descendant is never
    /// walked and the pass looks healthy.
    #[must_use]
    pub fn sliver_child_geometry_degraded(&self, index: usize) -> bool {
        crate::protocol::box_protocol::BoxLayoutCtxErased::sliver_child_geometry_degraded(
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
    ) -> crate::error::RenderResult<Option<f64>> {
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
        extent: f64,
    ) -> crate::error::RenderResult<f64> {
        crate::protocol::box_protocol::BoxLayoutCtxErased::child_intrinsic(
            &mut self.inner,
            index,
            dimension,
            extent,
        )
    }

    /// Convenience: maximum intrinsic width of child `index` for the given
    /// `height` extent.  Returns `0.0` when the intrinsics callback is not wired.
    pub fn child_max_intrinsic_width(
        &mut self,
        index: usize,
        height: f64,
    ) -> crate::error::RenderResult<f64> {
        self.child_intrinsic(index, IntrinsicDimension::MaxWidth, height)
    }

    /// Convenience: minimum intrinsic width of child `index` for the given
    /// `height` extent.  Returns `0.0` when the intrinsics callback is not wired.
    pub fn child_min_intrinsic_width(
        &mut self,
        index: usize,
        height: f64,
    ) -> crate::error::RenderResult<f64> {
        self.child_intrinsic(index, IntrinsicDimension::MinWidth, height)
    }

    /// Convenience: maximum intrinsic height of child `index` for the given
    /// `width` extent.  Returns `0.0` when the intrinsics callback is not wired.
    pub fn child_max_intrinsic_height(
        &mut self,
        index: usize,
        width: f64,
    ) -> crate::error::RenderResult<f64> {
        self.child_intrinsic(index, IntrinsicDimension::MaxHeight, width)
    }

    /// Convenience: minimum intrinsic height of child `index` for the given
    /// `width` extent.  Returns `0.0` when the intrinsics callback is not wired.
    pub fn child_min_intrinsic_height(
        &mut self,
        index: usize,
        width: f64,
    ) -> crate::error::RenderResult<f64> {
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
    pub fn layout_box_child(
        &mut self,
        index: usize,
        constraints: BoxConstraints,
    ) -> crate::error::RenderResult<Size> {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::layout_box_child(
            &mut self.inner,
            index,
            constraints,
        )
    }

    /// Queries a **Box** child intrinsic dimension from a Sliver parent.
    pub fn box_child_intrinsic(
        &mut self,
        index: usize,
        dimension: IntrinsicDimension,
        extent: f64,
    ) -> crate::error::RenderResult<f64> {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::box_child_intrinsic(
            &mut self.inner,
            index,
            dimension,
            extent,
        )
    }

    /// Convenience wrapper for the child's maximum intrinsic height.
    pub fn box_child_max_intrinsic_height(
        &mut self,
        index: usize,
        width: f64,
    ) -> crate::error::RenderResult<f64> {
        self.box_child_intrinsic(index, IntrinsicDimension::MaxHeight, width)
    }

    /// Convenience wrapper for the child's maximum intrinsic width.
    pub fn box_child_max_intrinsic_width(
        &mut self,
        index: usize,
        height: f64,
    ) -> crate::error::RenderResult<f64> {
        self.box_child_intrinsic(index, IntrinsicDimension::MaxWidth, height)
    }

    /// Records a child-build request for `logical_index` under this sliver
    /// — the producer half of the request-strategy seam.
    ///
    /// No render object is supplied — the element tree decides what to build
    /// and where to insert it. The request is deposited into the arena's
    /// `pending_child_requests` sink; after the walk releases its borrows, the
    /// pipeline moves it to
    /// [`PipelineOwner::take_pending_child_requests`](crate::pipeline::PipelineOwner::take_pending_child_requests)
    /// for the binding layer, which services it inside the same frame's
    /// layout↔build fixpoint.
    ///
    /// Returns [`ChildLayout::Scheduled`] once the request is recorded, or
    /// [`ChildLayout::Unwired`] when this context carries no request sink.
    pub fn request_child_build(&mut self, logical_index: usize) -> ChildLayout {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::request_child_build(
            &mut self.inner,
            logical_index,
        )
    }

    /// Whether a descendant's layout was degraded during this node's pass
    /// (a failure turned into a stand-in, or a poisoned node served its
    /// stand-in), at any depth. See `DegradationProbe`.
    #[must_use]
    pub fn descendant_layout_degraded(&self) -> bool {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::descendant_layout_degraded(
            &self.inner,
        )
    }

    /// Emits the retained logical-index band `[first, last)` for this
    /// element-owned sliver.
    ///
    /// `RenderSliverList` calls this once per layout pass after
    /// `walk_virtualizer_band` returns. The pipeline moves the signal to
    /// `PipelineOwner::take_pending_retain_bands`; the binding layer drives
    /// `SparseChildren::retain_band` from it, evicting out-of-band lazy
    /// children on the element side. The render side never disposes a child
    /// itself, which is what avoids an ABA double-remove between the two.
    pub fn emit_retain_band(&mut self, first: usize, last: usize) {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::emit_retain_band(
            &mut self.inner,
            first,
            last,
        );
    }
}
