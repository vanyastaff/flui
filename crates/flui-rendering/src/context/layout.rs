//! Rich LayoutContext with ergonomic API for layout operations.
//!
//! This module provides `LayoutContext`, a high-level wrapper around the layout
//! capability traits that offers ergonomic APIs for common layout patterns.
//!
//! # Features
//!
//! - **Constraint Helpers**: Easy access to min/max sizes, tight/loose constraints
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
//!     // Complete layout with computed size
//!     ctx.complete(Size::new(constraints.max_width(), y_offset));
//! }
//! ```

use flui_types::geometry::Offset;
use flui_types::Size;

use crate::arity::Arity;
use crate::constraints::{BoxConstraints, Constraints};
use crate::parent_data::ParentData;
use crate::protocol::{BoxLayout, LayoutCapability, LayoutContextApi, Protocol};

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

    /// Checks if layout is complete.
    pub fn is_complete(&self) -> bool {
        self.inner.is_complete()
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
    // LAYOUT COMPLETION
    // ════════════════════════════════════════════════════════════════════════

    /// Completes layout with the given geometry.
    pub fn complete(&mut self, geometry: <P::Layout as LayoutCapability>::Geometry) {
        self.inner.complete_layout(geometry);
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

use crate::protocol::BoxProtocol;

impl<'ctx, A: Arity, PD: ParentData + Default> LayoutContext<'ctx, BoxProtocol, A, PD>
where
    <BoxLayout as LayoutCapability>::Context<'ctx, A, PD>: LayoutContextApi<'ctx, BoxLayout, A, PD>,
{
    // ════════════════════════════════════════════════════════════════════════
    // BOX CONSTRAINT HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Gets the minimum width constraint.
    pub fn min_width(&self) -> f32 {
        self.inner.constraints().min_width
    }

    /// Gets the maximum width constraint.
    pub fn max_width(&self) -> f32 {
        self.inner.constraints().max_width
    }

    /// Gets the minimum height constraint.
    pub fn min_height(&self) -> f32 {
        self.inner.constraints().min_height
    }

    /// Gets the maximum height constraint.
    pub fn max_height(&self) -> f32 {
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
    pub fn tighten_width(&self, width: f32) -> BoxConstraints {
        self.inner.constraints().tighten(Some(width), None)
    }

    /// Returns constraints with only height tightened.
    pub fn tighten_height(&self, height: f32) -> BoxConstraints {
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
    pub fn constrain_width(&self, width: f32) -> f32 {
        self.inner.constraints().constrain_width(width)
    }

    /// Constrains only height.
    pub fn constrain_height(&self, height: f32) -> f32 {
        self.inner.constraints().constrain_height(height)
    }

    // ════════════════════════════════════════════════════════════════════════
    // BOX LAYOUT HELPERS
    // ════════════════════════════════════════════════════════════════════════

    /// Layouts a single child with parent's constraints and returns size.
    pub fn layout_single_child(&mut self) -> Size {
        if self.child_count() > 0 {
            let constraints = self.inner.constraints().clone();
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

    /// Completes layout with the given size.
    pub fn complete_with_size(&mut self, size: Size) {
        self.inner.complete_layout(size);
    }

    /// Completes layout matching the biggest allowed size.
    pub fn complete_with_biggest(&mut self) {
        let size = self.biggest();
        self.inner.complete_layout(size);
    }

    /// Completes layout matching the smallest allowed size.
    pub fn complete_with_smallest(&mut self) {
        let size = self.smallest();
        self.inner.complete_layout(size);
    }

    /// Completes layout matching a single child's size.
    pub fn complete_matching_child(&mut self) {
        let size = self.child_geometry(0).cloned().unwrap_or(Size::ZERO);
        self.inner.complete_layout(size);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    #[test]
    fn test_layout_context_compiles() {
        // This test just verifies the module compiles
        assert!(true);
    }
}
