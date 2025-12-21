//! Child handle for type-safe parent-child interactions.
//!
//! This module provides [`ChildHandle`], a typed view into a child render object
//! that enforces phase-based API restrictions at compile time.
//!
//! # Phase-Based API Safety
//!
//! The handle is parameterized by both ParentData type (`P`) and phase (`Ph`).
//! Different phases expose different APIs to prevent misuse:
//!
//! - **Layout Phase**: Can layout children, set offsets, modify ParentData
//! - **Paint Phase**: Can only paint children (read-only access)
//! - **Hit Test Phase**: Can only hit test children (read-only access)
//!
//! # Example
//!
//! ```ignore
//! // During layout phase - full access
//! fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Variable, FlexParentData>) -> Size {
//!     for mut child in ctx.iter_children_mut() {
//!         // child: ChildHandle<FlexParentData, LayoutPhase>
//!         let size = child.layout(constraints);  // ✅ OK
//!         child.set_offset(offset);              // ✅ OK
//!         child.parent_data_mut().flex = 2.0;    // ✅ OK
//!     }
//! }
//!
//! // During paint phase - read-only
//! fn paint(&self, ctx: &mut BoxPaintContext<Variable, FlexParentData>) {
//!     for child in ctx.iter_children() {
//!         // child: ChildHandle<FlexParentData, PaintPhase>
//!         child.paint(&mut ctx.canvas());        // ✅ OK
//!         // child.layout(constraints);          // ❌ Compile error!
//!         // child.set_offset(offset);           // ❌ Compile error!
//!     }
//! }
//! ```

use std::marker::PhantomData;

use flui_foundation::RenderId;
use flui_types::{Offset, Rect, Size};

use crate::constraints::BoxConstraints;
use crate::parent_data::ParentData;
use crate::phase::{HitTestPhase, LayoutPhase, PaintPhase, Phase};
use crate::pipeline::PaintingContext;
use crate::traits::{BoxHitTestEntry, BoxHitTestResult, TextBaseline};

// ============================================================================
// ChildHandle
// ============================================================================

/// Type-safe handle to a child render object.
///
/// The handle is parameterized by:
/// - `P`: The ParentData type (e.g., `FlexParentData`, `StackParentData`)
/// - `Ph`: The current phase (e.g., `LayoutPhase`, `PaintPhase`)
///
/// Different phases expose different APIs:
///
/// | Operation           | Layout | Paint | HitTest |
/// |---------------------|--------|-------|---------|
/// | `layout()`          | ✅     | ❌    | ❌      |
/// | `set_offset()`      | ✅     | ❌    | ❌      |
/// | `parent_data_mut()` | ✅     | ❌    | ❌      |
/// | `dry_layout()`      | ✅     | ❌    | ❌      |
/// | `paint()`           | ❌     | ✅    | ❌      |
/// | `hit_test()`        | ❌     | ❌    | ✅      |
/// | `size()`            | ✅     | ✅    | ✅      |
/// | `offset()`          | ✅     | ✅    | ✅      |
/// | `parent_data()`     | ✅     | ✅    | ✅      |
pub struct ChildHandle<'a, P: ParentData + Default, Ph: Phase = LayoutPhase> {
    /// ID of the child render node.
    child_id: RenderId,

    /// Cached size (valid after layout).
    size: Size,

    /// Position offset set by parent.
    offset: Offset,

    /// Parent data stored on this child.
    parent_data: &'a mut P,

    /// Phantom for phase type.
    _phase: PhantomData<Ph>,
}

// ============================================================================
// Type Aliases for Convenience
// ============================================================================

/// Child handle for layout phase (can layout, set offset, modify parent data).
pub type LayoutChildHandle<'a, P> = ChildHandle<'a, P, LayoutPhase>;

/// Child handle for paint phase (can only paint, read-only access).
pub type PaintChildHandle<'a, P> = ChildHandle<'a, P, PaintPhase>;

/// Child handle for hit test phase (can only hit test, read-only access).
pub type HitTestChildHandle<'a, P> = ChildHandle<'a, P, HitTestPhase>;

// ============================================================================
// Common Methods (All Phases)
// ============================================================================

impl<'a, P: ParentData + Default, Ph: Phase> ChildHandle<'a, P, Ph> {
    /// Creates a new child handle.
    #[inline]
    pub fn new(child_id: RenderId, size: Size, offset: Offset, parent_data: &'a mut P) -> Self {
        Self {
            child_id,
            size,
            offset,
            parent_data,
            _phase: PhantomData,
        }
    }

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

