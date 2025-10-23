//! RenderIgnorePointer - makes widget ignore pointer events

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderIgnorePointer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IgnorePointerData {
    /// Whether to ignore pointer events
    pub ignoring: bool,
}

impl IgnorePointerData {
    /// Create new ignore pointer data
    pub fn new(ignoring: bool) -> Self {
        Self { ignoring }
    }
}

impl Default for IgnorePointerData {
    fn default() -> Self {
        Self { ignoring: true }
    }
}

/// RenderObject that makes its subtree ignore pointer events
///
/// When ignoring is true, this widget and its children don't respond to
/// pointer events. Unlike AbsorbPointer, events pass through to widgets behind.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::interaction::IgnorePointerData};
///
/// let mut ignore = SingleRenderBox::new(IgnorePointerData::new(true));
/// ```
pub type RenderIgnorePointer = SingleRenderBox<IgnorePointerData>;

// ===== Public API =====

impl RenderIgnorePointer {
    /// Check if ignoring pointer events
    pub fn ignoring(&self) -> bool {
        self.data().ignoring
    }

    /// Set whether to ignore pointer events
    ///
    /// This doesn't affect layout or paint, only hit testing.
    pub fn set_ignoring(&mut self, ignoring: bool) {
        if self.data().ignoring != ignoring {
            self.data_mut().ignoring = ignoring;
            // Note: In a full implementation, this would mark needs hit test update
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderIgnorePointer {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with same constraints
        let size = if let Some(&child_id) = children_ids.first() {
            ctx.layout_child(child_id, constraints)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Paint child normally - ignoring only affects hit testing
        let children_ids = ctx.children();
        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
        }

        // TODO: In a real implementation, we would:
        // 1. Register hit test behavior during hit testing phase
        // 2. Return false from hit_test to let events pass through
        // 3. Child doesn't receive events but widgets behind do
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ignore_pointer_data_new() {
        let data = IgnorePointerData::new(true);
        assert!(data.ignoring);

        let data = IgnorePointerData::new(false);
        assert!(!data.ignoring);
    }

    #[test]
    fn test_ignore_pointer_data_default() {
        let data = IgnorePointerData::default();
        assert!(data.ignoring);
    }

    #[test]
    fn test_render_ignore_pointer_new() {
        let ignore = SingleRenderBox::new(IgnorePointerData::new(true));
        assert!(ignore.ignoring());
    }

    #[test]
    fn test_render_ignore_pointer_set_ignoring() {
        let mut ignore = SingleRenderBox::new(IgnorePointerData::new(true));

        ignore.set_ignoring(false);
        assert!(!ignore.ignoring());

        ignore.set_ignoring(true);
        assert!(ignore.ignoring());
    }

    #[test]
    fn test_render_ignore_pointer_layout() {
        use flui_core::testing::mock_render_context;

        let ignore = SingleRenderBox::new(IgnorePointerData::new(true));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = ignore.layout(constraints, &ctx);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_ignore_pointer_data_equality() {
        let data1 = IgnorePointerData::new(true);
        let data2 = IgnorePointerData::new(true);
        let data3 = IgnorePointerData::new(false);

        assert_eq!(data1, data2);
        assert_ne!(data1, data3);
    }
}
