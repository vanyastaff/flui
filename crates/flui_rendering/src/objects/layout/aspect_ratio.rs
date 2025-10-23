//! RenderAspectRatio - maintains aspect ratio

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderAspectRatio
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AspectRatioData {
    /// The aspect ratio to maintain (width / height)
    pub aspect_ratio: f32,
}

impl AspectRatioData {
    /// Create new aspect ratio data
    pub fn new(aspect_ratio: f32) -> Self {
        assert!(aspect_ratio > 0.0, "Aspect ratio must be positive");
        Self { aspect_ratio }
    }
}

/// RenderObject that maintains an aspect ratio
///
/// Sizes the child to maintain the specified aspect ratio (width / height).
/// For example, an aspect ratio of 16/9 = 1.777... will maintain a 16:9 ratio.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::AspectRatioData};
///
/// // 16:9 aspect ratio
/// let mut aspect = SingleRenderBox::new(AspectRatioData::new(16.0 / 9.0));
/// ```
pub type RenderAspectRatio = SingleRenderBox<AspectRatioData>;

// ===== Public API =====

impl RenderAspectRatio {
    /// Get the aspect ratio
    pub fn aspect_ratio(&self) -> f32 {
        self.data().aspect_ratio
    }

    /// Set new aspect ratio
    ///
    /// If aspect ratio changes, marks as needing layout.
    pub fn set_aspect_ratio(&mut self, aspect_ratio: f32) {
        assert!(aspect_ratio > 0.0, "Aspect ratio must be positive");
        if (self.data().aspect_ratio - aspect_ratio).abs() > f32::EPSILON {
            self.data_mut().aspect_ratio = aspect_ratio;
            self.mark_needs_layout();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderAspectRatio {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        let aspect_ratio = self.data().aspect_ratio;

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

        // Layout child with tight constraints if we have one
        let children_ids = ctx.children();
        if let Some(&child_id) = children_ids.first() {
            let child_constraints = BoxConstraints::tight(final_size);
            let _ = ctx.layout_child_cached(child_id, child_constraints, None);
        }

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(final_size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        final_size
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
    fn test_aspect_ratio_data_new() {
        let data = AspectRatioData::new(16.0 / 9.0);
        assert!((data.aspect_ratio - 16.0 / 9.0).abs() < f32::EPSILON);
    }

    #[test]
    #[should_panic(expected = "Aspect ratio must be positive")]
    fn test_aspect_ratio_data_new_zero() {
        AspectRatioData::new(0.0);
    }

    #[test]
    #[should_panic(expected = "Aspect ratio must be positive")]
    fn test_aspect_ratio_data_new_negative() {
        AspectRatioData::new(-1.0);
    }

    #[test]
    fn test_render_aspect_ratio_new() {
        let aspect = SingleRenderBox::new(AspectRatioData::new(2.0));
        assert!((aspect.aspect_ratio() - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_render_aspect_ratio_set_aspect_ratio() {
        let mut aspect = SingleRenderBox::new(AspectRatioData::new(16.0 / 9.0));

        aspect.set_aspect_ratio(4.0 / 3.0);
        assert!((aspect.aspect_ratio() - 4.0 / 3.0).abs() < f32::EPSILON);
        assert!(aspect.needs_layout());
    }

    #[test]
    fn test_render_aspect_ratio_layout_width_constrained() {
        use flui_core::testing::mock_render_context;

        let aspect = SingleRenderBox::new(AspectRatioData::new(2.0)); // 2:1 ratio
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 200.0);

        let (_tree, ctx) = mock_render_context();
        let size = aspect.layout(constraints, &ctx);

        // Should use max width and calculate height
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 50.0); // 100 / 2.0
    }

    #[test]
    fn test_render_aspect_ratio_layout_height_constrained() {
        use flui_core::testing::mock_render_context;

        let aspect = SingleRenderBox::new(AspectRatioData::new(0.5)); // 1:2 ratio
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = aspect.layout(constraints, &ctx);

        // Should use max height and calculate width
        assert_eq!(size.width, 50.0); // 100 * 0.5
        assert_eq!(size.height, 100.0);
    }

    #[test]
    fn test_render_aspect_ratio_layout_tight_constraints() {
        use flui_core::testing::mock_render_context;

        let aspect = SingleRenderBox::new(AspectRatioData::new(16.0 / 9.0));
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        let (_tree, ctx) = mock_render_context();
        let size = aspect.layout(constraints, &ctx);

        // With tight constraints, must use exact size
        assert_eq!(size, Size::new(100.0, 100.0));
    }
}
