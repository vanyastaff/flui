//! RenderAspectRatio - attempts to size the child to a specific aspect ratio.
//!
//! This render object tries to size its child to match a specific aspect ratio
//! (width / height). It's commonly used for images, videos, and other media
//! where maintaining proportions is important.
//!
//! # Layout Algorithm
//!
//! 1. If no child: use smallest size within constraints
//! 2. Determine constrained dimension:
//!    - If width is constrained (finite maxWidth): height = width / aspectRatio
//!    - If height is constrained (finite maxHeight): width = height * aspectRatio
//!    - If both constrained: choose smaller size that fits
//!    - If neither constrained: error (unbounded constraints)
//! 3. Constrain result to parent constraints
//! 4. Layout child with tight constraints of result

use crate::{BoxConstraints, Offset, Size};
use flui_core::{DynRenderObject, ElementId};
use crate::RenderFlags;

/// Attempts to size the child to a specific aspect ratio.
///
/// The aspect ratio is the ratio of width to height. For example:
/// - 16/9 = 1.777... (widescreen)
/// - 4/3 = 1.333... (classic TV)
/// - 1/1 = 1.0 (square)
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderAspectRatio;
///
/// // 16:9 aspect ratio (widescreen)
/// let mut aspect_ratio = RenderAspectRatio::new(16.0 / 9.0);
/// ```
#[derive(Debug)]
pub struct RenderAspectRatio {
    /// Element ID for caching
    element_id: Option<ElementId>,

    /// The aspect ratio to attempt to use (width / height)
    aspect_ratio: f32,

    /// The single child
    child: Option<Box<dyn DynRenderObject>>,

    /// Current size after layout
    size: Size,

    /// Current constraints
    constraints: Option<BoxConstraints>,

    /// Render flags (needs_layout, needs_paint, boundaries)
    flags: RenderFlags,
}

impl RenderAspectRatio {
    /// Create a new RenderAspectRatio with the given aspect ratio
    ///
    /// # Panics
    ///
    /// Panics if aspect_ratio is not positive and finite
    pub fn new(aspect_ratio: f32) -> Self {
        assert!(
            aspect_ratio.is_finite() && aspect_ratio > 0.0,
            "aspect_ratio must be positive and finite, got {}",
            aspect_ratio
        );

        Self {
            element_id: None,
            aspect_ratio,
            child: None,
            size: Size::zero(),
            constraints: None,
            flags: RenderFlags::new(),
        }
    }

    /// Create RenderAspectRatio with element ID for caching
    pub fn with_element_id(aspect_ratio: f32, element_id: ElementId) -> Self {
        assert!(
            aspect_ratio.is_finite() && aspect_ratio > 0.0,
            "aspect_ratio must be positive and finite, got {}",
            aspect_ratio
        );

        Self {
            element_id: Some(element_id),
            aspect_ratio,
            child: None,
            size: Size::zero(),
            constraints: None,
            flags: RenderFlags::new(),
        }
    }

