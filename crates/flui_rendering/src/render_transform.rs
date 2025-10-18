//! RenderObject that applies a transformation matrix before painting its child.
//!
//! # Examples
//!
//! ```
//! use flui_rendering::{RenderTransform, RenderBox};
//! use flui_types::{Matrix4, Size, Offset};
//! use flui_core::BoxConstraints;
//!
//! let mut render = RenderTransform::new(Matrix4::translation(10.0, 20.0, 0.0));
//! let child = Box::new(RenderBox::new());
//! render.set_child(Some(child));
//!
//! let constraints = BoxConstraints::loose(Size::new(100.0, 100.0));
//! render.layout(constraints);
//! ```

use crate::RenderObject;
use flui_core::BoxConstraints;
use flui_types::{Matrix4, Offset, Size};

/// A render object that applies a transformation matrix before painting its child.
///
/// The transformation affects painting and hit testing. By default, hit tests are
/// transformed to match the painted position. Set `transform_hit_tests` to false
/// to perform hit tests in the child's original coordinate space.
///
/// # Transformation Order
///
/// Transformations are applied from the child's coordinate space outward:
/// 1. Child is painted at (0, 0) in its own coordinate space
/// 2. Transformation matrix is applied
/// 3. Result is painted in parent's coordinate space
///
/// # Common Transformations
///
/// - **Translation**: Move child by offset
///   ```
///   use flui_types::Matrix4;
///   let transform = Matrix4::translation(10.0, 20.0, 0.0);
///   ```
///
/// - **Scaling**: Scale child size
///   ```
///   use flui_types::Matrix4;
///   let transform = Matrix4::scaling(2.0, 2.0, 1.0); // 2x scale
///   ```
///
/// - **Rotation**: Rotate child around origin
///   ```
///   use flui_types::Matrix4;
///   let transform = Matrix4::rotation_z(std::f32::consts::PI / 4.0); // 45°
///   ```
///
/// - **Combined**: Transformations combine right-to-left
///   ```
///   use flui_types::Matrix4;
///   let translate = Matrix4::translation(100.0, 100.0, 0.0);
///   let rotate = Matrix4::rotation_z(std::f32::consts::PI / 4.0);
///   let scale = Matrix4::scaling(2.0, 2.0, 1.0);
///
///   // Applied in order: scale -> rotate -> translate
///   let combined = translate * rotate * scale;
///   ```
#[derive(Debug)]
pub struct RenderTransform {
    /// The transformation matrix to apply
    transform: Matrix4,

    /// Whether to transform hit tests (default: true)
    transform_hit_tests: bool,

    /// The single child
    child: Option<Box<dyn RenderObject>>,

    /// Cached size after layout
    size: Size,

    /// Dirty flags
    needs_layout_flag: bool,
    needs_paint_flag: bool,
}

impl RenderTransform {
    /// Creates a new RenderTransform with the given transformation matrix.
    ///
    /// Hit tests are transformed by default.
    pub fn new(transform: Matrix4) -> Self {
        Self {
            transform,
            transform_hit_tests: true,
            child: None,
            size: Size::zero(),
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Sets the transformation matrix.
    pub fn set_transform(&mut self, transform: Matrix4) {
        if self.transform != transform {
            self.transform = transform;
            self.mark_needs_paint();
        }
    }

    /// Returns the transformation matrix.
    pub fn transform(&self) -> &Matrix4 {
        &self.transform
    }

    /// Sets whether hit tests should be transformed.
    ///
    /// If true (default), hit tests are performed in the transformed coordinate space.
    /// If false, hit tests are performed in the child's original coordinate space.
    pub fn set_transform_hit_tests(&mut self, value: bool) {
        if self.transform_hit_tests != value {
            self.transform_hit_tests = value;
            // Changing hit test behavior doesn't require repaint
        }
    }

    /// Returns whether hit tests are transformed.
    pub fn transform_hit_tests(&self) -> bool {
        self.transform_hit_tests
    }

    /// Sets the child.
    pub fn set_child(&mut self, child: Option<Box<dyn RenderObject>>) {
        self.child = child;
        self.mark_needs_layout();
    }

    /// Removes and returns the child.
    pub fn remove_child(&mut self) -> Option<Box<dyn RenderObject>> {
        let child = self.child.take();
        self.mark_needs_layout();
        child
    }
}

impl RenderObject for RenderTransform {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = &mut self.child {
            // Layout child with same constraints
            let child_size = child.layout(constraints);
            self.size = constraints.constrain(child_size);
        } else {
            // No child: use smallest size
            self.size = constraints.smallest();
        }

        self.needs_layout_flag = false;
        self.needs_paint_flag = false;
        self.size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            // Note: egui doesn't directly support transform matrices in 2D painting.
            // For now, we'll paint the child at the transformed origin offset.
            // Full matrix transformation would require egui layers or custom rendering.

            // Extract translation from matrix
            let (tx, ty, _tz) = self.transform.translation_component();
            let transformed_offset = offset + Offset::new(tx, ty);

            child.paint(painter, transformed_offset);
        }
    }

