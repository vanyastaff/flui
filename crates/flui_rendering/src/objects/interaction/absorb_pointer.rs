//! RenderAbsorbPointer - prevents pointer events from reaching children

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderAbsorbPointer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsorbPointerData {
    /// Whether to absorb pointer events
    pub absorbing: bool,
}

impl AbsorbPointerData {
    /// Create new absorb pointer data
    pub fn new(absorbing: bool) -> Self {
        Self { absorbing }
    }
}

impl Default for AbsorbPointerData {
    fn default() -> Self {
        Self { absorbing: true }
    }
}

/// RenderObject that prevents pointer events from reaching its child
///
/// When absorbing is true, this widget consumes all pointer events,
/// preventing them from reaching the child. The child is still painted
/// but doesn't receive events.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::interaction::AbsorbPointerData};
///
/// let mut absorb = SingleRenderBox::new(AbsorbPointerData::new(true));
/// ```
pub type RenderAbsorbPointer = SingleRenderBox<AbsorbPointerData>;

// ===== Public API =====

impl RenderAbsorbPointer {
    /// Check if absorbing pointer events
    pub fn absorbing(&self) -> bool {
        self.data().absorbing
    }

    /// Set whether to absorb pointer events
    ///
    /// This doesn't affect layout or paint, only hit testing.
    pub fn set_absorbing(&mut self, absorbing: bool) {
        if self.data().absorbing != absorbing {
            self.data_mut().absorbing = absorbing;
            // Note: In a full implementation, this would mark needs hit test update
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderAbsorbPointer {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with same constraints
        let size = if let Some(&child_id) = children_ids.first() {
            ctx.layout_child_cached(child_id, constraints, None)
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
        // Paint child normally - absorbing only affects hit testing
        let children_ids = ctx.children();
        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
        }

        // TODO: In a real implementation, we would:
        // 1. Register hit test behavior during hit testing phase
        // 2. Return true from hit_test to absorb events
        // 3. Prevent events from propagating to child
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_absorb_pointer_data_new() {
        let data = AbsorbPointerData::new(true);
        assert!(data.absorbing);

        let data = AbsorbPointerData::new(false);
        assert!(!data.absorbing);
    }

    #[test]
    fn test_absorb_pointer_data_default() {
        let data = AbsorbPointerData::default();
        assert!(data.absorbing);
    }

    #[test]
    fn test_render_absorb_pointer_new() {
        let absorb = SingleRenderBox::new(AbsorbPointerData::new(true));
        assert!(absorb.absorbing());
    }

    #[test]
    fn test_render_absorb_pointer_set_absorbing() {
        let mut absorb = SingleRenderBox::new(AbsorbPointerData::new(true));

        absorb.set_absorbing(false);
        assert!(!absorb.absorbing());

        absorb.set_absorbing(true);
        assert!(absorb.absorbing());
    }

    #[test]
    fn test_render_absorb_pointer_layout() {
        use flui_core::testing::mock_render_context;

        let absorb = SingleRenderBox::new(AbsorbPointerData::new(true));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = absorb.layout(constraints, &ctx);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_absorb_pointer_data_equality() {
        let data1 = AbsorbPointerData::new(true);
        let data2 = AbsorbPointerData::new(true);
        let data3 = AbsorbPointerData::new(false);

        assert_eq!(data1, data2);
        assert_ne!(data1, data3);
    }
}
