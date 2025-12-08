//! Pipeline visitable traits for layout, paint, and hit-test operations.
//!
//! These traits define the interface for tree traversal during rendering phases.

use flui_foundation::ElementId;

// ============================================================================
// LAYOUT VISITABLE
// ============================================================================

/// Trait for elements that can participate in layout traversal.
pub trait LayoutVisitable {
    /// The constraint type passed down during layout.
    type Constraints;

    /// The geometry type returned from layout.
    type Geometry;

    /// The position type for child positioning.
    type Position;

    /// Perform layout on an element with the given constraints.
    fn layout_element(&mut self, id: ElementId, constraints: Self::Constraints) -> Self::Geometry;

    /// Set the position of an element relative to its parent.
    fn set_position(&mut self, id: ElementId, position: Self::Position);

    /// Get the current position of an element.
    fn get_position(&self, id: ElementId) -> Option<Self::Position>;

    /// Get the computed geometry of an element.
    fn get_geometry(&self, id: ElementId) -> Option<Self::Geometry>;
}

// ============================================================================
// PAINT VISITABLE
// ============================================================================

/// Trait for elements that can participate in paint traversal.
pub trait PaintVisitable {
    /// The position type for painting.
    type Position;

    /// The result type from painting.
    type PaintResult;

    /// Paint an element at the given position.
    fn paint_element(&mut self, id: ElementId, position: Self::Position) -> Self::PaintResult;

    /// Combine paint results from children.
    fn combine_paint_results(&self, results: Vec<Self::PaintResult>) -> Self::PaintResult;
}

// ============================================================================
// HIT TEST VISITABLE
// ============================================================================

/// Trait for elements that can participate in hit-test traversal.
pub trait HitTestVisitable {
    /// The position type for hit testing.
    type Position;

    /// The hit result accumulator type.
    type HitResult;

    /// Perform hit test on an element.
    ///
    /// Returns true if the element was hit and added to the result.
    fn hit_test_element(
        &self,
        id: ElementId,
        position: Self::Position,
        result: &mut Self::HitResult,
    ) -> bool;

    /// Transform a position from parent to child coordinate space.
    fn transform_position_for_child(
        &self,
        parent: ElementId,
        child: ElementId,
        position: Self::Position,
    ) -> Self::Position;
}
