//! RenderLimitedBox - limits max width/height

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{constraints::BoxConstraints, Offset, Size};

/// RenderObject that limits maximum size when unconstrained
///
/// This is useful to prevent a child from becoming infinitely large when
/// placed in an unbounded context. Only applies limits when the incoming
/// constraints are infinite.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderLimitedBox;
///
/// let limited = RenderLimitedBox::new(100.0, 100.0);
/// ```
#[derive(Debug)]
pub struct RenderLimitedBox {
    /// Maximum width when unconstrained
    pub max_width: f32,
    /// Maximum height when unconstrained
    pub max_height: f32,
}

impl RenderLimitedBox {
    /// Create new RenderLimitedBox
    pub fn new(max_width: f32, max_height: f32) -> Self {
        Self {
            max_width,
            max_height,
        }
    }

    /// Set new max width
    pub fn set_max_width(&mut self, max_width: f32) {
        self.max_width = max_width;
    }

    /// Set new max height
    pub fn set_max_height(&mut self, max_height: f32) {
        self.max_height = max_height;
    }
}

impl Default for RenderLimitedBox {
    fn default() -> Self {
        Self {
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }
}

impl SingleRender for RenderLimitedBox {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Apply limits only if constraints are infinite
        let max_width = if constraints.max_width.is_infinite() {
            self.max_width
        } else {
            constraints.max_width
        };
        let max_height = if constraints.max_height.is_infinite() {
            self.max_height
        } else {
            constraints.max_height
        };

        let limited_constraints = BoxConstraints::new(
            constraints.min_width,
            max_width,
            constraints.min_height,
            max_height,
        );

        // SingleArity always has exactly one child
        tree.layout_child(child_id, limited_constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        tree.paint_child(child_id, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_limited_box_new() {
        let limited = RenderLimitedBox::new(100.0, 200.0);
        assert_eq!(limited.max_width, 100.0);
        assert_eq!(limited.max_height, 200.0);
    }

    #[test]
    fn test_render_limited_box_default() {
        let limited = RenderLimitedBox::default();
        assert!(limited.max_width.is_infinite());
        assert!(limited.max_height.is_infinite());
    }

    #[test]
    fn test_render_limited_box_set_max_width() {
        let mut limited = RenderLimitedBox::new(100.0, 200.0);
        limited.set_max_width(150.0);
        assert_eq!(limited.max_width, 150.0);
    }

    #[test]
    fn test_render_limited_box_set_max_height() {
        let mut limited = RenderLimitedBox::new(100.0, 200.0);
        limited.set_max_height(250.0);
        assert_eq!(limited.max_height, 250.0);
    }
}
