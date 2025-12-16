//! RenderFractionalTranslation - translates child by a fraction of its size.
//!
//! This render object translates its child by a fraction of the child's own dimensions.

use flui_types::{Offset, Point, Rect, Size};

use crate::constraints::BoxConstraints;
use crate::containers::BoxChild;
use crate::pipeline::PaintingContext;
use crate::traits::{RenderBox, TextBaseline};

/// A render object that translates its child by a fraction of the child's size.
///
/// Unlike regular translation which uses absolute pixels, this uses the child's
/// dimensions. For example, a translation of (1.0, 0.0) moves the child to the
/// right by its own width.
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `RenderFractionalTranslation` which extends `RenderProxyBox`.
/// Like Flutter, this stores child directly and delegates size to child.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::RenderFractionalTranslation;
/// use flui_types::Offset;
///
/// // Move child half its width to the right
/// let translation = RenderFractionalTranslation::new(Offset::new(0.5, 0.0));
///
/// // Move child completely off to the left
/// let offscreen = RenderFractionalTranslation::new(Offset::new(-1.0, 0.0));
/// ```
#[derive(Debug)]
pub struct RenderFractionalTranslation {
    /// The child render object.
    child: BoxChild,

    /// Cached size from layout.
    size: Size,

    /// The translation as a fraction of child size.
    translation: Offset,

    /// Whether hit tests should be transformed.
    transform_hit_tests: bool,
}

impl RenderFractionalTranslation {
    /// Creates a new fractional translation.
    pub fn new(translation: Offset) -> Self {
        Self {
            child: BoxChild::new(),
            size: Size::ZERO,
            translation,
            transform_hit_tests: true,
        }
    }

    /// Creates with no translation.
    pub fn none() -> Self {
        Self::new(Offset::ZERO)
    }

    // ========================================================================
    // Child access
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

    /// Takes the child.
    pub fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        self.child.take()
    }

    // ========================================================================
    // Translation configuration
    // ========================================================================

    /// Returns the translation fraction.
    pub fn translation(&self) -> Offset {
        self.translation
    }

    /// Sets the translation fraction.
    pub fn set_translation(&mut self, translation: Offset) {
        if self.translation != translation {
            self.translation = translation;
        }
    }

    /// Returns whether hit tests are transformed.
    pub fn transform_hit_tests(&self) -> bool {
        self.transform_hit_tests
    }

    /// Sets whether hit tests should be transformed.
    pub fn set_transform_hit_tests(&mut self, value: bool) {
        self.transform_hit_tests = value;
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Computes the actual pixel offset for the current size.
    pub fn compute_offset(&self) -> Offset {
        let size = self.size();
        Offset::new(
            size.width * self.translation.dx,
            size.height * self.translation.dy,
        )
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = constraints.smallest();
        self.size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        _constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        self.size = child_size;
        self.size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        let translation = self.compute_offset();
        let child_offset = Offset::new(offset.dx + translation.dx, offset.dy + translation.dy);

        // In real implementation: context.paint_child(child, child_offset);
        let _ = (context, child_offset);
    }

    /// Hit test with translation applied.
    pub fn hit_test(&self, position: Offset) -> bool {
        let size = self.size();
        let rect = Rect::from_origin_size(Point::ZERO, size);

        if self.transform_hit_tests {
            // Transform position back to child space
            let translation = self.compute_offset();
            let local = Offset::new(position.dx - translation.dx, position.dy - translation.dy);
            rect.contains(Point::new(local.dx, local.dy))
        } else {
            // Hit test in original position
            rect.contains(Point::new(position.dx, position.dy))
        }
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
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

impl Default for RenderFractionalTranslation {
    fn default() -> Self {
        Self::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fractional_translation_new() {
        let translation = RenderFractionalTranslation::new(Offset::new(0.5, 0.25));
        assert_eq!(translation.translation(), Offset::new(0.5, 0.25));
    }

    #[test]
    fn test_compute_offset() {
        let mut translation = RenderFractionalTranslation::new(Offset::new(0.5, 0.25));
        translation.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 80.0)),
            Size::new(100.0, 80.0),
        );

        let offset = translation.compute_offset();

        assert!((offset.dx - 50.0).abs() < f32::EPSILON);
        assert!((offset.dy - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hit_test_with_transform() {
        let mut translation = RenderFractionalTranslation::new(Offset::new(0.5, 0.0));
        translation.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        // Child is translated 50px to the right
        // Original position (0,0) now appears at (50, 0)
        // Point at (25, 50) should NOT hit because it's before the translated child
        assert!(!translation.hit_test(Offset::new(25.0, 50.0)));

        // Point at (75, 50) should hit (it's within the translated child)
        assert!(translation.hit_test(Offset::new(75.0, 50.0)));
    }

    #[test]
    fn test_hit_test_without_transform() {
        let mut translation = RenderFractionalTranslation::new(Offset::new(0.5, 0.0));
        translation.set_transform_hit_tests(false);
        translation.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        // Without transform, hit tests use original bounds
        assert!(translation.hit_test(Offset::new(25.0, 50.0)));
        assert!(translation.hit_test(Offset::new(75.0, 50.0)));
    }

    #[test]
    fn test_layout() {
        let mut translation = RenderFractionalTranslation::new(Offset::new(1.0, 1.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);
        let child_size = Size::new(100.0, 75.0);

        let size = translation.perform_layout_with_child(constraints, child_size);

        assert_eq!(size, child_size);
    }

    #[test]
    fn test_negative_translation() {
        let mut translation = RenderFractionalTranslation::new(Offset::new(-0.5, -0.5));
        translation.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        let offset = translation.compute_offset();

        assert!((offset.dx - (-50.0)).abs() < f32::EPSILON);
        assert!((offset.dy - (-50.0)).abs() < f32::EPSILON);
    }
}