    /// Returns the offset of this child.
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Returns a reference to the typed parent data.
    #[inline]
    pub fn parent_data(&self) -> &P {
        self.parent_data
    }

    /// Returns the paint bounds of this child.
    #[inline]
    pub fn paint_bounds(&self) -> Rect {
        Rect::new(
            self.offset.dx,
            self.offset.dy,
            self.size.width,
            self.size.height,
        )
    }

    /// Checks if a point is within this child's bounds.
    #[inline]
    pub fn contains_point(&self, point: Offset) -> bool {
        let local = Offset::new(point.dx - self.offset.dx, point.dy - self.offset.dy);
        local.dx >= 0.0
            && local.dy >= 0.0
            && local.dx < self.size.width
            && local.dy < self.size.height
    }
}

// ============================================================================
// Layout Phase Methods
// ============================================================================

impl<'a, P: ParentData + Default> ChildHandle<'a, P, LayoutPhase> {
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
    /// Only available during layout phase.
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

    /// Layouts this child with the given constraints.
    ///
    /// Returns the resulting size. The size is also cached in this handle.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let size = child.layout(BoxConstraints::tight(Size::new(100.0, 50.0)));
    /// child.set_offset(Offset::new(0.0, current_y));
    /// current_y += size.height;
    /// ```
    #[inline]
    pub fn layout(&mut self, _constraints: BoxConstraints) -> Size {
        // TODO: Actually perform layout via RenderTree
        // For now, return cached size
        self.size
    }

    /// Performs dry layout (compute size without side effects).
    ///
    /// Returns the size this child would have with the given constraints,
    /// without actually performing layout.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let hypothetical_size = child.dry_layout(constraints);
    /// if hypothetical_size.width > available_width {
    ///     // Use different constraints
    /// }
    /// ```
    #[inline]
    pub fn dry_layout(&self, _constraints: BoxConstraints) -> Size {
        // TODO: Actually perform dry layout
        self.size
    }

    /// Returns the minimum intrinsic width for a given height.
    #[inline]
    pub fn get_min_intrinsic_width(&self, _height: f32) -> f32 {
        // TODO: Delegate to render object
        0.0
    }

    /// Returns the maximum intrinsic width for a given height.
    #[inline]
    pub fn get_max_intrinsic_width(&self, _height: f32) -> f32 {
        // TODO: Delegate to render object
        0.0
    }

    /// Returns the minimum intrinsic height for a given width.
    #[inline]
    pub fn get_min_intrinsic_height(&self, _width: f32) -> f32 {
        // TODO: Delegate to render object
        0.0
    }

    /// Returns the maximum intrinsic height for a given width.
    #[inline]
    pub fn get_max_intrinsic_height(&self, _width: f32) -> f32 {
        // TODO: Delegate to render object
        0.0
    }

    /// Returns the distance from the top of the box to its baseline.
    #[inline]
    pub fn get_distance_to_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        // TODO: Delegate to render object
        None
    }
}

// ============================================================================
// Paint Phase Methods
// ============================================================================

impl<'a, P: ParentData + Default> ChildHandle<'a, P, PaintPhase> {
    /// Paints this child at its stored offset.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<Variable, FlexParentData>) {
    ///     for child in ctx.iter_children() {
    ///         child.paint(ctx.canvas());  // Paints at child.offset()
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn paint(&self, _context: &mut PaintingContext) {
        // TODO: Actually paint via render object
        // context.paint_child(self.child_id, self.offset);
    }

    /// Paints this child at a custom offset (overriding stored offset).
    ///
    /// Useful for scrolling or animated positions.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let animated_offset = lerp(start_offset, end_offset, t);
    /// child.paint_at(ctx.canvas(), animated_offset);
    /// ```
    #[inline]
    pub fn paint_at(&self, _context: &mut PaintingContext, _offset: Offset) {
        // TODO: Actually paint at offset
        // context.paint_child(self.child_id, offset);
    }
}

// ============================================================================
// Hit Test Phase Methods
// ============================================================================

impl<'a, P: ParentData + Default> ChildHandle<'a, P, HitTestPhase> {
    /// Hit tests this child at the given position.
    ///
    /// The position is in the parent's coordinate system.
    /// Returns true if the child (or any of its descendants) was hit.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn hit_test(&self, ctx: &mut BoxHitTestContext<Variable, FlexParentData>, position: Offset) -> bool {
    ///     // Test children in reverse order (front to back)
    ///     for child in ctx.iter_children().rev() {
    ///         if child.hit_test(ctx.result_mut(), position) {
    ///             return true;
    ///         }
    ///     }
    ///     false
    /// }
    /// ```
    #[inline]
    pub fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Transform position to local coordinates
        let local_position =
            Offset::new(position.dx - self.offset.dx, position.dy - self.offset.dy);

