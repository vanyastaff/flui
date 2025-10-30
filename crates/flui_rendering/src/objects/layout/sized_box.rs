//! RenderSizedBox - enforces exact size

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Size, constraints::BoxConstraints, Offset};

/// RenderObject that enforces exact size constraints
///
/// This widget forces its child_id to have a specific width and/or height,
/// or acts as an invisible spacer if no child_id is present.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSizedBox;
///
/// // Force child_id to be exactly 100x100
/// let sized = RenderSizedBox::exact(100.0, 100.0);
///
/// // Create a 50 pixel wide spacer
/// let spacer = RenderSizedBox::width(50.0);
/// ```
#[derive(Debug)]
pub struct RenderSizedBox {
    /// Explicit width (None = unconstrained)
    pub width: Option<f32>,
    /// Explicit height (None = unconstrained)
    pub height: Option<f32>,
}

impl RenderSizedBox {
    /// Create new RenderSizedBox with optional width and height
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self { width, height }
    }

    /// Create with specific width and height
    pub fn exact(width: f32, height: f32) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
        }
    }

    /// Create with only width specified
    pub fn width(width: f32) -> Self {
        Self {
            width: Some(width),
            height: None,
        }
    }

    /// Create with only height specified
    pub fn height(height: f32) -> Self {
        Self {
            width: None,
            height: Some(height),
        }
    }

    /// Set width
    pub fn set_width(&mut self, width: Option<f32>) {
        self.width = width;
    }

    /// Set height
    pub fn set_height(&mut self, height: Option<f32>) {
        self.height = height;
    }
}

impl Default for RenderSizedBox {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl SingleRender for RenderSizedBox {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
                        // Calculate our size based on explicit width/height
        let width = self.width.unwrap_or(constraints.max_width);
        let height = self.height.unwrap_or(constraints.max_height);

        let size = Size::new(width, height);

        // SingleArity always has exactly one child_id
        // Lay it out with tight constraints
        let child_constraints = BoxConstraints::tight(size);
        tree.layout_child(child_id, child_constraints);

        size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Simply pass through child_id layer (or empty if no child_id)
                tree.paint_child(child_id, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sized_box_new() {
        let sized = RenderSizedBox::new(Some(100.0), Some(50.0));
        assert_eq!(sized.width, Some(100.0));
        assert_eq!(sized.height, Some(50.0));
    }

    #[test]
    fn test_render_sized_box_exact() {
        let sized = RenderSizedBox::exact(100.0, 100.0);
        assert_eq!(sized.width, Some(100.0));
        assert_eq!(sized.height, Some(100.0));
    }

    #[test]
    fn test_render_sized_box_width() {
        let sized = RenderSizedBox::width(50.0);
        assert_eq!(sized.width, Some(50.0));
        assert_eq!(sized.height, None);
    }

    #[test]
    fn test_render_sized_box_height() {
        let sized = RenderSizedBox::height(75.0);
        assert_eq!(sized.width, None);
        assert_eq!(sized.height, Some(75.0));
    }

    #[test]
    fn test_render_sized_box_default() {
        let sized = RenderSizedBox::default();
        assert_eq!(sized.width, None);
        assert_eq!(sized.height, None);
    }

    #[test]
    fn test_render_sized_box_set_width() {
        let mut sized = RenderSizedBox::width(50.0);
        sized.set_width(Some(100.0));
        assert_eq!(sized.width, Some(100.0));
    }

    #[test]
    fn test_render_sized_box_set_height() {
        let mut sized = RenderSizedBox::height(50.0);
        sized.set_height(Some(100.0));
        assert_eq!(sized.height, Some(100.0));
    }
}