    /// Sets element ID for caching
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }

    /// Gets element ID
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Get the aspect ratio
    pub fn aspect_ratio(&self) -> f32 {
        self.aspect_ratio
    }

    /// Set the aspect ratio
    ///
    /// # Panics
    ///
    /// Panics if aspect_ratio is not positive and finite
    pub fn set_aspect_ratio(&mut self, aspect_ratio: f32) {
        assert!(
            aspect_ratio.is_finite() && aspect_ratio > 0.0,
            "aspect_ratio must be positive and finite, got {}",
            aspect_ratio
        );

        if (self.aspect_ratio - aspect_ratio).abs() > f32::EPSILON {
            self.aspect_ratio = aspect_ratio;
            self.mark_needs_layout();
        }
    }

    /// Set the child
    pub fn set_child(&mut self, child: Box<dyn DynRenderObject>) {
        self.child = Some(child);
        self.mark_needs_layout();
    }

    /// Get a reference to the child
    pub fn child(&self) -> Option<&dyn DynRenderObject> {
        self.child.as_deref()
    }

    /// Remove the child
    pub fn remove_child(&mut self) -> Option<Box<dyn DynRenderObject>> {
        let child = self.child.take();
        if child.is_some() {
            self.mark_needs_layout();
        }
        child
    }

    /// Compute the size that matches the aspect ratio within constraints
    fn compute_size_for_aspect_ratio(&self, constraints: BoxConstraints) -> Size {
        // If constraints are tight, we must use that exact size
        if constraints.is_tight() {
            return constraints.smallest();
        }

        // Check if constraints are bounded
        let has_bounded_width = constraints.max_width.is_finite();
        let has_bounded_height = constraints.max_height.is_finite();

        // At least one dimension must be bounded
        assert!(
            has_bounded_width || has_bounded_height,
            "RenderAspectRatio requires at least one bounded constraint. Got: {:?}",
            constraints
        );

        let mut size = if has_bounded_width && has_bounded_height {
            // Both dimensions bounded - choose smaller size
            self.compute_size_both_bounded(constraints)
        } else if has_bounded_width {
            // Width bounded, compute height
            let width = constraints.max_width;
            let height = width / self.aspect_ratio;
            Size::new(width, height)
        } else {
            // Height bounded, compute width
            let height = constraints.max_height;
            let width = height * self.aspect_ratio;
            Size::new(width, height)
        };

        // Constrain to bounds
        size = constraints.constrain(size);
        size
    }

    /// Compute size when both width and height are bounded
    fn compute_size_both_bounded(&self, constraints: BoxConstraints) -> Size {
        // Try width-based sizing
        let width = constraints.max_width;
        let height_from_width = width / self.aspect_ratio;

        if height_from_width <= constraints.max_height {
            // Width-based size fits
            Size::new(width, height_from_width)
        } else {
            // Use height-based sizing
            let height = constraints.max_height;
            let width_from_height = height * self.aspect_ratio;
            Size::new(width_from_height, height)
        }
    }
}

