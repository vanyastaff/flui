//! RenderAspectRatio - maintains a specific aspect ratio.
//!
//! This render object attempts to size itself to a specific aspect ratio,
//! respecting the constraints from its parent.

use flui_types::{Offset, Size};

use crate::constraints::BoxConstraints;
use crate::containers::BoxChild;
use crate::pipeline::PaintingContext;
use crate::traits::{RenderBox, TextBaseline};

/// A render object that attempts to size itself to a specific aspect ratio.
///
/// The aspect ratio is expressed as width / height. For example, a 16:9
/// aspect ratio would be 16.0 / 9.0 ≈ 1.78.
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `RenderAspectRatio` which extends `RenderProxyBox`.
/// Like Flutter, this stores child directly and delegates size to child.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::basic::RenderAspectRatio;
///
/// // 16:9 aspect ratio
/// let mut aspect = RenderAspectRatio::new(16.0 / 9.0);
///
/// // Square
/// let mut aspect = RenderAspectRatio::new(1.0);
/// ```
#[derive(Debug)]
pub struct RenderAspectRatio {
    /// The child render object using type-safe container.
    child: BoxChild,

    /// Cached size from layout.
    size: Size,

    /// The aspect ratio (width / height).
    aspect_ratio: f32,
}

impl RenderAspectRatio {
    /// Creates a new render aspect ratio.
    ///
    /// The aspect ratio must be positive and finite.
    pub fn new(aspect_ratio: f32) -> Self {
        debug_assert!(
            aspect_ratio > 0.0 && aspect_ratio.is_finite(),
            "Aspect ratio must be positive and finite"
        );
        Self {
            child: BoxChild::new(),
            size: Size::ZERO,
            aspect_ratio,
        }
    }

    // ========================================================================
    // Child access (using type-safe BoxChild container)
    // ========================================================================

    /// Returns a reference to the child, if present.
    pub fn child(&self) -> Option<&dyn RenderBox> {
        self.child.get()
    }

    /// Returns a mutable reference to the child, if present.
    pub fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.child.get_mut()
    }

    /// Sets the child.
    pub fn set_child(&mut self, child: Option<Box<dyn RenderBox>>) {
        self.child.clear();
        if let Some(c) = child {
            self.child.set(c);
        }
    }

    /// Takes the child out of the container.
    pub fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        self.child.take()
    }

    // ========================================================================
    // Aspect ratio configuration
    // ========================================================================

    /// Returns the aspect ratio.
    pub fn aspect_ratio(&self) -> f32 {
        self.aspect_ratio
    }

    /// Sets the aspect ratio.
    pub fn set_aspect_ratio(&mut self, aspect_ratio: f32) {
        debug_assert!(
            aspect_ratio > 0.0 && aspect_ratio.is_finite(),
            "Aspect ratio must be positive and finite"
        );
        if (self.aspect_ratio - aspect_ratio).abs() > f32::EPSILON {
            self.aspect_ratio = aspect_ratio;
            // In real implementation: self.mark_needs_layout();
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Computes the size that satisfies the aspect ratio within constraints.
    fn apply_aspect_ratio(&self, constraints: BoxConstraints) -> Size {
        // Try width-based calculation first
        if constraints.max_width.is_finite() {
            let width = constraints.max_width;
            let height = width / self.aspect_ratio;

            if height >= constraints.min_height && height <= constraints.max_height {
                return Size::new(width, height);
            }
        }

        // Try height-based calculation
        if constraints.max_height.is_finite() {
            let height = constraints.max_height;
            let width = height * self.aspect_ratio;

            if width >= constraints.min_width && width <= constraints.max_width {
                return Size::new(width, height);
            }
        }

        // Fall back to constrained size
        let width = constraints.max_width;
        let height = constraints.max_height;

        if width.is_finite() {
            let computed_height = width / self.aspect_ratio;
            if computed_height <= height || !height.is_finite() {
                return constraints.constrain(Size::new(width, computed_height));
            }
        }

        if height.is_finite() {
            let computed_width = height * self.aspect_ratio;
            return constraints.constrain(Size::new(computed_width, height));
        }

        // If unbounded, use minimum size
        constraints.smallest()
    }

    /// Performs layout.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.apply_aspect_ratio(constraints);
        self.size
    }

    /// Returns constraints for the child (tight to computed size).
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        let size = self.apply_aspect_ratio(constraints);
        BoxConstraints::tight(size)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Child would be painted at offset
        let _ = (context, offset);
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        if height.is_finite() {
            height * self.aspect_ratio
        } else {
            child_width.unwrap_or(0.0)
        }
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        self.compute_min_intrinsic_width(height, child_width)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        if width.is_finite() {
            width / self.aspect_ratio
        } else {
            child_height.unwrap_or(0.0)
        }
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        self.compute_min_intrinsic_height(width, child_height)
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        child_baseline: Option<f32>,
    ) -> Option<f32> {
        child_baseline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aspect_ratio_16_9() {
        let mut aspect = RenderAspectRatio::new(16.0 / 9.0);
        let constraints = BoxConstraints::new(0.0, 160.0, 0.0, 200.0);

        let size = aspect.perform_layout(constraints);

        assert_eq!(size.width, 160.0);
        assert!((size.height - 90.0).abs() < 0.01);
    }

    #[test]
    fn test_aspect_ratio_square() {
        let mut aspect = RenderAspectRatio::new(1.0);
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 200.0);

        let size = aspect.perform_layout(constraints);

        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 100.0);
    }

    #[test]
    fn test_aspect_ratio_height_constrained() {
        let mut aspect = RenderAspectRatio::new(2.0); // width = 2 * height
        let constraints = BoxConstraints::new(0.0, 400.0, 0.0, 50.0);

        let size = aspect.perform_layout(constraints);

        assert_eq!(size.height, 50.0);
        assert_eq!(size.width, 100.0);
    }

    #[test]
    fn test_aspect_ratio_intrinsics() {
        let aspect = RenderAspectRatio::new(2.0);

        assert_eq!(aspect.compute_min_intrinsic_width(50.0, None), 100.0);
        assert_eq!(aspect.compute_max_intrinsic_width(50.0, None), 100.0);
        assert_eq!(aspect.compute_min_intrinsic_height(100.0, None), 50.0);
        assert_eq!(aspect.compute_max_intrinsic_height(100.0, None), 50.0);
    }

    #[test]
    fn test_aspect_ratio_set() {
        let mut aspect = RenderAspectRatio::new(1.0);
        assert_eq!(aspect.aspect_ratio(), 1.0);

        aspect.set_aspect_ratio(2.0);
        assert_eq!(aspect.aspect_ratio(), 2.0);
    }
}
