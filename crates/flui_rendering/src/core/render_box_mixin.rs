//! Common trait providing shared functionality to all RenderBox types

use flui_types::constraints::BoxConstraints;
use super::{RenderState, RenderFlags};

/// Trait providing common functionality to all RenderBox generic types
///
/// This trait is implemented by LeafRenderBox<T>, SingleRenderBox<T>, and ContainerRenderBox<T>.
/// It provides access to shared state and helper methods.
///
/// # Design Pattern
///
/// Instead of duplicating state access across 81 RenderObject implementations,
/// we centralize them in this mixin trait. Each generic type implements the two
/// required methods (`state()` and `state_mut()`), and gets helper methods for free.
///
/// # Important: DynRenderObject vs RenderBoxMixin
///
/// This trait intentionally does NOT duplicate methods from DynRenderObject.
/// Methods like `size()`, `needs_layout()`, `needs_paint()`, `mark_needs_layout()`,
/// and `mark_needs_paint()` are defined in DynRenderObject and should be used directly.
/// This avoids trait ambiguity and maintains a single source of truth.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{LeafRenderBox, RenderBoxMixin};
/// use flui_core::DynRenderObject;
///
/// struct MyData {
///     color: Color,
/// }
///
/// type RenderMyWidget = LeafRenderBox<MyData>;
///
/// let mut render_obj = RenderMyWidget::new(MyData { color: Color::RED });
///
/// // Use DynRenderObject methods directly:
/// render_obj.mark_needs_layout();
/// render_obj.mark_needs_paint();
///
/// if render_obj.needs_layout() {
///     // Perform layout...
/// }
///
/// // RenderBoxMixin provides helper methods:
/// if render_obj.has_size() {
///     // Size is available...
/// }
/// ```
pub trait RenderBoxMixin {
    // ========== Required Methods ==========

    /// Get immutable reference to the shared state
    fn state(&self) -> &RenderState;

    /// Get mutable reference to the shared state
    fn state_mut(&mut self) -> &mut RenderState;

    // ========== Provided Methods (with default implementations) ==========
    //
    // NOTE: We intentionally do NOT duplicate methods from DynRenderObject here.
    // Methods like `size()`, `needs_layout()`, `needs_paint()`, `mark_needs_layout()`,
    // and `mark_needs_paint()` are defined in DynRenderObject and should be used directly.
    // This avoids trait ambiguity and maintains a single source of truth.

    /// Get the constraints used in the last layout
    ///
    /// Returns the constraints from the last layout pass.
    /// Returns `None` if layout hasn't been performed yet.
    ///
    /// NOTE: This is a helper that provides Option<BoxConstraints> return type,
    /// while DynRenderObject::constraints() also exists. This helper is more
    /// convenient for checking if layout has occurred.
    #[inline]
    fn constraints(&self) -> Option<BoxConstraints> {
        *self.state().constraints.lock()
    }

    /// Get the dirty state flags
    ///
    /// Returns the full bitflags with all dirty state information.
    /// Use this when you need to check multiple flags at once.
    #[inline]
    fn flags(&self) -> RenderFlags {
        *self.state().flags.lock()
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
    fn set_is_repaint_boundary(&self, value: bool) {
        let mut flags = self.state().flags.lock();
        if value {
            flags.insert(RenderFlags::IS_REPAINT_BOUNDARY);
        } else {
            flags.remove(RenderFlags::IS_REPAINT_BOUNDARY);
        }
    }

    /// Check if this is a repaint boundary
    #[inline]
    fn is_repaint_boundary(&self) -> bool {
        self.state().flags.lock().contains(RenderFlags::IS_REPAINT_BOUNDARY)
    }

    /// Clear the needs_layout flag
    ///
    /// Called after layout is performed.
    #[inline]
    fn clear_needs_layout(&self) {
        self.state().clear_needs_layout();
    }

    /// Clear the needs_paint flag
    ///
    /// Called after painting is performed.
    #[inline]
    fn clear_needs_paint(&self) {
        self.state().clear_needs_paint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Size;

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

        // Initially needs layout (use state directly since methods removed)
        assert!(mock.state().needs_layout());
        assert!(!mock.state().needs_paint());

        // Mark as needing paint
        mock.state_mut().mark_needs_paint();
        assert!(mock.state().needs_paint());

        // Clear needs layout
        mock.state_mut().clear_needs_layout();
        assert!(!mock.state().needs_layout());
    }

    #[test]
    fn test_repaint_boundary() {
        let mock = MockRenderBox {
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
        assert!(mock.state().size.lock().is_none());

        *mock.state_mut().size.lock() = Some(Size::new(100.0, 100.0));
        assert!(mock.has_size());
        assert_eq!(*mock.state().size.lock(), Some(Size::new(100.0, 100.0)));
    }
}
