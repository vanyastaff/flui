//! RenderAspectRatio - maintains aspect ratio

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints};

/// RenderObject that maintains an aspect ratio
///
/// Sizes the child to maintain the specified aspect ratio (width / height).
/// For example, an aspect ratio of 16/9 = 1.777... will maintain a 16:9 ratio.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAspectRatio;
///
/// // 16:9 aspect ratio
/// let aspect = RenderAspectRatio::new(16.0 / 9.0);
/// ```
#[derive(Debug)]
pub struct RenderAspectRatio {
    /// The aspect ratio to maintain (width / height)
    pub aspect_ratio: f32,
}

impl RenderAspectRatio {
    /// Create new RenderAspectRatio
    pub fn new(aspect_ratio: f32) -> Self {
        assert!(aspect_ratio > 0.0, "Aspect ratio must be positive");
        Self { aspect_ratio }
    }

    /// Set new aspect ratio
    pub fn set_aspect_ratio(&mut self, aspect_ratio: f32) {
        assert!(aspect_ratio > 0.0, "Aspect ratio must be positive");
        self.aspect_ratio = aspect_ratio;
    }
}

impl Default for RenderAspectRatio {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl SingleRender for RenderAspectRatio {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        let aspect_ratio = self.aspect_ratio;

        // Calculate size maintaining aspect ratio
        let size = if constraints.is_tight() {
            // If constraints are tight, we must use them exactly
            constraints.smallest()
        } else {
            // Try to fill available space while maintaining aspect ratio
            let width = constraints.max_width;
            let height = width / aspect_ratio;

            if height <= constraints.max_height {
                // Width-based size fits
                Size::new(width, height)
            } else {
                // Use height-based size
                let height = constraints.max_height;
                let width = height * aspect_ratio;
                Size::new(width, height)
            }
        };

        // Constrain to bounds
        let final_size = constraints.constrain(size);

        // SingleArity always has exactly one child
        // Layout child with tight constraints
        tree.layout_child(child_id, BoxConstraints::tight(final_size));

        final_size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Simply return child layer - no transformation needed
        (tree.paint_child(child_id, offset)) as _
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_aspect_ratio_new() {
        let aspect = RenderAspectRatio::new(2.0);
        assert!((aspect.aspect_ratio - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_render_aspect_ratio_default() {
        let aspect = RenderAspectRatio::default();
        assert!((aspect.aspect_ratio - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    #[should_panic(expected = "Aspect ratio must be positive")]
    fn test_render_aspect_ratio_new_zero() {
        RenderAspectRatio::new(0.0);
    }

    #[test]
    #[should_panic(expected = "Aspect ratio must be positive")]
    fn test_render_aspect_ratio_new_negative() {
        RenderAspectRatio::new(-1.0);
    }

    #[test]
    fn test_render_aspect_ratio_set() {
        let mut aspect = RenderAspectRatio::new(16.0 / 9.0);
        aspect.set_aspect_ratio(4.0 / 3.0);
        assert!((aspect.aspect_ratio - 4.0 / 3.0).abs() < f32::EPSILON);
    }
}