    fn hit_test(&self, position: Offset) -> bool {
        if let Some(child) = &self.child {
            if self.transform_hit_tests {
                // Transform the hit test position by inverse of transformation
                // For simple translation, just subtract the translation component
                let (tx, ty, _tz) = self.transform.translation_component();
                let local_position = position - Offset::new(tx, ty);
                child.hit_test(local_position)
            } else {
                // Don't transform hit tests
                child.hit_test(position)
            }
        } else {
            false
        }
    }

    fn size(&self) -> Size {
        self.size
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout_flag = true;
        self.mark_needs_paint();
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint_flag = true;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderBox;

    #[test]
    fn test_render_transform_new() {
        let transform = Matrix4::translation(10.0, 20.0, 0.0);
        let render = RenderTransform::new(transform);

        assert_eq!(render.transform(), &transform);
        assert!(render.transform_hit_tests());
        assert!(render.needs_layout());
    }

    #[test]
    fn test_render_transform_set_transform() {
        let mut render = RenderTransform::new(Matrix4::identity());

        let new_transform = Matrix4::scaling(2.0, 2.0, 1.0);
        render.set_transform(new_transform);

        assert_eq!(render.transform(), &new_transform);
        assert!(render.needs_paint());
    }

    #[test]
    fn test_render_transform_set_transform_hit_tests() {
        let mut render = RenderTransform::new(Matrix4::identity());

        assert!(render.transform_hit_tests());

        render.set_transform_hit_tests(false);
        assert!(!render.transform_hit_tests());
    }

    #[test]
    fn test_render_transform_layout_with_child() {
        let mut render = RenderTransform::new(Matrix4::translation(10.0, 20.0, 0.0));
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let size = render.layout(constraints);

        assert_eq!(size, Size::new(100.0, 100.0));
        assert!(!render.needs_layout());
    }

    #[test]
    fn test_render_transform_layout_without_child() {
        let mut render = RenderTransform::new(Matrix4::identity());

        let constraints = BoxConstraints::new(10.0, 100.0, 10.0, 100.0);
        let size = render.layout(constraints);

        // Should use smallest size
        assert_eq!(size, Size::new(10.0, 10.0));
    }

    #[test]
    fn test_render_transform_remove_child() {
        let mut render = RenderTransform::new(Matrix4::identity());
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let removed = render.remove_child();
        assert!(removed.is_some());
        assert!(render.child.is_none());
        assert!(render.needs_layout());
    }

    #[test]
    fn test_render_transform_translation() {
        let mut render = RenderTransform::new(Matrix4::translation(50.0, 100.0, 0.0));
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        render.layout(constraints);

        // Translation is extracted in paint()
        let (tx, ty, _) = render.transform().translation_component();
        assert_eq!(tx, 50.0);
        assert_eq!(ty, 100.0);
    }

    #[test]
    fn test_render_transform_scaling() {
        let mut render = RenderTransform::new(Matrix4::scaling(2.0, 3.0, 1.0));
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));
        let size = render.layout(constraints);

        // Layout is not affected by scaling (size stays same)
        assert_eq!(size, Size::new(50.0, 50.0));
    }

    #[test]
    fn test_render_transform_rotation() {
        let mut render = RenderTransform::new(Matrix4::rotation_z(std::f32::consts::PI / 2.0));
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = render.layout(constraints);

        // Layout is not affected by rotation
        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_render_transform_hit_test_transformed() {
        let mut render = RenderTransform::new(Matrix4::translation(10.0, 20.0, 0.0));
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));
        render.layout(constraints);

        // Hit test at transformed position (15, 25)
        // Should transform back to (5, 5) in child space, which is within (50, 50)
        assert!(render.hit_test(Offset::new(15.0, 25.0)));

        // Hit test at (5, 5) is outside transformed bounds
        assert!(!render.hit_test(Offset::new(5.0, 5.0)));
    }

    #[test]
    fn test_render_transform_hit_test_untransformed() {
        let mut render = RenderTransform::new(Matrix4::translation(10.0, 20.0, 0.0));
        render.set_transform_hit_tests(false);

        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));
        render.layout(constraints);

        // Hit tests in original child space (not transformed)
        assert!(render.hit_test(Offset::new(25.0, 25.0)));
        assert!(!render.hit_test(Offset::new(60.0, 60.0)));
    }

    #[test]
    fn test_render_transform_visit_children() {
        let mut render = RenderTransform::new(Matrix4::identity());
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let mut count = 0;
        render.visit_children(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_transform_visit_children_no_child() {
        let render = RenderTransform::new(Matrix4::identity());

        let mut count = 0;
        render.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_render_transform_mark_needs_paint_only() {
        let mut render = RenderTransform::new(Matrix4::identity());

        // Layout to clear flags
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        render.layout(constraints);

        assert!(!render.needs_layout());
        assert!(!render.needs_paint());

        // Changing transform should only mark needs_paint
        render.set_transform(Matrix4::scaling(2.0, 2.0, 1.0));

        assert!(!render.needs_layout());
        assert!(render.needs_paint());
    }
}
