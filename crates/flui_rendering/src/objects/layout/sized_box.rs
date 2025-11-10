//! RenderSizedBox - enforces exact size constraints

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_engine::BoxedLayer;
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that enforces exact size constraints
///
/// This render object forces its child to have a specific width and/or height.
/// If width or height is None, that dimension uses the constraint's max value.
///
/// # Layout Behavior
///
/// - **Both specified**: Forces exact size (tight constraints)
/// - **Width only**: Sets width, height fills constraint
/// - **Height only**: Sets height, width fills constraint
/// - **Neither specified**: Fills max constraints (same as unconstrained child)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSizedBox;
///
/// // Force child to be exactly 100x100
/// let sized = RenderSizedBox::exact(100.0, 100.0);
///
/// // Set width only, height flexible
/// let wide = RenderSizedBox::width(200.0);
///
/// // Set height only, width flexible
/// let tall = RenderSizedBox::height(150.0);
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

impl Render for RenderSizedBox {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Calculate our size based on explicit width/height
        // If not specified, use constraint's max (fill available space)
        let width = self.width.unwrap_or(constraints.max_width);
        let height = self.height.unwrap_or(constraints.max_height);

        let size = Size::new(width, height);

        // Force child to be exactly this size with tight constraints
        let child_constraints = BoxConstraints::tight(size);
        tree.layout_child(child_id, child_constraints);

        size
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Pass-through: child painted at our offset
        tree.paint_child(child_id, offset)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable // Default - update if needed
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

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn arity(&self) -> Arity {
            Arity::Exact(1)
        }
    }
}