impl DynRenderObject for RenderAspectRatio {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        crate::impl_cached_layout!(self, constraints, {
            // If no child, use smallest size
            if self.child.is_none() {
                return constraints.smallest();
            }

            // Calculate size based on aspect ratio and constraints
            let size = self.compute_size_for_aspect_ratio(constraints);

            // Layout child with tight constraints
            if let Some(child) = &mut self.child {
                let _ = child.layout(BoxConstraints::tight(size));
            }

            size
        })
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child at offset
        if let Some(child) = &self.child {
            child.paint(painter, offset);
        }
    }

    fn size(&self) -> Size {
        self.size
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }

    fn needs_layout(&self) -> bool {
        self.flags.needs_layout()
    }

    fn mark_needs_layout(&mut self) {
        self.flags.mark_needs_layout();
    }

    fn needs_paint(&self) -> bool {
        self.flags.needs_paint()
    }

    fn mark_needs_paint(&mut self) {
        self.flags.mark_needs_paint();
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RenderBox;

    #[test]
    fn test_render_aspect_ratio_new() {
        let aspect_ratio = RenderAspectRatio::new(16.0 / 9.0);
        assert!((aspect_ratio.aspect_ratio() - 16.0 / 9.0).abs() < f32::EPSILON);
        assert!(aspect_ratio.needs_layout());
    }

    #[test]
    #[should_panic(expected = "aspect_ratio must be positive and finite")]
    fn test_render_aspect_ratio_new_invalid_zero() {
        RenderAspectRatio::new(0.0);
    }

    #[test]
    #[should_panic(expected = "aspect_ratio must be positive and finite")]
    fn test_render_aspect_ratio_new_invalid_negative() {
        RenderAspectRatio::new(-1.0);
    }

    #[test]
    #[should_panic(expected = "aspect_ratio must be positive and finite")]
    fn test_render_aspect_ratio_new_invalid_infinity() {
        RenderAspectRatio::new(f32::INFINITY);
    }

    #[test]
    fn test_render_aspect_ratio_no_child() {
        let mut aspect_ratio = RenderAspectRatio::new(1.0);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = aspect_ratio.layout(constraints);

        // No child - should use smallest size
        assert_eq!(size, Size::zero());
    }

    #[test]
    fn test_render_aspect_ratio_width_bounded() {
        let mut aspect_ratio = RenderAspectRatio::new(16.0 / 9.0);
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        // Width bounded to 160, height unbounded
        let constraints = BoxConstraints::new(0.0, 160.0, 0.0, f32::INFINITY);
        let size = aspect_ratio.layout(constraints);

        // height = width / aspectRatio = 160 / (16/9) = 90
        assert_eq!(size.width, 160.0);
        assert!((size.height - 90.0).abs() < 0.01);
    }

    #[test]
    fn test_render_aspect_ratio_height_bounded() {
        let mut aspect_ratio = RenderAspectRatio::new(16.0 / 9.0);
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        // Height bounded to 90, width unbounded
        let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, 90.0);
        let size = aspect_ratio.layout(constraints);

        // width = height * aspectRatio = 90 * (16/9) = 160
        assert!((size.width - 160.0).abs() < 0.01);
        assert_eq!(size.height, 90.0);
    }

    #[test]
    fn test_render_aspect_ratio_both_bounded_tight_constraints() {
        let mut aspect_ratio = RenderAspectRatio::new(16.0 / 9.0);
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        // Tight constraints: must use exact size regardless of aspect ratio
        let constraints = BoxConstraints::tight(Size::new(160.0, 200.0));
        let size = aspect_ratio.layout(constraints);

        // Tight constraints - must return exact size
        assert_eq!(size.width, 160.0);
        assert_eq!(size.height, 200.0);
    }

    #[test]
    fn test_render_aspect_ratio_both_bounded_loose_width_limiting() {
        let mut aspect_ratio = RenderAspectRatio::new(16.0 / 9.0);
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        // Loose constraints: 0-160 width, 0-200 height
        let constraints = BoxConstraints::new(0.0, 160.0, 0.0, 200.0);
        let size = aspect_ratio.layout(constraints);

        // Width-based: height = 160 / (16/9) = 90 <= 200 ✓
        assert_eq!(size.width, 160.0);
        assert!((size.height - 90.0).abs() < 0.01);
    }

    #[test]
    fn test_render_aspect_ratio_both_bounded_loose_height_limiting() {
        let mut aspect_ratio = RenderAspectRatio::new(16.0 / 9.0);
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        // Loose constraints: 0-200 width, 0-90 height
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 90.0);
        let size = aspect_ratio.layout(constraints);

        // Width-based: height = 200 / (16/9) = 112.5 > 90 ✗
        // Height-based: width = 90 * (16/9) = 160 ✓
        assert!((size.width - 160.0).abs() < 0.01);
        assert_eq!(size.height, 90.0);
    }

    #[test]
    fn test_render_aspect_ratio_square() {
        let mut aspect_ratio = RenderAspectRatio::new(1.0);
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, f32::INFINITY);
        let size = aspect_ratio.layout(constraints);

        // Square: width = height
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 100.0);
    }

    #[test]
    fn test_render_aspect_ratio_portrait() {
        let mut aspect_ratio = RenderAspectRatio::new(3.0 / 4.0); // Portrait (0.75)
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::new(0.0, 90.0, 0.0, f32::INFINITY);
        let size = aspect_ratio.layout(constraints);

        // height = width / aspectRatio = 90 / 0.75 = 120
        assert_eq!(size.width, 90.0);
        assert_eq!(size.height, 120.0);
    }

    #[test]
    fn test_render_aspect_ratio_set_aspect_ratio() {
        let mut aspect_ratio = RenderAspectRatio::new(1.0);
        assert!((aspect_ratio.aspect_ratio() - 1.0).abs() < f32::EPSILON);

        aspect_ratio.set_aspect_ratio(16.0 / 9.0);
        assert!((aspect_ratio.aspect_ratio() - 16.0 / 9.0).abs() < f32::EPSILON);
        assert!(aspect_ratio.needs_layout());
    }

    #[test]
    fn test_render_aspect_ratio_remove_child() {
        let mut aspect_ratio = RenderAspectRatio::new(1.0);
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        assert!(aspect_ratio.child().is_some());

        let removed = aspect_ratio.remove_child();
        assert!(removed.is_some());
        assert!(aspect_ratio.child().is_none());
        assert!(aspect_ratio.needs_layout());
    }

    #[test]
    fn test_render_aspect_ratio_visit_children() {
        let mut aspect_ratio = RenderAspectRatio::new(1.0);
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        let mut count = 0;
        aspect_ratio.visit_children(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_aspect_ratio_visit_children_no_child() {
        let aspect_ratio = RenderAspectRatio::new(1.0);

        let mut count = 0;
        aspect_ratio.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    #[should_panic(expected = "requires at least one bounded constraint")]
    fn test_render_aspect_ratio_unbounded_constraints() {
        let mut aspect_ratio = RenderAspectRatio::new(1.0);
        aspect_ratio.set_child(Box::new(RenderBox::new()));

        // Both width and height unbounded - should panic
        let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
        aspect_ratio.layout(constraints);
    }
}
