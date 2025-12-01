//! RenderAspectRatio - maintains aspect ratio

use flui_core::render::{BoxProtocol, LayoutContext, PaintContext};
use flui_core::render::{RenderBox, Single};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

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

impl RenderBox<Single> for RenderAspectRatio {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
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

        // Layout child with tight constraints
        ctx.layout_child(child_id, BoxConstraints::tight(final_size));

        final_size
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Simply paint child - no transformation needed
        ctx.paint_child(child_id, offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_aspect_ratio_new() {
        let aspect = RenderAspectRatio::new(16.0 / 9.0);
        assert!((aspect.aspect_ratio - 16.0 / 9.0).abs() < 0.001);
    }

    #[test]
    fn test_render_aspect_ratio_set() {
        let mut aspect = RenderAspectRatio::new(16.0 / 9.0);
        aspect.set_aspect_ratio(4.0 / 3.0);
        assert!((aspect.aspect_ratio - 4.0 / 3.0).abs() < 0.001);
    }

    #[test]
    #[should_panic(expected = "Aspect ratio must be positive")]
    fn test_aspect_ratio_negative() {
        RenderAspectRatio::new(-1.0);
    }

    #[test]
    #[should_panic(expected = "Aspect ratio must be positive")]
    fn test_aspect_ratio_zero() {
        RenderAspectRatio::new(0.0);
    }

    #[test]
    fn test_render_aspect_ratio_default() {
        let aspect = RenderAspectRatio::default();
        assert_eq!(aspect.aspect_ratio, 1.0);
    }
}
