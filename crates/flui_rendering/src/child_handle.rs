//! Child handle for type-safe parent-child state access.
//!
//! This module provides [`ChildHandle`], a typed view into a child's state
//! (size, offset, parent data) for use during layout and painting operations.
//!
//! # Context-Based API
//!
//! Child handles provide read/write access to child state. For actual operations
//! (layout, paint, hit test), use the Context API methods:
//!
//! - **Layout**: `ctx.layout_child(index, constraints)` via `LayoutContext`
//! - **Paint**: `ctx.paint_child(index)` via `PaintContext`
//! - **Hit Test**: `ctx.hit_test_child(index)` via `HitTestContext`
//!
//! # Example
//!
//! ```ignore
//! // During layout - use Context for layout, Handle for positioning
//! fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Variable, FlexParentData>) {
//!     for i in 0..ctx.child_count() {
//!         // Layout through context
//!         let size = ctx.layout_child(i, constraints);
//!
//!         // Position through context
//!         ctx.position_child(i, Offset::new(0.0, y_offset));
//!         y_offset += size.height;
//!     }
//! }
//!
//! // Alternative: using ChildrenAccess for typed parent data
//! fn perform_layout(&mut self, children: &mut ChildrenAccess<Variable, FlexParentData>) {
//!     children.for_each(|mut child| {
//!         // Read/modify parent data
//!         child.parent_data_mut().flex = 2.0;
//!
//!         // Read cached values
//!         let current_offset = child.offset();
//!         child.set_offset(new_offset);
//!     });
//! }
//! ```

use flui_foundation::RenderId;
use flui_types::{Offset, Point, Rect, Size};

use crate::parent_data::ParentData;

// ============================================================================
// ChildHandle
// ============================================================================

/// Type-safe handle to a child's state (size, offset, parent data).
///
/// `ChildHandle` provides access to a child's cached state. For operations
/// that require the render tree (layout, paint, hit test), use the
/// corresponding Context API instead.
///
/// # Type Parameters
///
/// - `P`: The ParentData type (e.g., `FlexParentData`, `StackParentData`)
///
/// # Available Operations
///
/// | Operation | Method |
/// |-----------|--------|
/// | Read ID | `id()` |
/// | Read size | `size()` |
/// | Read/set offset | `offset()`, `set_offset()` |
/// | Read/modify parent data | `parent_data()`, `parent_data_mut()` |
/// | Bounds checking | `contains_point()`, `paint_bounds()` |
///
/// # Note on Layout/Paint/HitTest
///
/// This handle does NOT provide layout/paint/hit_test operations.
/// Use the Context API for those:
/// - `LayoutContext::layout_child(index, constraints)`
/// - `PaintContext::paint_child(index)`
/// - `HitTestContext::hit_test_child(index)`
pub struct ChildHandle<'a, P: ParentData + Default> {
    /// ID of the child render node.
    child_id: RenderId,

    /// Cached size (valid after layout).
    size: Size,

    /// Position offset set by parent.
    offset: Offset,

    /// Parent data stored on this child.
    parent_data: &'a mut P,
}

// ============================================================================
// Core Methods
// ============================================================================

impl<'a, P: ParentData + Default> ChildHandle<'a, P> {
    /// Creates a new child handle.
    #[inline]
    pub fn new(child_id: RenderId, size: Size, offset: Offset, parent_data: &'a mut P) -> Self {
        Self {
            child_id,
            size,
            offset,
            parent_data,
        }
    }

    // ========================================================================
    // Read Accessors
    // ========================================================================

    /// Returns the child's render ID.
    #[inline]
    pub fn id(&self) -> RenderId {
        self.child_id
    }

    /// Returns the size of this child (valid after layout).
    #[inline]
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the offset of this child from parent's origin.
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Returns a reference to the typed parent data.
    #[inline]
    pub fn parent_data(&self) -> &P {
        self.parent_data
    }

    // ========================================================================
    // Write Accessors
    // ========================================================================

    /// Sets the offset of this child.
    ///
    /// This positions the child relative to the parent's origin.
    ///
    /// # Example
    ///
    /// ```ignore
    /// child.set_offset(Offset::new(10.0, 20.0));
    /// ```
    #[inline]
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Returns a mutable reference to the typed parent data.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // For FlexParentData
    /// child.parent_data_mut().flex = 2.0;
    /// child.parent_data_mut().fit = FlexFit::Tight;
    ///
    /// // For StackParentData
    /// child.parent_data_mut().left = Some(10.0);
    /// child.parent_data_mut().top = Some(20.0);
    /// ```
    #[inline]
    pub fn parent_data_mut(&mut self) -> &mut P {
        self.parent_data
    }

