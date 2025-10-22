//! RenderPadding - adds padding around a child

use flui_types::{EdgeInsets, Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderPadding
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaddingData {
    /// The padding to apply
    pub padding: EdgeInsets,
}

impl PaddingData {
    /// Create new padding data
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

/// RenderObject that adds padding around its child
///
/// Padding increases the size of the widget by the padding amount.
/// The child is laid out with constraints deflated by the padding,
/// then the final size includes the padding.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::PaddingData};
/// use flui_types::EdgeInsets;
///
/// let mut padding = SingleRenderBox::new(PaddingData::new(EdgeInsets::all(10.0)));
/// ```
pub type RenderPadding = SingleRenderBox<PaddingData>;

// ===== Public API =====

impl RenderPadding {
    /// Get the padding
    pub fn padding(&self) -> EdgeInsets {
        self.data().padding
    }

    /// Set new padding
    ///
    /// If padding changes, marks as needing layout.
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        if self.data().padding != padding {
            self.data_mut().padding = padding;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderPadding {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let padding = self.data().padding;

        // Layout child with deflated constraints
        let size = if let Some(child) = self.child_mut() {
            // Deflate constraints by padding
            let child_constraints = BoxConstraints::new(
                (constraints.min_width - padding.horizontal_total()).max(0.0),
                constraints.max_width - padding.horizontal_total(),
                (constraints.min_height - padding.vertical_total()).max(0.0),
                constraints.max_height - padding.vertical_total(),
            );

            // Layout child
            let child_size = child.layout(child_constraints);

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

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child with offset adjusted for padding
        if let Some(child) = self.child() {
            let padding = self.data().padding;
            let child_offset = Offset::new(
                offset.dx + padding.left,
                offset.dy + padding.top,
            );
            child.paint(painter, child_offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_padding_new() {
        let padding = SingleRenderBox::new(PaddingData::new(EdgeInsets::all(10.0)));
        assert_eq!(padding.padding(), EdgeInsets::all(10.0));
    }

    #[test]
    fn test_render_padding_set_padding() {
        let mut padding = SingleRenderBox::new(PaddingData::new(EdgeInsets::all(10.0)));

        padding.set_padding(EdgeInsets::all(20.0));
        assert_eq!(padding.padding(), EdgeInsets::all(20.0));
        assert!(RenderBoxMixin::needs_layout(&padding));
    }

    #[test]
    fn test_render_padding_layout_no_child() {
        let mut padding = SingleRenderBox::new(PaddingData::new(EdgeInsets::all(10.0)));
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        let size = padding.layout(constraints);

        // With no child, size should be just the padding
        assert_eq!(size, Size::new(20.0, 20.0));
    }

    #[test]
    fn test_render_padding_layout_with_child() {
        // This test would require creating a mock child RenderObject
        // For now, we verify the basic structure works
        let padding = SingleRenderBox::new(PaddingData::new(EdgeInsets::all(10.0)));
        assert_eq!(padding.padding(), EdgeInsets::all(10.0));
    }

    #[test]
    fn test_padding_data_debug() {
        let data = PaddingData::new(EdgeInsets::all(5.0));
        let debug_str = format!("{:?}", data);
        assert!(debug_str.contains("PaddingData"));
    }
}
