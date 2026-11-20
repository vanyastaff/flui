//! The operations a typed layout or hit-test context offers a render object.
//!
//! [`LayoutContextApi`] and [`HitTestContextApi`] are what
//! [`Protocol::LayoutContext`] and [`Protocol::HitTestContext`] implement, so
//! code generic over the protocol reaches a context's constraints, children
//! and position through them.

use flui_foundation::Arity;
use flui_foundation::geometry::Offset;

use super::protocol::Protocol;
use crate::parent_data::ParentData;

/// API for layout context operations.
pub trait LayoutContextApi<'ctx, P: Protocol, A: Arity, PD: ParentData> {
    /// Gets the layout constraints from parent.
    fn constraints(&self) -> &P::Constraints;

    /// Gets the number of children.
    fn child_count(&self) -> usize;

    /// Layouts a child with given constraints.
    fn layout_child(
        &mut self,
        index: usize,
        constraints: P::Constraints,
    ) -> crate::error::RenderResult<P::Geometry>;

    /// Positions a child at the given offset.
    fn position_child(&mut self, index: usize, offset: Offset);

    /// A child's current geometry (after layout), or `None` for an index out of range.
    ///
    /// A `RawBoxLayoutContext` in **Direct** storage (built with `new`, `with_children` or
    /// `with_layout_callback`) answers its child state's size: the size it was built with until
    /// `layout_child` runs, then the laid-out one.
    ///
    /// In **Proxy** storage (built by `RawBoxLayoutContext::from_erased` inside the
    /// `RenderObject<BoxProtocol>` blanket impl) it asks the wrapped erased context
    /// (`ErasedBoxLayoutContext::child_size`). The pipeline's driver context answers `None` for a
    /// child this pass has not laid out yet, so a caller lays a child out before reading its size.
    fn child_geometry(&self, index: usize) -> Option<&P::Geometry>;

    /// Gets a child's parent data.
    fn child_parent_data(&self, index: usize) -> Option<&PD>;

    /// Gets mutable reference to child's parent data.
    fn child_parent_data_mut(&mut self, index: usize) -> Option<&mut PD>;
}

/// API for hit test context operations.
pub trait HitTestContextApi<'ctx, P: Protocol, A: Arity, PD: ParentData> {
    /// Gets the hit test position in local coordinates.
    fn position(&self) -> &P::Position;

    /// Checks if position is inside the given bounds.
    fn is_hit(&self, bounds: flui_foundation::geometry::Rect) -> bool;

    /// Tests a child for hits with position transformation.
    fn hit_test_child(&mut self, index: usize, position: P::Position) -> bool;

    /// Tests a child at its laid-out position (`RenderState.offset`),
    /// resolved by the pipeline driver — the parent supplies no offset.
    ///
    /// Default `false` for contexts without driver plumbing (leaf test
    /// fixtures, the sliver surface until its hit-test walk lands).
    fn hit_test_child_at_layout_offset(&mut self, _index: usize) -> bool {
        false
    }

    /// Adds a transform to the hit test path.
    fn push_transform(&mut self, transform: flui_foundation::geometry::Matrix4);

    /// Removes the most recent transform.
    fn pop_transform(&mut self);

    /// Adds an offset transform (convenience method).
    fn push_offset(&mut self, offset: Offset) {
        self.push_transform(flui_foundation::geometry::Matrix4::translation(
            offset.dx, offset.dy, 0.0,
        ));
    }
}
