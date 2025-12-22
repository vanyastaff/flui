//! Layout capability trait for protocol composition.
//!
//! This module defines the `LayoutCapability` trait which groups:
//! - Constraints (input from parent)
//! - Geometry (output to parent)
//! - LayoutContext (GAT for layout operations)

use crate::arity::Arity;
use crate::constraints::Constraints;
use crate::parent_data::ParentData;
use std::fmt::Debug;
use std::hash::Hash;

// ============================================================================
// LAYOUT CAPABILITY
// ============================================================================

/// Capability trait for layout operations.
///
/// Groups all types related to layout: constraints from parent,
/// geometry output, and the layout context GAT.
///
/// # Type Parameters
///
/// - `Constraints`: Layout input from parent (must be hashable for caching)
/// - `Geometry`: Layout output returned to parent
/// - `Context<'ctx, A, P>`: Layout context with lifetime, arity, and parent data
///
/// # Example
///
/// ```ignore
/// pub struct BoxLayout;
///
/// impl LayoutCapability for BoxLayout {
///     type Constraints = BoxConstraints;
///     type Geometry = Size;
///     type Context<'ctx, A: Arity, P: ParentData> = BoxLayoutContext<'ctx, A, P>
///     where Self: 'ctx;
///
///     fn default_geometry() -> Self::Geometry {
///         Size::ZERO
///     }
/// }
/// ```
pub trait LayoutCapability: Send + Sync + 'static {
    /// Layout constraints from parent.
    ///
    /// Must be hashable for layout caching.
    type Constraints: Clone + Debug + Send + Sync + Hash + Eq + 'static;

    /// Layout geometry output.
    ///
    /// Returned to parent after layout completes.
    type Geometry: Clone + Debug + Default + Send + Sync + 'static;

    /// Layout context with Generic Associated Type.
    ///
    /// Provides access to constraints, child layout, and positioning.
    type Context<'ctx, A: Arity, P: ParentData>: LayoutContextApi<'ctx, Self, A, P>
    where
        Self: 'ctx;

    /// Returns the default geometry for uninitialized state.
    fn default_geometry() -> Self::Geometry {
        Self::Geometry::default()
    }

    /// Validates constraints before layout.
    ///
    /// Returns `true` if constraints are valid, `false` otherwise.
    fn validate_constraints(_constraints: &Self::Constraints) -> bool {
        true // Default: accept all
    }

    /// Normalizes constraints for consistent cache keys.
    ///
    /// Handles float precision issues for caching.
    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints // Default: no normalization
    }
}

// ============================================================================
// LAYOUT CONTEXT API
// ============================================================================

/// API for layout context operations.
///
/// Provides access to constraints and child layout operations.
/// Implemented by protocol-specific layout contexts.
pub trait LayoutContextApi<'ctx, L: LayoutCapability + ?Sized, A: Arity, P: ParentData>:
    Send + Sync
{
    /// Gets the layout constraints from parent.
    fn constraints(&self) -> &L::Constraints;

    /// Checks if layout is complete.
    fn is_complete(&self) -> bool;

    /// Marks layout complete with final geometry.
    fn complete_layout(&mut self, geometry: L::Geometry);

    /// Gets the number of children.
    fn child_count(&self) -> usize;

    /// Layouts a child with given constraints.
    ///
    /// Returns the child's geometry after layout.
    fn layout_child(&mut self, index: usize, constraints: L::Constraints) -> L::Geometry;

    /// Positions a child at the given offset.
    fn position_child(&mut self, index: usize, offset: flui_types::Offset);

    /// Gets a child's current geometry (after layout).
    fn child_geometry(&self, index: usize) -> Option<&L::Geometry>;

    /// Gets a child's parent data.
    fn child_parent_data(&self, index: usize) -> Option<&P>;

    /// Gets mutable reference to child's parent data.
    fn child_parent_data_mut(&mut self, index: usize) -> Option<&mut P>;
}

// ============================================================================
// BOX LAYOUT CAPABILITY
// ============================================================================

use crate::constraints::BoxConstraints;
use flui_types::Size;

/// Layout capability for box (2D) layout.
///
/// Uses `BoxConstraints` for input and `Size` for output.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxLayout;

impl LayoutCapability for BoxLayout {
    type Constraints = BoxConstraints;
    type Geometry = Size;
    type Context<'ctx, A: Arity, P: ParentData>
        = BoxLayoutCtx<'ctx, A, P>
    where
        Self: 'ctx;

    fn default_geometry() -> Self::Geometry {
        Size::ZERO
    }

    fn validate_constraints(constraints: &Self::Constraints) -> bool {
        constraints.is_normalized()
    }

    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints.normalize()
    }
}

