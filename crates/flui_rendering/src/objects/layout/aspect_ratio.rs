//! RenderAspectRatio - maintains aspect ratio
//!
//! Flutter equivalent: `RenderAspectRatio`
//! Source: https://api.flutter.dev/flutter/rendering/RenderAspectRatio-class.html

use crate::core::{BoxProtocol, LayoutContext, PaintContext};
use crate::core::{FullRenderTree, RenderBox, Single};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that maintains an aspect ratio
///
/// Sizes the child to maintain the specified aspect ratio (width / height).
/// For example, an aspect ratio of 16/9 = 1.777... will maintain a 16:9 ratio.
///
/// # Layout Algorithm (Flutter-compatible)
///
/// 1. First tries the largest width permitted by constraints
/// 2. Height is calculated by applying aspect ratio to width
/// 3. If height violates constraints, recalculates using height-first approach
/// 4. Iteratively refines until constraints are satisfied
/// 5. Falls back to constraint-satisfying size if no feasible ratio-preserving size exists
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

    /// Apply aspect ratio constraints to find best size
    /// Flutter-compatible algorithm that tries multiple approaches
    fn apply_aspect_ratio(&self, constraints: &BoxConstraints) -> Size {
        let aspect_ratio = self.aspect_ratio;

        // If constraints are tight, we must use them exactly
        if constraints.is_tight() {
            return constraints.smallest();
        }

        // Try width-first: use max width and calculate height
        let mut width = constraints.max_width;
        let mut height = width / aspect_ratio;

        // Check if height violates max constraint
        if height > constraints.max_height {
            // Height-first: use max height and calculate width
            height = constraints.max_height;
            width = height * aspect_ratio;
        }

        // Check if width violates max constraint (from height-first calculation)
        if width > constraints.max_width {
            width = constraints.max_width;
            height = width / aspect_ratio;
        }

        // Now check minimum constraints
        // If width is too small, try increasing it
        if width < constraints.min_width {
            width = constraints.min_width;
            height = width / aspect_ratio;
        }

        // If height is too small, try increasing it
        if height < constraints.min_height {
            height = constraints.min_height;
            width = height * aspect_ratio;
        }

        // Final constraint check - if we still can't satisfy constraints,
        // prioritize satisfying constraints over aspect ratio
        let final_size = Size::new(
            width.clamp(constraints.min_width, constraints.max_width),
            height.clamp(constraints.min_height, constraints.max_height),
        );

        tracing::trace!(
            aspect_ratio = aspect_ratio,
            constraints = ?constraints,
            calculated_size = ?final_size,
            "RenderAspectRatio::apply_aspect_ratio"
        );

        final_size
    }
}

impl Default for RenderAspectRatio {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl<T: FullRenderTree> RenderBox<T, Single> for RenderAspectRatio {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;

        // Calculate size maintaining aspect ratio with Flutter-compatible algorithm
        let final_size = self.apply_aspect_ratio(&constraints);

        // Layout child with tight constraints
        ctx.layout_child(child_id, BoxConstraints::tight(final_size));

        final_size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
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

    #[test]
    fn test_apply_aspect_ratio_width_first() {
        let aspect = RenderAspectRatio::new(2.0); // 2:1 ratio
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let size = aspect.apply_aspect_ratio(&constraints);
        // Width 100, height should be 50 (100 / 2)
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 50.0);
    }

    #[test]
    fn test_apply_aspect_ratio_height_limited() {
        let aspect = RenderAspectRatio::new(0.5); // 1:2 ratio (width:height)
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let size = aspect.apply_aspect_ratio(&constraints);
        // Height would be 200 (100 / 0.5), but max is 100
        // So height = 100, width = 50 (100 * 0.5)
        assert_eq!(size.width, 50.0);
        assert_eq!(size.height, 100.0);
    }

    #[test]
    fn test_apply_aspect_ratio_min_width() {
        let aspect = RenderAspectRatio::new(2.0); // 2:1 ratio
        let constraints = BoxConstraints::new(80.0, 100.0, 0.0, 30.0);
        let size = aspect.apply_aspect_ratio(&constraints);
        // Max height 30 -> width would be 60, but min_width is 80
        // So width = 80, height clamped to 30
        assert!(size.width >= 80.0);
        assert!(size.height <= 30.0);
    }

    #[test]
    fn test_apply_aspect_ratio_tight_constraints() {
        let aspect = RenderAspectRatio::new(2.0);
        let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));
        let size = aspect.apply_aspect_ratio(&constraints);
        // Tight constraints must be used exactly
        assert_eq!(size.width, 50.0);
        assert_eq!(size.height, 50.0);
    }
}