        // Check if within bounds
        if local_position.dx < 0.0
            || local_position.dy < 0.0
            || local_position.dx >= self.size.width
            || local_position.dy >= self.size.height
        {
            return false;
        }

        // TODO: Delegate to render object for actual hit testing
        result.add(BoxHitTestEntry::new(local_position));
        true
    }

    /// Hit tests this child with a custom offset (overriding stored offset).
    ///
    /// Useful for scrolling views where visual offset differs from logical offset.
    #[inline]
    pub fn hit_test_at(
        &self,
        result: &mut BoxHitTestResult,
        position: Offset,
        paint_offset: Offset,
    ) -> bool {
        let local_position =
            Offset::new(position.dx - paint_offset.dx, position.dy - paint_offset.dy);

        if local_position.dx < 0.0
            || local_position.dy < 0.0
            || local_position.dx >= self.size.width
            || local_position.dy >= self.size.height
        {
            return false;
        }

        // TODO: Delegate to render object
        result.add(BoxHitTestEntry::new(local_position));
        true
    }
}

// ============================================================================
// Phase Transition (Internal)
// ============================================================================

impl<'a, P: ParentData + Default> ChildHandle<'a, P, LayoutPhase> {
    /// Converts this handle to paint phase (internal use).
    ///
    /// After layout is complete, handles can be converted for painting.
    #[inline]
    pub(crate) fn into_paint_phase(self) -> ChildHandle<'a, P, PaintPhase> {
        ChildHandle {
            child_id: self.child_id,
            size: self.size,
            offset: self.offset,
            parent_data: self.parent_data,
            _phase: PhantomData,
        }
    }

    /// Converts this handle to hit test phase (internal use).
    #[inline]
    pub(crate) fn into_hit_test_phase(self) -> ChildHandle<'a, P, HitTestPhase> {
        ChildHandle {
            child_id: self.child_id,
            size: self.size,
            offset: self.offset,
            parent_data: self.parent_data,
            _phase: PhantomData,
        }
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
        let handle: ChildHandle<BoxParentData, LayoutPhase> = ChildHandle::new(
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
    fn test_layout_phase_set_offset() {
        let mut parent_data = BoxParentData::default();
        let mut handle: LayoutChildHandle<BoxParentData> = ChildHandle::new(
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
        let handle: ChildHandle<BoxParentData, LayoutPhase> = ChildHandle::new(
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
        let handle: ChildHandle<BoxParentData, LayoutPhase> = ChildHandle::new(
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
    fn test_phase_transition() {
        let mut parent_data = BoxParentData::default();
        let layout_handle: LayoutChildHandle<BoxParentData> = ChildHandle::new(
            RenderId::new(1),
            Size::new(100.0, 50.0),
            Offset::new(10.0, 20.0),
            &mut parent_data,
        );

        // Convert to paint phase
        let paint_handle = layout_handle.into_paint_phase();
        assert_eq!(paint_handle.size(), Size::new(100.0, 50.0));
        assert_eq!(paint_handle.offset(), Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_type_aliases() {
        fn assert_layout_handle<P: ParentData + Default>(_: LayoutChildHandle<P>) {}
        fn assert_paint_handle<P: ParentData + Default>(_: PaintChildHandle<P>) {}
        fn assert_hit_test_handle<P: ParentData + Default>(_: HitTestChildHandle<P>) {}

        let mut pd = BoxParentData::default();

        let lh: LayoutChildHandle<BoxParentData> =
            ChildHandle::new(RenderId::new(1), Size::ZERO, Offset::ZERO, &mut pd);
        assert_layout_handle(lh);

        let mut pd2 = BoxParentData::default();
        let ph: PaintChildHandle<BoxParentData> = ChildHandle {
            child_id: RenderId::new(1),
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data: &mut pd2,
            _phase: PhantomData,
        };
        assert_paint_handle(ph);

        let mut pd3 = BoxParentData::default();
        let hh: HitTestChildHandle<BoxParentData> = ChildHandle {
            child_id: RenderId::new(1),
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data: &mut pd3,
            _phase: PhantomData,
        };
        assert_hit_test_handle(hh);
    }
}