    // ========================================================================
    // Geometry Helpers
    // ========================================================================

    /// Returns the paint bounds of this child.
    #[inline]
    pub fn paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(self.offset.dx, self.offset.dy), self.size)
    }

    /// Checks if a point (in parent coordinates) is within this child's bounds.
    #[inline]
    pub fn contains_point(&self, point: Offset) -> bool {
        let local = Offset::new(point.dx - self.offset.dx, point.dy - self.offset.dy);
        local.dx >= 0.0
            && local.dy >= 0.0
            && local.dx < self.size.width
            && local.dy < self.size.height
    }

    /// Transforms a point from parent coordinates to local child coordinates.
    #[inline]
    pub fn global_to_local(&self, point: Offset) -> Offset {
        Offset::new(point.dx - self.offset.dx, point.dy - self.offset.dy)
    }

    /// Transforms a point from local child coordinates to parent coordinates.
    #[inline]
    pub fn local_to_global(&self, point: Offset) -> Offset {
        Offset::new(point.dx + self.offset.dx, point.dy + self.offset.dy)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parent_data::BoxParentData;

    #[test]
    fn test_child_handle_creation() {
        let mut parent_data = BoxParentData::default();
        let handle: ChildHandle<BoxParentData> = ChildHandle::new(
            RenderId::new(1),
            Size::new(100.0, 50.0),
            Offset::new(10.0, 20.0),
            &mut parent_data,
        );

        assert_eq!(handle.id().get(), 1);
        assert_eq!(handle.size(), Size::new(100.0, 50.0));
        assert_eq!(handle.offset(), Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_set_offset() {
        let mut parent_data = BoxParentData::default();
        let mut handle: ChildHandle<BoxParentData> = ChildHandle::new(
            RenderId::new(1),
            Size::new(100.0, 50.0),
            Offset::ZERO,
            &mut parent_data,
        );

        handle.set_offset(Offset::new(30.0, 40.0));
        assert_eq!(handle.offset(), Offset::new(30.0, 40.0));
    }

    #[test]
    fn test_contains_point() {
        let mut parent_data = BoxParentData::default();
        let handle: ChildHandle<BoxParentData> = ChildHandle::new(
            RenderId::new(1),
            Size::new(100.0, 50.0),
            Offset::new(10.0, 20.0),
            &mut parent_data,
        );

        // Inside bounds
        assert!(handle.contains_point(Offset::new(50.0, 40.0)));

        // Outside bounds
        assert!(!handle.contains_point(Offset::new(5.0, 40.0))); // Left of
        assert!(!handle.contains_point(Offset::new(150.0, 40.0))); // Right of
        assert!(!handle.contains_point(Offset::new(50.0, 15.0))); // Above
        assert!(!handle.contains_point(Offset::new(50.0, 100.0))); // Below
    }

    #[test]
    fn test_paint_bounds() {
        let mut parent_data = BoxParentData::default();
        let handle: ChildHandle<BoxParentData> = ChildHandle::new(
            RenderId::new(1),
            Size::new(100.0, 50.0),
            Offset::new(10.0, 20.0),
            &mut parent_data,
        );

        let bounds = handle.paint_bounds();
        assert_eq!(bounds.left(), 10.0);
        assert_eq!(bounds.top(), 20.0);
        assert_eq!(bounds.width(), 100.0);
        assert_eq!(bounds.height(), 50.0);
    }

    #[test]
    fn test_coordinate_transforms() {
        let mut parent_data = BoxParentData::default();
        let handle: ChildHandle<BoxParentData> = ChildHandle::new(
            RenderId::new(1),
            Size::new(100.0, 50.0),
            Offset::new(10.0, 20.0),
            &mut parent_data,
        );

        // Global to local
        let local = handle.global_to_local(Offset::new(50.0, 40.0));
        assert_eq!(local, Offset::new(40.0, 20.0));

        // Local to global
        let global = handle.local_to_global(Offset::new(40.0, 20.0));
        assert_eq!(global, Offset::new(50.0, 40.0));
    }
}
