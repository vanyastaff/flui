//! RenderOpacity - applies opacity to a child

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderOpacity
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpacityData {
    /// Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    pub opacity: f32,
}

impl OpacityData {
    /// Create new opacity data
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0)
        }
    }
}

/// RenderObject that applies opacity to its child
///
/// The opacity value ranges from 0.0 (fully transparent) to 1.0 (fully opaque).
/// Changing opacity only affects painting, not layout.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::OpacityData};
///
/// let mut opacity = SingleRenderBox::new(OpacityData::new(0.5));
/// ```
pub type RenderOpacity = SingleRenderBox<OpacityData>;

// ===== Public API =====

impl RenderOpacity {
    /// Get the opacity
    pub fn opacity(&self) -> f32 {
        self.data().opacity
    }

    /// Set new opacity
    ///
    /// If opacity changes, marks as needing paint (not layout).
    pub fn set_opacity(&mut self, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if self.data().opacity != clamped {
            self.data_mut().opacity = clamped;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderOpacity {
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
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Paint child with opacity
        if let Some(&child_id) = children_ids.first() {
            let opacity = self.data().opacity;

            // If fully transparent, skip painting
            if opacity <= 0.0 {
                return;
            }

            // If fully opaque, paint normally
            if opacity >= 1.0 {
                ctx.paint_child(child_id, painter, offset);
                return;
            }

            // TODO: When egui supports opacity layers, apply opacity here
            // For now, just paint the child normally
            // In a real implementation, we would:
            // 1. Create an offscreen layer
            // 2. Paint child to that layer
            // 3. Composite the layer with opacity
            ctx.paint_child(child_id, painter, offset);
        }
    }

    fn hit_test_children(&self, result: &mut flui_types::events::HitTestResult, position: Offset, ctx: &flui_core::RenderContext) -> bool {
        // Test hit on child (single child only)
        if let Some(&child_id) = ctx.children().first() {
            // No offset for opacity - pass position through unchanged
            return ctx.hit_test_child(child_id, result, position);
        }

        false
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_opacity_new() {
        let mut opacity = SingleRenderBox::new(OpacityData::new(0.5));
        assert_eq!(opacity.opacity(), 0.5);
    }

    #[test]
    fn test_render_opacity_clamping() {
        let opacity1 = SingleRenderBox::new(OpacityData::new(-0.5));
        assert_eq!(opacity1.opacity(), 0.0);

        let opacity2 = SingleRenderBox::new(OpacityData::new(1.5));
        assert_eq!(opacity2.opacity(), 1.0);
    }

    #[test]
    fn test_render_opacity_set_opacity() {
        use flui_core::testing::mock_render_context;

        let mut opacity = SingleRenderBox::new(OpacityData::new(0.5));

        // Clear initial needs_layout flag by doing a layout
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let (_tree, ctx) = mock_render_context();
        let _ = opacity.layout(constraints, &ctx);

        // Now set opacity - should only mark needs_paint, not needs_layout
        opacity.set_opacity(0.8);
        assert_eq!(opacity.opacity(), 0.8);
        assert!(opacity.needs_paint());
        assert!(!opacity.needs_layout());
    }

    #[test]
    fn test_render_opacity_layout_no_child() {
        use flui_core::testing::mock_render_context;

        let mut opacity = SingleRenderBox::new(OpacityData::new(0.5));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = opacity.layout(constraints, &ctx);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_opacity_data_debug() {
        let data = OpacityData::new(0.75);
        let debug_str = format!("{:?}", data);
        assert!(debug_str.contains("OpacityData"));
    }
}