/// Box layout context implementation.
///
/// Provides layout operations for box protocol.
pub struct BoxLayoutCtx<'ctx, A: Arity, P: ParentData> {
    constraints: BoxConstraints,
    geometry: Option<Size>,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<'ctx, A: Arity, P: ParentData> BoxLayoutCtx<'ctx, A, P> {
    /// Creates a new box layout context with given constraints.
    pub fn new(constraints: BoxConstraints) -> Self {
        Self {
            constraints,
            geometry: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<'ctx, A: Arity, P: ParentData> LayoutContextApi<'ctx, BoxLayout, A, P>
    for BoxLayoutCtx<'ctx, A, P>
{
    fn constraints(&self) -> &BoxConstraints {
        &self.constraints
    }

    fn is_complete(&self) -> bool {
        self.geometry.is_some()
    }

    fn complete_layout(&mut self, geometry: Size) {
        self.geometry = Some(geometry);
    }

    fn child_count(&self) -> usize {
        0 // Override in actual implementation
    }

    fn layout_child(&mut self, _index: usize, _constraints: BoxConstraints) -> Size {
        Size::ZERO // Override in actual implementation
    }

    fn position_child(&mut self, _index: usize, _offset: flui_types::Offset) {
        // Override in actual implementation
    }

    fn child_geometry(&self, _index: usize) -> Option<&Size> {
        None // Override in actual implementation
    }

    fn child_parent_data(&self, _index: usize) -> Option<&P> {
        None // Override in actual implementation
    }

    fn child_parent_data_mut(&mut self, _index: usize) -> Option<&mut P> {
        None // Override in actual implementation
    }
}

// ============================================================================
// SLIVER LAYOUT CAPABILITY
// ============================================================================

use crate::constraints::{SliverConstraints, SliverGeometry};

/// Layout capability for sliver (scrollable) layout.
///
/// Uses `SliverConstraints` for input and `SliverGeometry` for output.
#[derive(Debug, Clone, Copy, Default)]
pub struct SliverLayout;

impl LayoutCapability for SliverLayout {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
    type Context<'ctx, A: Arity, P: ParentData>
        = SliverLayoutCtx<'ctx, A, P>
    where
        Self: 'ctx;

    fn default_geometry() -> Self::Geometry {
        SliverGeometry::ZERO
    }

    fn validate_constraints(constraints: &Self::Constraints) -> bool {
        constraints.is_normalized()
    }

    fn normalize_constraints(constraints: Self::Constraints) -> Self::Constraints {
        constraints.normalize()
    }
}

/// Sliver layout context implementation.
///
/// Provides layout operations for sliver protocol.
pub struct SliverLayoutCtx<'ctx, A: Arity, P: ParentData> {
    constraints: SliverConstraints,
    geometry: Option<SliverGeometry>,
    _phantom: std::marker::PhantomData<(&'ctx (), A, P)>,
}

impl<'ctx, A: Arity, P: ParentData> SliverLayoutCtx<'ctx, A, P> {
    /// Creates a new sliver layout context with given constraints.
    pub fn new(constraints: SliverConstraints) -> Self {
        Self {
            constraints,
            geometry: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<'ctx, A: Arity, P: ParentData> LayoutContextApi<'ctx, SliverLayout, A, P>
    for SliverLayoutCtx<'ctx, A, P>
{
    fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    fn is_complete(&self) -> bool {
        self.geometry.is_some()
    }

    fn complete_layout(&mut self, geometry: SliverGeometry) {
        self.geometry = Some(geometry);
    }

    fn child_count(&self) -> usize {
        0 // Override in actual implementation
    }

    fn layout_child(&mut self, _index: usize, _constraints: SliverConstraints) -> SliverGeometry {
        SliverGeometry::ZERO // Override in actual implementation
    }

    fn position_child(&mut self, _index: usize, _offset: flui_types::Offset) {
        // Override in actual implementation
    }

    fn child_geometry(&self, _index: usize) -> Option<&SliverGeometry> {
        None // Override in actual implementation
    }

    fn child_parent_data(&self, _index: usize) -> Option<&P> {
        None // Override in actual implementation
    }

    fn child_parent_data_mut(&mut self, _index: usize) -> Option<&mut P> {
        None // Override in actual implementation
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_layout_default_geometry() {
        let size = BoxLayout::default_geometry();
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn test_sliver_layout_default_geometry() {
        let geometry = SliverLayout::default_geometry();
        assert_eq!(geometry, SliverGeometry::ZERO);
    }

    #[test]
    fn test_box_constraints_validation() {
        let valid = BoxConstraints::tight(Size::new(100.0, 100.0));
        assert!(BoxLayout::validate_constraints(&valid));
    }
}
