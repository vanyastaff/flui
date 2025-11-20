//! RenderIntrinsicHeight - sizes child to its intrinsic height

use flui_core::render::{
    {BoxProtocol, LayoutContext, PaintContext},
    RenderBox,
    Single,
};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that sizes child_id to its intrinsic height
///
/// This forces the child_id to be as tall as it "naturally" wants to be,
/// ignoring the parent's height constraints. Useful for making widgets
/// take up only as much vertical space as needed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderIntrinsicHeight;
///
/// // Child will be sized to its intrinsic height
/// let intrinsic = RenderIntrinsicHeight::new();
/// ```
#[derive(Debug)]
pub struct RenderIntrinsicHeight {
    /// Step width (rounds intrinsic width to nearest multiple)
    pub step_width: Option<f32>,
    /// Step height (rounds intrinsic height to nearest multiple)
    pub step_height: Option<f32>,
}

impl RenderIntrinsicHeight {
    /// Create new RenderIntrinsicHeight
    pub fn new() -> Self {
        Self {
            step_width: None,
            step_height: None,
        }
    }

    /// Create with step width
    pub fn with_step_width(step_width: f32) -> Self {
        Self {
            step_width: Some(step_width),
            step_height: None,
        }
    }

    /// Create with step height
    pub fn with_step_height(step_height: f32) -> Self {
        Self {
            step_width: None,
            step_height: Some(step_height),
        }
    }

    /// Create with both step dimensions
    pub fn with_steps(step_width: f32, step_height: f32) -> Self {
        Self {
            step_width: Some(step_width),
            step_height: Some(step_height),
        }
    }

    /// Set step width
    pub fn set_step_width(&mut self, step_width: Option<f32>) {
        self.step_width = step_width;
    }

    /// Set step height
    pub fn set_step_height(&mut self, step_height: Option<f32>) {
        self.step_height = step_height;
    }
}

impl Default for RenderIntrinsicHeight {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBox<Single> for RenderIntrinsicHeight {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        let child_id = ctx.children.single();

        // Layout child with infinite height to get intrinsic height
        let intrinsic_constraints = BoxConstraints::new(
            ctx.constraints.min_width,
            ctx.constraints.max_width,
            0.0,
            f32::INFINITY,
        );

        let child_size = ctx.layout_child(child_id, intrinsic_constraints);

        // Apply step width/height if specified
        let width = if let Some(step) = self.step_width {
            (child_size.width / step).ceil() * step
        } else {
            child_size.width
        };

        let height = if let Some(step) = self.step_height {
            (child_size.height / step).ceil() * step
        } else {
            child_size.height
        };

        // Constrain to parent constraints
        ctx.constraints.constrain(Size::new(width, height))
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        let child_id = ctx.children.single();
        ctx.paint_child(child_id, ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_intrinsic_height_new() {
        let intrinsic = RenderIntrinsicHeight::new();
        assert_eq!(intrinsic.step_width, None);
        assert_eq!(intrinsic.step_height, None);
    }

    #[test]
    fn test_render_intrinsic_height_with_step_width() {
        let intrinsic = RenderIntrinsicHeight::with_step_width(10.0);
        assert_eq!(intrinsic.step_width, Some(10.0));
        assert_eq!(intrinsic.step_height, None);
    }

    #[test]
    fn test_render_intrinsic_height_with_step_height() {
        let intrinsic = RenderIntrinsicHeight::with_step_height(5.0);
        assert_eq!(intrinsic.step_width, None);
        assert_eq!(intrinsic.step_height, Some(5.0));
    }

    #[test]
    fn test_render_intrinsic_height_with_steps() {
        let intrinsic = RenderIntrinsicHeight::with_steps(10.0, 5.0);
        assert_eq!(intrinsic.step_width, Some(10.0));
        assert_eq!(intrinsic.step_height, Some(5.0));
    }

    #[test]
    fn test_render_intrinsic_height_set_step_width() {
        let mut intrinsic = RenderIntrinsicHeight::new();
        intrinsic.set_step_width(Some(8.0));
        assert_eq!(intrinsic.step_width, Some(8.0));
    }

    #[test]
    fn test_render_intrinsic_height_set_step_height() {
        let mut intrinsic = RenderIntrinsicHeight::new();
        intrinsic.set_step_height(Some(4.0));
        assert_eq!(intrinsic.step_height, Some(4.0));
    }
}
