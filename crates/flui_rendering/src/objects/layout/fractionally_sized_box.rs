//! RenderFractionallySizedBox - sizes child as fraction of parent

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderFractionallySizedBox
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FractionallySizedBoxData {
    /// Width factor (0.0 - 1.0), None means unconstrained
    pub width_factor: Option<f32>,
    /// Height factor (0.0 - 1.0), None means unconstrained
    pub height_factor: Option<f32>,
}

impl FractionallySizedBoxData {
    /// Create new fractionally sized box data
    pub fn new(width_factor: Option<f32>, height_factor: Option<f32>) -> Self {
        if let Some(w) = width_factor {
            assert!((0.0..=1.0).contains(&w), "Width factor must be between 0.0 and 1.0");
        }
        if let Some(h) = height_factor {
            assert!((0.0..=1.0).contains(&h), "Height factor must be between 0.0 and 1.0");
        }
        Self {
            width_factor,
            height_factor,
        }
    }

    /// Create with both width and height factors
    pub fn both(factor: f32) -> Self {
        Self::new(Some(factor), Some(factor))
    }

    /// Create with only width factor
    pub fn width(factor: f32) -> Self {
        Self::new(Some(factor), None)
    }

    /// Create with only height factor
    pub fn height(factor: f32) -> Self {
        Self::new(None, Some(factor))
    }
}

/// RenderObject that sizes child as a fraction of available space
///
/// This is useful for making a child take up a percentage of its parent.
/// For example, width_factor: 0.5 makes the child half the parent's width.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::FractionallySizedBoxData};
///
/// // 50% width and height
/// let mut fractional = SingleRenderBox::new(FractionallySizedBoxData::both(0.5));
/// ```
pub type RenderFractionallySizedBox = SingleRenderBox<FractionallySizedBoxData>;

// ===== Public API =====

impl RenderFractionallySizedBox {
    /// Get the width factor
    pub fn width_factor(&self) -> Option<f32> {
        self.data().width_factor
    }

    /// Get the height factor
    pub fn height_factor(&self) -> Option<f32> {
        self.data().height_factor
    }

    /// Set new width factor
    pub fn set_width_factor(&mut self, factor: Option<f32>) {
        if let Some(w) = factor {
            assert!((0.0..=1.0).contains(&w), "Width factor must be between 0.0 and 1.0");
        }
        if self.data().width_factor != factor {
            self.data_mut().width_factor = factor;
            self.mark_needs_layout();
        }
    }

    /// Set new height factor
    pub fn set_height_factor(&mut self, factor: Option<f32>) {
        if let Some(h) = factor {
            assert!((0.0..=1.0).contains(&h), "Height factor must be between 0.0 and 1.0");
        }
        if self.data().height_factor != factor {
            self.data_mut().height_factor = factor;
            self.mark_needs_layout();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderFractionallySizedBox {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        let width_factor = self.data().width_factor;
        let height_factor = self.data().height_factor;

        // Calculate target size based on factors
        let target_width = width_factor.map(|f| constraints.max_width * f);
        let target_height = height_factor.map(|f| constraints.max_height * f);

        // Create child constraints
        let child_constraints = BoxConstraints::new(
            target_width.unwrap_or(constraints.min_width),
            target_width.unwrap_or(constraints.max_width),
            target_height.unwrap_or(constraints.min_height),
            target_height.unwrap_or(constraints.max_height),
        );

        // Layout child
        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            ctx.layout_child_cached(child_id, child_constraints, None)
        } else {
            // No child - use target size or smallest
            Size::new(
                target_width.unwrap_or(constraints.min_width),
                target_height.unwrap_or(constraints.min_height),
            )
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Simply paint child at offset
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fractionally_sized_box_data_new() {
        let data = FractionallySizedBoxData::new(Some(0.5), Some(0.75));
        assert_eq!(data.width_factor, Some(0.5));
        assert_eq!(data.height_factor, Some(0.75));
    }

    #[test]
    fn test_fractionally_sized_box_data_both() {
        let data = FractionallySizedBoxData::both(0.5);
        assert_eq!(data.width_factor, Some(0.5));
        assert_eq!(data.height_factor, Some(0.5));
    }

    #[test]
    fn test_fractionally_sized_box_data_width() {
        let data = FractionallySizedBoxData::width(0.5);
        assert_eq!(data.width_factor, Some(0.5));
        assert_eq!(data.height_factor, None);
    }

    #[test]
    fn test_fractionally_sized_box_data_height() {
        let data = FractionallySizedBoxData::height(0.75);
        assert_eq!(data.width_factor, None);
        assert_eq!(data.height_factor, Some(0.75));
    }

    #[test]
    #[should_panic(expected = "Width factor must be between 0.0 and 1.0")]
    fn test_fractionally_sized_box_data_invalid_width() {
        FractionallySizedBoxData::new(Some(1.5), None);
    }

    #[test]
    #[should_panic(expected = "Height factor must be between 0.0 and 1.0")]
    fn test_fractionally_sized_box_data_invalid_height() {
        FractionallySizedBoxData::new(None, Some(-0.1));
    }

    #[test]
    fn test_render_fractionally_sized_box_new() {
        let fractional = SingleRenderBox::new(FractionallySizedBoxData::both(0.5));
        assert_eq!(fractional.width_factor(), Some(0.5));
        assert_eq!(fractional.height_factor(), Some(0.5));
    }

    #[test]
    fn test_render_fractionally_sized_box_set_factors() {
        let mut fractional = SingleRenderBox::new(FractionallySizedBoxData::both(0.5));

        fractional.set_width_factor(Some(0.75));
        assert_eq!(fractional.width_factor(), Some(0.75));
        assert!(fractional.needs_layout());
    }

    #[test]
    fn test_render_fractionally_sized_box_layout_both_factors() {
        use flui_core::testing::mock_render_context;

        let fractional = SingleRenderBox::new(FractionallySizedBoxData::both(0.5));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 200.0);

        let (_tree, ctx) = mock_render_context();
        let size = fractional.layout(constraints, &ctx);

        // Should be 50% of max constraints
        assert_eq!(size, Size::new(50.0, 100.0));
    }

    #[test]
    fn test_render_fractionally_sized_box_layout_width_only() {
        use flui_core::testing::mock_render_context;

        let fractional = SingleRenderBox::new(FractionallySizedBoxData::width(0.25));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 200.0);

        let (_tree, ctx) = mock_render_context();
        let size = fractional.layout(constraints, &ctx);

        // Should be 25% width, min height
        assert_eq!(size, Size::new(25.0, 0.0));
    }

    #[test]
    fn test_render_fractionally_sized_box_layout_height_only() {
        use flui_core::testing::mock_render_context;

        let fractional = SingleRenderBox::new(FractionallySizedBoxData::height(0.75));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 200.0);

        let (_tree, ctx) = mock_render_context();
        let size = fractional.layout(constraints, &ctx);

        // Should be min width, 75% height
        assert_eq!(size, Size::new(0.0, 150.0));
    }

    #[test]
    fn test_render_fractionally_sized_box_layout_no_factors() {
        use flui_core::testing::mock_render_context;

        let fractional = SingleRenderBox::new(FractionallySizedBoxData::new(None, None));
        let constraints = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);

        let (_tree, ctx) = mock_render_context();
        let size = fractional.layout(constraints, &ctx);

        // Should use min constraints when no factors
        assert_eq!(size, Size::new(10.0, 20.0));
    }
}
