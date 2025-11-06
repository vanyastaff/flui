//! RenderIntrinsicWidth - sizes child_id to its intrinsic width

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{constraints::BoxConstraints, Offset, Size};

/// RenderObject that sizes child_id to its intrinsic width
///
/// This forces the child_id to be as wide as it "naturally" wants to be,
/// ignoring the parent's width constraints. Useful for making text
/// widgets take up only as much space as needed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderIntrinsicWidth;
///
/// // Child will be sized to its intrinsic width
/// let intrinsic = RenderIntrinsicWidth::new();
/// ```
#[derive(Debug)]
pub struct RenderIntrinsicWidth {
    /// Step width (rounds intrinsic width to nearest multiple)
    pub step_width: Option<f32>,
    /// Step height (rounds intrinsic height to nearest multiple)
    pub step_height: Option<f32>,
}

impl RenderIntrinsicWidth {
    /// Create new RenderIntrinsicWidth
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

impl Default for RenderIntrinsicWidth {
    fn default() -> Self {
        Self::new()
    }
}

impl SingleRender for RenderIntrinsicWidth {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // SingleArity always has exactly one child_id
        // Layout child_id with infinite width to get intrinsic width
        // Get child_id's intrinsic width by giving it infinite width
        let intrinsic_constraints = BoxConstraints::new(
            0.0,
            f32::INFINITY,
            constraints.min_height,
            constraints.max_height,
        );

        let child_size = tree.layout_child(child_id, intrinsic_constraints);

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
        constraints.constrain(Size::new(width, height))
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        tree.paint_child(child_id, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_intrinsic_width_new() {
        let intrinsic = RenderIntrinsicWidth::new();
        assert_eq!(intrinsic.step_width, None);
        assert_eq!(intrinsic.step_height, None);
    }

    #[test]
    fn test_render_intrinsic_width_with_step_width() {
        let intrinsic = RenderIntrinsicWidth::with_step_width(10.0);
        assert_eq!(intrinsic.step_width, Some(10.0));
        assert_eq!(intrinsic.step_height, None);
    }

    #[test]
    fn test_render_intrinsic_width_with_step_height() {
        let intrinsic = RenderIntrinsicWidth::with_step_height(5.0);
        assert_eq!(intrinsic.step_width, None);
        assert_eq!(intrinsic.step_height, Some(5.0));
    }

    #[test]
    fn test_render_intrinsic_width_with_steps() {
        let intrinsic = RenderIntrinsicWidth::with_steps(10.0, 5.0);
        assert_eq!(intrinsic.step_width, Some(10.0));
        assert_eq!(intrinsic.step_height, Some(5.0));
    }

    #[test]
    fn test_render_intrinsic_width_set_step_width() {
        let mut intrinsic = RenderIntrinsicWidth::new();
        intrinsic.set_step_width(Some(8.0));
        assert_eq!(intrinsic.step_width, Some(8.0));
    }

    #[test]
    fn test_render_intrinsic_width_set_step_height() {
        let mut intrinsic = RenderIntrinsicWidth::new();
        intrinsic.set_step_height(Some(4.0));
        assert_eq!(intrinsic.step_height, Some(4.0));
    }
}
