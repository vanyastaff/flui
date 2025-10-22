//! Common trait providing shared functionality to all RenderBox types

use flui_types::{Size, constraints::BoxConstraints};
use super::{RenderState, RenderFlags};

/// Trait providing common functionality to all RenderBox generic types
///
/// This trait is implemented by LeafRenderBox<T>, SingleRenderBox<T>, and ContainerRenderBox<T>.
/// It provides default implementations for common operations that all RenderObjects need.
///
/// # Design Pattern
///
/// Instead of duplicating these methods across 81 RenderObject implementations,
/// we centralize them in this mixin trait. Each generic type implements the two
/// required methods (`state()` and `state_mut()`), and gets all other methods for free.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{LeafRenderBox, RenderBoxMixin};
///
/// struct MyData {
///     color: Color,
/// }
///
/// type RenderMyWidget = LeafRenderBox<MyData>;
///
/// let mut render_obj = RenderMyWidget::new(MyData { color: Color::RED });
///
/// // These methods come from RenderBoxMixin:
/// render_obj.mark_needs_layout();
/// render_obj.mark_needs_paint();
///
/// if render_obj.needs_layout() {
///     // Perform layout...
/// }
/// ```
pub trait RenderBoxMixin {
    // ========== Required Methods ==========

    /// Get immutable reference to the shared state
    fn state(&self) -> &RenderState;

    /// Get mutable reference to the shared state
    fn state_mut(&mut self) -> &mut RenderState;

    // ========== Provided Methods (with default implementations) ==========

    /// Get the current size (after layout)
    ///
    /// Returns the size determined by the last layout pass.
    /// Returns `None` if layout hasn't been performed yet.
    #[inline]
    fn size(&self) -> Option<Size> {
        self.state().size
    }

    /// Get the constraints used in the last layout
    ///
    /// Returns the constraints from the last layout pass.
    /// Returns `None` if layout hasn't been performed yet.
    #[inline]
    fn constraints(&self) -> Option<BoxConstraints> {
        self.state().constraints
    }

    /// Check if this render object needs layout
    ///
    /// Returns `true` if `mark_needs_layout()` has been called and
    /// layout hasn't been performed yet.
    #[inline]
    fn needs_layout(&self) -> bool {
        self.state().needs_layout()
    }

    /// Check if this render object needs paint
    ///
    /// Returns `true` if `mark_needs_paint()` has been called and
    /// painting hasn't been performed yet.
    #[inline]
    fn needs_paint(&self) -> bool {
        self.state().needs_paint()
    }

    /// Mark this render object as needing layout
    ///
    /// This schedules the render object for layout during the next frame.
    /// The layout will be performed by the Element that owns this RenderObject.
    #[inline]
    fn mark_needs_layout(&mut self) {
        self.state_mut().mark_needs_layout();
    }

    /// Mark this render object as needing paint
    ///
    /// This schedules the render object for painting during the next frame.
    /// Only affects visual appearance, not size or position.
    #[inline]
    fn mark_needs_paint(&mut self) {
        self.state_mut().mark_needs_paint();
    }

    /// Get the dirty state flags
    ///
    /// Returns the full bitflags with all dirty state information.
    /// Use this when you need to check multiple flags at once.
    #[inline]
    fn flags(&self) -> RenderFlags {
        self.state().flags
    }

    /// Check if layout has been performed
    ///
    /// Returns `true` if this RenderObject has been laid out at least once.
    #[inline]
    fn has_size(&self) -> bool {
        self.state().has_size()
    }

    /// Mark as repaint boundary
    ///
    /// A repaint boundary isolates painting - when this object needs repaint,
    /// it doesn't cause ancestors to repaint. Useful for expensive widgets.
    #[inline]
    fn set_is_repaint_boundary(&mut self, value: bool) {
        if value {
            self.state_mut().flags.insert(RenderFlags::IS_REPAINT_BOUNDARY);
        } else {
            self.state_mut().flags.remove(RenderFlags::IS_REPAINT_BOUNDARY);
        }
    }

    /// Check if this is a repaint boundary
    #[inline]
    fn is_repaint_boundary(&self) -> bool {
        self.state().flags.contains(RenderFlags::IS_REPAINT_BOUNDARY)
    }

    /// Clear the needs_layout flag
    ///
    /// Called after layout is performed.
    #[inline]
    fn clear_needs_layout(&mut self) {
        self.state_mut().clear_needs_layout();
    }

    /// Clear the needs_paint flag
    ///
    /// Called after painting is performed.
    #[inline]
    fn clear_needs_paint(&mut self) {
        self.state_mut().clear_needs_paint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementation for testing
    struct MockRenderBox {
        state: RenderState,
    }

    impl RenderBoxMixin for MockRenderBox {
        fn state(&self) -> &RenderState {
            &self.state
        }

        fn state_mut(&mut self) -> &mut RenderState {
            &mut self.state
        }
    }

    #[test]
    fn test_mixin_basic() {
        let mut mock = MockRenderBox {
            state: RenderState::new(),
        };

        // Initially needs layout
        assert!(mock.needs_layout());
        assert!(!mock.needs_paint());

        // Mark as needing paint
        mock.mark_needs_paint();
        assert!(mock.needs_paint());

        // Clear needs layout
        mock.state_mut().clear_needs_layout();
        assert!(!mock.needs_layout());
    }

    #[test]
    fn test_repaint_boundary() {
        let mut mock = MockRenderBox {
            state: RenderState::new(),
        };

        assert!(!mock.is_repaint_boundary());

        mock.set_is_repaint_boundary(true);
        assert!(mock.is_repaint_boundary());

        mock.set_is_repaint_boundary(false);
        assert!(!mock.is_repaint_boundary());
    }

    #[test]
    fn test_size_tracking() {
        let mut mock = MockRenderBox {
            state: RenderState::new(),
        };

        assert!(!mock.has_size());
        assert!(mock.size().is_none());

        mock.state_mut().size = Some(Size::new(100.0, 100.0));
        assert!(mock.has_size());
        assert_eq!(mock.size(), Some(Size::new(100.0, 100.0)));
    }
}
