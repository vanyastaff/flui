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
    /// Returns a [`ChildLayout<BoxChildRef>`]: `Ready(handle)` when the child is
    /// laid out in this pass (the handle carries its id + size), `Scheduled` when
    /// queued for a later pass (the v1 next-frame backend), `NoChild` when `build`
    /// declines (end of an unknown-length source), or `Unwired` when this context
    /// has no build backend. `build(index)` is called at most once, only when a
    /// child must be created, and may return `None` to decline.
    pub fn build_and_layout_box_child(
        &mut self,
        index: usize,
        constraints: BoxConstraints,
        build: &mut dyn FnMut(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>>,
    ) -> ChildLayout<BoxChildRef> {
        crate::protocol::sliver_protocol::SliverLayoutCtxErased::build_and_layout_box_child(
            &mut self.inner,
            index,
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
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    #[test]
    fn test_layout_context_compiles() {
        // This test just verifies the module compiles — empty body is enough
        // because failure surfaces at `cargo build`, not at assert time.
    }
}
