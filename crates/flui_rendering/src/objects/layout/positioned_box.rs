//! RenderPositionedBox - positions child with explicit coordinates

use flui_types::{Offset, Size};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{BoxedLayer, TransformLayer};

/// RenderObject that positions child with explicit coordinates
///
/// This is typically used inside a Stack to position a child at specific
/// coordinates. The coordinates can be absolute (left, top, right, bottom)
/// or combined with explicit width/height.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderPositionedBox;
///
/// let positioned = RenderPositionedBox::at(10.0, 20.0);
/// ```
#[derive(Debug)]
pub struct RenderPositionedBox {
    /// Distance from left edge
    pub left: Option<f32>,
    /// Distance from top edge
    pub top: Option<f32>,
    /// Distance from right edge
    pub right: Option<f32>,
    /// Distance from bottom edge
    pub bottom: Option<f32>,
    /// Explicit width
    pub width: Option<f32>,
    /// Explicit height
    pub height: Option<f32>,
}

impl RenderPositionedBox {
    /// Create new RenderPositionedBox
    pub fn new() -> Self {
        Self {
            left: None,
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Create with left and top
    pub fn at(left: f32, top: f32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            ..Self::new()
        }
    }

    /// Create with all edges
    pub fn fill(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            right: Some(right),
            bottom: Some(bottom),
            width: None,
            height: None,
        }
    }

    /// Set left position
    pub fn set_left(&mut self, left: Option<f32>) {
        self.left = left;
    }

    /// Set top position
    pub fn set_top(&mut self, top: Option<f32>) {
        self.top = top;
    }
}

impl Default for RenderPositionedBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderPositionedBox {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let constraints = cx.constraints();

        // Calculate child constraints based on positioning
        let child_constraints = if let (Some(left), Some(right)) = (self.left, self.right) {
            // Width determined by left and right
            let width = (constraints.max_width - left - right).max(0.0);
            constraints.tighten(Some(width), None)
        } else if let Some(width) = self.width {
            // Explicit width
            constraints.tighten(Some(width), None)
        } else {
            // Unconstrained width
            constraints
        };

        let child_constraints = if let (Some(top), Some(bottom)) = (self.top, self.bottom) {
            // Height determined by top and bottom
            let height = (constraints.max_height - top - bottom).max(0.0);
            child_constraints.tighten(None, Some(height))
        } else if let Some(height) = self.height {
            // Explicit height
            child_constraints.tighten(None, Some(height))
        } else {
            // Unconstrained height
            child_constraints
        };

        // Layout child (SingleArity always has a child)
        let child = cx.child();
        cx.layout_child(child, child_constraints)
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);

        // Calculate paint offset based on positioning
        let offset = Offset::new(
            self.left.unwrap_or(0.0),
            self.top.unwrap_or(0.0),
        );

        // Use TransformLayer to position child
        Box::new(TransformLayer::translate(child_layer, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_positioned_box_new() {
        let positioned = RenderPositionedBox::new();
        assert_eq!(positioned.left, None);
        assert_eq!(positioned.top, None);
        assert_eq!(positioned.right, None);
        assert_eq!(positioned.bottom, None);
    }

    #[test]
    fn test_render_positioned_box_at() {
        let positioned = RenderPositionedBox::at(10.0, 20.0);
        assert_eq!(positioned.left, Some(10.0));
        assert_eq!(positioned.top, Some(20.0));
    }

    #[test]
    fn test_render_positioned_box_fill() {
        let positioned = RenderPositionedBox::fill(10.0, 20.0, 30.0, 40.0);
        assert_eq!(positioned.left, Some(10.0));
        assert_eq!(positioned.top, Some(20.0));
        assert_eq!(positioned.right, Some(30.0));
        assert_eq!(positioned.bottom, Some(40.0));
    }

    #[test]
    fn test_render_positioned_box_set_left() {
        let mut positioned = RenderPositionedBox::new();
        positioned.set_left(Some(15.0));
        assert_eq!(positioned.left, Some(15.0));
    }

    #[test]
    fn test_render_positioned_box_set_top() {
        let mut positioned = RenderPositionedBox::new();
        positioned.set_top(Some(25.0));
        assert_eq!(positioned.top, Some(25.0));
    }
}
