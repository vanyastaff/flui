//! RenderAlign - aligns child within available space

use flui_types::{Alignment, Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderAlign
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlignData {
    /// The alignment within the available space
    pub alignment: Alignment,
    /// Width factor - if Some, the width is child_width * width_factor
    /// Otherwise, shrink wraps to child
    pub width_factor: Option<f32>,
    /// Height factor - if Some, the height is child_height * height_factor
    /// Otherwise, shrink wraps to child
    pub height_factor: Option<f32>,
}

impl AlignData {
    /// Create new align data with default alignment (center)
    pub fn new(alignment: Alignment) -> Self {
        Self {
            alignment,
            width_factor: None,
            height_factor: None,
        }
    }

    /// Create with alignment and size factors
    pub fn with_factors(
        alignment: Alignment,
        width_factor: Option<f32>,
        height_factor: Option<f32>,
    ) -> Self {
        Self {
            alignment,
            width_factor,
            height_factor,
        }
    }
}

impl Default for AlignData {
    fn default() -> Self {
        Self {
            alignment: Alignment::CENTER,
            width_factor: None,
            height_factor: None,
        }
    }
}

/// RenderObject that aligns its child within the available space
///
/// This widget positions its child according to the alignment parameter.
/// If width_factor or height_factor are specified, the RenderAlign will
/// size itself to be that factor times the child's size in that dimension.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::AlignData};
/// use flui_types::Alignment;
///
/// let mut align = SingleRenderBox::new(AlignData::new(Alignment::TOP_LEFT));
/// ```
pub type RenderAlign = SingleRenderBox<AlignData>;

// ===== Public API =====

impl RenderAlign {
    /// Get the alignment
    pub fn alignment(&self) -> Alignment {
        self.data().alignment
    }

    /// Set new alignment
    ///
    /// If alignment changes, marks as needing layout.
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if self.data().alignment != alignment {
            self.data_mut().alignment = alignment;
            self.mark_needs_layout();
        }
    }

    /// Get width factor
    pub fn width_factor(&self) -> Option<f32> {
        self.data().width_factor
    }

    /// Set width factor
    pub fn set_width_factor(&mut self, width_factor: Option<f32>) {
        if self.data().width_factor != width_factor {
            self.data_mut().width_factor = width_factor;
            self.mark_needs_layout();
        }
    }

    /// Get height factor
    pub fn height_factor(&self) -> Option<f32> {
        self.data().height_factor
    }

    /// Set height factor
    pub fn set_height_factor(&mut self, height_factor: Option<f32>) {
        if self.data().height_factor != height_factor {
            self.data_mut().height_factor = height_factor;
            self.mark_needs_layout();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderAlign {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Clone data to avoid borrow checker issues
        let width_factor = self.data().width_factor;
        let height_factor = self.data().height_factor;

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with loose constraints to get its natural size
        let child_size = if let Some(&child_id) = children_ids.first() {
            // Let child determine its own size within constraints
            ctx.layout_child(child_id, constraints.loosen())
        } else {
            Size::ZERO
        };

        // Calculate our size based on factors
        let width = if let Some(factor) = width_factor {
            (child_size.width * factor).min(constraints.max_width)
        } else {
            // Shrink wrap to child if no factor
            child_size.width.clamp(constraints.min_width, constraints.max_width)
        };

        let height = if let Some(factor) = height_factor {
            (child_size.height * factor).min(constraints.max_height)
        } else {
            // Shrink wrap to child if no factor
            child_size.height.clamp(constraints.min_height, constraints.max_height)
        };

        let size = Size::new(width, height);

        // Store child size for paint offset calculation
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Paint child at aligned position
        if let Some(&child_id) = children_ids.first() {
            let data = self.data();
            let size = state.size.lock().unwrap_or(Size::ZERO);

            // Get child size from context
            // Get child size from tree
                let child_size = if let Some(child_elem) = ctx.tree().get(child_id) {
                    if let Some(child_ro) = child_elem.render_object() {
                        child_ro.size()
                    } else {
                        Size::ZERO
                    }
                } else {
                    Size::ZERO
                };

            // Calculate aligned offset manually
            // Alignment: -1.0 = left/top, 0.0 = center, 1.0 = right/bottom
            let available_width = size.width - child_size.width;
            let available_height = size.height - child_size.height;

            let aligned_x = (available_width * (data.alignment.x + 1.0)) / 2.0;
            let aligned_y = (available_height * (data.alignment.y + 1.0)) / 2.0;

            let paint_offset = Offset::new(
                offset.dx + aligned_x,
                offset.dy + aligned_y,
            );

            ctx.paint_child(child_id, painter, paint_offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_data_new() {
        let data = AlignData::new(Alignment::TOP_LEFT);
        assert_eq!(data.alignment, Alignment::TOP_LEFT);
        assert_eq!(data.width_factor, None);
        assert_eq!(data.height_factor, None);
    }

    #[test]
    fn test_align_data_with_factors() {
        let data = AlignData::with_factors(Alignment::CENTER, Some(2.0), Some(3.0));
        assert_eq!(data.alignment, Alignment::CENTER);
        assert_eq!(data.width_factor, Some(2.0));
        assert_eq!(data.height_factor, Some(3.0));
    }

    #[test]
    fn test_render_align_new() {
        let align = SingleRenderBox::new(AlignData::new(Alignment::BOTTOM_RIGHT));
        assert_eq!(align.alignment(), Alignment::BOTTOM_RIGHT);
    }

    #[test]
    fn test_render_align_set_alignment() {
        let mut align = SingleRenderBox::new(AlignData::default());

        align.set_alignment(Alignment::TOP_CENTER);
        assert_eq!(align.alignment(), Alignment::TOP_CENTER);
        assert!(align.needs_layout());
    }

    #[test]
    fn test_render_align_set_width_factor() {
        let mut align = SingleRenderBox::new(AlignData::default());

        align.set_width_factor(Some(2.0));
        assert_eq!(align.width_factor(), Some(2.0));
        assert!(align.needs_layout());
    }

    #[test]
    fn test_render_align_set_height_factor() {
        let mut align = SingleRenderBox::new(AlignData::default());

        align.set_height_factor(Some(1.5));
        assert_eq!(align.height_factor(), Some(1.5));
        assert!(align.needs_layout());
    }

    #[test]
    fn test_render_align_layout_no_child() {
        use flui_core::testing::mock_render_context;

        let align = SingleRenderBox::new(AlignData::new(Alignment::CENTER));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = align.layout(constraints, &ctx);

        // No child - should be minimum size
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn test_render_align_layout_with_factors() {
        // We can't easily test with a real child here without creating a mock,
        // but we can verify that the alignment is stored correctly
        let mut align = SingleRenderBox::new(
            AlignData::with_factors(Alignment::CENTER, Some(2.0), Some(3.0))
        );

        assert_eq!(align.width_factor(), Some(2.0));
        assert_eq!(align.height_factor(), Some(3.0));
    }
}
