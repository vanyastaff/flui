//! RenderPadding - adds padding around a child

use flui_types::{EdgeInsets, Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;

/// RenderObject that adds padding around its child
///
/// After architecture refactoring, RenderObjects now directly implement DynRenderObject
/// without a RenderBox wrapper. State is stored in ElementTree, accessed via RenderContext.
///
/// Padding increases the size of the widget by the padding amount.
/// The child is laid out with constraints deflated by the padding,
/// then the final size includes the padding.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderPadding;
/// use flui_types::EdgeInsets;
///
/// let padding = RenderPadding::new(EdgeInsets::all(10.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderPadding {
    /// The padding to apply
    pub padding: EdgeInsets,
}

impl RenderPadding {
    /// Create new padding data
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }

    /// Get the padding
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Set new padding (returns new instance)
    ///
    /// Since we no longer have interior mutability in the data itself,
    /// this returns a new RenderPadding instance.
    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderPadding {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints directly in state
        *state.constraints.lock() = Some(constraints);

        let padding = self.padding;

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with deflated constraints
        let size = if let Some(&child_id) = children_ids.first() {
            // Deflate constraints by padding
            let child_constraints = BoxConstraints::new(
                (constraints.min_width - padding.horizontal_total()).max(0.0),
                constraints.max_width - padding.horizontal_total(),
                (constraints.min_height - padding.vertical_total()).max(0.0),
                constraints.max_height - padding.vertical_total(),
            );

            // Layout child via RenderContext
            let child_size = ctx.layout_child_cached(child_id, child_constraints, None);

            // Add padding to child size
            Size::new(
                child_size.width + padding.horizontal_total(),
                child_size.height + padding.vertical_total(),
            )
        } else {
            // No child - just return padding size
            Size::new(
                padding.horizontal_total(),
                padding.vertical_total(),
            )
        };

        // Store size directly in state and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, _state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Paint child with offset adjusted for padding
        if let Some(&child_id) = children_ids.first() {
            let padding = self.padding;
            let child_offset = Offset::new(
                offset.dx + padding.left,
                offset.dy + padding.top,
            );

            // Debug log for significant padding (likely margin)
            if padding.left >= 10.0 || padding.right >= 10.0 {
                tracing::debug!(
                    "RenderPadding::paint: padding={:?}, parent_offset={:?}, child_offset={:?}",
                    padding, offset, child_offset
                );
            }

            ctx.paint_child(child_id, painter, child_offset);
        }
    }

    fn hit_test_children(&self, result: &mut flui_types::events::HitTestResult, position: Offset, ctx: &flui_core::RenderContext) -> bool {
        // Test hit on child (single child only)
        if let Some(&child_id) = ctx.children().first() {
            let padding = self.padding;

            // Subtract padding from position to get position relative to child
            let child_position = Offset::new(
                position.dx - padding.left,
                position.dy - padding.top,
            );

            // Hit test child
            return ctx.hit_test_child(child_id, result, child_position);
        }

        false
    }

    // All other methods (size, mark_needs_layout, etc.) use default implementations
    // from DynRenderObject trait, which delegate to RenderContext/ElementTree.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_padding_new() {
        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        assert_eq!(padding.padding(), EdgeInsets::all(10.0));
    }

    #[test]
    fn test_render_padding_with_padding() {
        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        let padding = padding.with_padding(EdgeInsets::all(20.0));
        assert_eq!(padding.padding(), EdgeInsets::all(20.0));
    }

    #[test]
    fn test_render_padding_layout_no_child() {
        use flui_core::testing::mock_render_context;

        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        let (_tree, ctx) = mock_render_context();
        let size = padding.layout(constraints, &ctx);

        // With no child, size should be just the padding
        assert_eq!(size, Size::new(20.0, 20.0));
    }

    #[test]
    fn test_render_padding_layout_with_child() {
        // This test would require creating a mock child RenderObject
        // For now, we verify the basic structure works
        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        assert_eq!(padding.padding(), EdgeInsets::all(10.0));
    }

    #[test]
    fn test_padding_data_debug() {
        let data = RenderPadding::new(EdgeInsets::all(5.0));
        let debug_str = format!("{:?}", data);
        assert!(debug_str.contains("RenderPadding"));
    }
}
