//! RenderFractionallySizedBox - sizes child as fraction of parent

use crate::core::{
    RenderBox, Single, {BoxLayoutCtx, BoxPaintCtx},
};
use flui_types::Size;

/// RenderObject that sizes child_id as a fraction of available space
///
/// This is useful for making a child_id take up a percentage of its parent.
/// For example, width_factor: 0.5 makes the child_id half the parent's width.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderFractionallySizedBox;
///
/// // 50% width and height
/// let fractional = RenderFractionallySizedBox::new(Some(0.5), Some(0.5));
/// ```
#[derive(Debug)]
pub struct RenderFractionallySizedBox {
    /// Width factor (0.0 - 1.0), None means unconstrained
    pub width_factor: Option<f32>,
    /// Height factor (0.0 - 1.0), None means unconstrained
    pub height_factor: Option<f32>,
}

impl RenderFractionallySizedBox {
    /// Create new RenderFractionallySizedBox
    pub fn new(width_factor: Option<f32>, height_factor: Option<f32>) -> Self {
        if let Some(w) = width_factor {
            assert!(
                (0.0..=1.0).contains(&w),
                "Width factor must be between 0.0 and 1.0"
            );
        }
        if let Some(h) = height_factor {
            assert!(
                (0.0..=1.0).contains(&h),
                "Height factor must be between 0.0 and 1.0"
            );
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

    /// Set new width factor
    pub fn set_width_factor(&mut self, factor: Option<f32>) {
        if let Some(w) = factor {
            assert!(
                (0.0..=1.0).contains(&w),
                "Width factor must be between 0.0 and 1.0"
            );
        }
        self.width_factor = factor;
    }

    /// Set new height factor
    pub fn set_height_factor(&mut self, factor: Option<f32>) {
        if let Some(h) = factor {
            assert!(
                (0.0..=1.0).contains(&h),
                "Height factor must be between 0.0 and 1.0"
            );
        }
        self.height_factor = factor;
    }
}

impl Default for RenderFractionallySizedBox {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl RenderBox<Single> for RenderFractionallySizedBox {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Single>) -> Size {
        let child_id = ctx.children.single();

        // Calculate target size based on factors
        let target_width = self.width_factor.map(|f| ctx.constraints.max_width * f);
        let target_height = self.height_factor.map(|f| ctx.constraints.max_height * f);

        // Create child constraints
        let child_constraints = ctx.constraints.tighten(target_width, target_height);

        // Single arity always has exactly one child
        ctx.layout_child(child_id, child_constraints)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        let child_id = ctx.children.single();
        ctx.paint_child(child_id, ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_fractionally_sized_box_new() {
        let fractional = RenderFractionallySizedBox::new(Some(0.5), Some(0.75));
        assert_eq!(fractional.width_factor, Some(0.5));
        assert_eq!(fractional.height_factor, Some(0.75));
    }

    #[test]
    fn test_render_fractionally_sized_box_both() {
        let fractional = RenderFractionallySizedBox::both(0.5);
        assert_eq!(fractional.width_factor, Some(0.5));
        assert_eq!(fractional.height_factor, Some(0.5));
    }

    #[test]
    fn test_render_fractionally_sized_box_width() {
        let fractional = RenderFractionallySizedBox::width(0.5);
        assert_eq!(fractional.width_factor, Some(0.5));
        assert_eq!(fractional.height_factor, None);
    }

    #[test]
    fn test_render_fractionally_sized_box_height() {
        let fractional = RenderFractionallySizedBox::height(0.75);
        assert_eq!(fractional.width_factor, None);
        assert_eq!(fractional.height_factor, Some(0.75));
    }

    #[test]
    #[should_panic(expected = "Width factor must be between 0.0 and 1.0")]
    fn test_render_fractionally_sized_box_invalid_width() {
        RenderFractionallySizedBox::new(Some(1.5), None);
    }

    #[test]
    #[should_panic(expected = "Height factor must be between 0.0 and 1.0")]
    fn test_render_fractionally_sized_box_invalid_height() {
        RenderFractionallySizedBox::new(None, Some(-0.1));
    }

    #[test]
    fn test_render_fractionally_sized_box_set_factors() {
        let mut fractional = RenderFractionallySizedBox::both(0.5);
        fractional.set_width_factor(Some(0.75));
        assert_eq!(fractional.width_factor, Some(0.75));
    }
}
