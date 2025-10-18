//! RenderLimitedBox - limits the size of its child when constraints are unbounded.
//!
//! This render object is useful when placing a child in an unbounded environment,
//! such as a scrollable list. Without LimitedBox, the child would try to expand
//! to infinity and cause a layout error.
//!
//! # Layout Algorithm
//!
//! 1. Check each dimension of parent constraints
//! 2. If max_width is infinite → limit to maxWidth
//! 3. If max_height is infinite → limit to maxHeight
//! 4. If constraint is bounded → pass through unchanged
//! 5. Layout child with modified constraints

use crate::{BoxConstraints, Offset, RenderObject, Size};

/// Limits the size of its child when constraints are unbounded.
///
/// When the parent provides unbounded constraints (infinite max width/height),
/// this render object limits them to the specified maxWidth and maxHeight.
///
/// When constraints are already bounded, this render object passes them through
/// unchanged, making it invisible in that case.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderLimitedBox;
///
/// // Limit unbounded constraints to 100x100
/// let mut limited_box = RenderLimitedBox::new(100.0, 100.0);
/// ```
///
/// # Use Cases
///
/// - Placing children in scrollable lists (ListView)
/// - Handling unbounded constraints in Row/Column
/// - Preventing infinite size errors
#[derive(Debug)]
pub struct RenderLimitedBox {
    /// Maximum width when parent provides unbounded width
    max_width: f32,

    /// Maximum height when parent provides unbounded height
    max_height: f32,

    /// The single child
    child: Option<Box<dyn RenderObject>>,

    /// Current size after layout
    size: Size,

    /// Current constraints
    constraints: Option<BoxConstraints>,

    /// Layout dirty flag
    needs_layout_flag: bool,

    /// Paint dirty flag
    needs_paint_flag: bool,
}

impl RenderLimitedBox {
    /// Create a new RenderLimitedBox with the given max dimensions
    ///
    /// # Arguments
    ///
    /// * `max_width` - Maximum width when parent width is unbounded (default: 0.0)
    /// * `max_height` - Maximum height when parent height is unbounded (default: 0.0)
    ///
    /// # Panics
    ///
    /// Panics if max_width or max_height are negative or NaN
    pub fn new(max_width: f32, max_height: f32) -> Self {
        assert!(
            max_width >= 0.0 && max_width.is_finite(),
            "max_width must be non-negative and finite, got {}",
            max_width
        );
        assert!(
            max_height >= 0.0 && max_height.is_finite(),
            "max_height must be non-negative and finite, got {}",
            max_height
        );

        Self {
            max_width,
            max_height,
            child: None,
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Get the max width
    pub fn max_width(&self) -> f32 {
        self.max_width
    }

    /// Get the max height
    pub fn max_height(&self) -> f32 {
        self.max_height
    }

    /// Set the max width
    ///
    /// # Panics
    ///
    /// Panics if max_width is negative or NaN
    pub fn set_max_width(&mut self, max_width: f32) {
        assert!(
            max_width >= 0.0 && max_width.is_finite(),
            "max_width must be non-negative and finite, got {}",
            max_width
        );

        if (self.max_width - max_width).abs() > f32::EPSILON {
            self.max_width = max_width;
            self.mark_needs_layout();
        }
    }

    /// Set the max height
    ///
    /// # Panics
    ///
    /// Panics if max_height is negative or NaN
    pub fn set_max_height(&mut self, max_height: f32) {
        assert!(
            max_height >= 0.0 && max_height.is_finite(),
            "max_height must be non-negative and finite, got {}",
            max_height
        );

        if (self.max_height - max_height).abs() > f32::EPSILON {
            self.max_height = max_height;
            self.mark_needs_layout();
        }
    }

    /// Set the child
    pub fn set_child(&mut self, child: Box<dyn RenderObject>) {
        self.child = Some(child);
        self.mark_needs_layout();
    }

    /// Get a reference to the child
    pub fn child(&self) -> Option<&dyn RenderObject> {
        self.child.as_deref()
    }

    /// Remove the child
    pub fn remove_child(&mut self) -> Option<Box<dyn RenderObject>> {
        let child = self.child.take();
        if child.is_some() {
            self.mark_needs_layout();
        }
        child
    }

    /// Perform layout on this render object
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // If no child, use smallest size
        if self.child.is_none() {
            self.size = constraints.smallest();
            return self.size;
        }

        // Compute child constraints with limited unbounded dimensions
        let child_constraints = self.limit_constraints(constraints);

        // Layout child
        if let Some(child) = &mut self.child {
            let child_size = child.layout(child_constraints);
            self.size = constraints.constrain(child_size);
        } else {
            self.size = constraints.smallest();
        }

        self.size
    }

    /// Limit unbounded constraints to max dimensions
    fn limit_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        BoxConstraints::new(
            constraints.min_width,
            if constraints.max_width.is_infinite() {
                self.max_width
            } else {
                constraints.max_width
            },
            constraints.min_height,
            if constraints.max_height.is_infinite() {
                self.max_height
            } else {
                constraints.max_height
            },
        )
    }
}

impl RenderObject for RenderLimitedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.constraints = Some(constraints);
        self.needs_layout_flag = false;
        self.perform_layout(constraints)
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

    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout_flag = true;
        self.needs_paint_flag = true;
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
    fn test_render_limited_box_new() {
        let limited_box = RenderLimitedBox::new(100.0, 200.0);
        assert_eq!(limited_box.max_width(), 100.0);
        assert_eq!(limited_box.max_height(), 200.0);
        assert!(limited_box.needs_layout());
    }

    #[test]
    #[should_panic(expected = "max_width must be non-negative and finite")]
    fn test_render_limited_box_new_invalid_width() {
        RenderLimitedBox::new(-1.0, 100.0);
    }

    #[test]
    #[should_panic(expected = "max_height must be non-negative and finite")]
    fn test_render_limited_box_new_invalid_height() {
        RenderLimitedBox::new(100.0, -1.0);
    }

    #[test]
    fn test_render_limited_box_no_child() {
        let mut limited_box = RenderLimitedBox::new(100.0, 100.0);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = limited_box.layout(constraints);

        // No child - should use smallest size
        assert_eq!(size, Size::zero());
    }

    #[test]
    fn test_render_limited_box_unbounded_width() {
        let mut limited_box = RenderLimitedBox::new(100.0, 200.0);

        // RenderBox expands to fill max constraints by default
        let child = RenderBox::new();
        limited_box.set_child(Box::new(child));

        // Unbounded width, bounded height
        let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, 150.0);
        let size = limited_box.layout(constraints);

        // Child constraints are limited to (0..100, 0..150)
        // RenderBox expands to biggest = 100x150
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 150.0);
    }

    #[test]
    fn test_render_limited_box_unbounded_height() {
        let mut limited_box = RenderLimitedBox::new(100.0, 200.0);
        limited_box.set_child(Box::new(RenderBox::new()));

        // Bounded width, unbounded height
        let constraints = BoxConstraints::new(0.0, 150.0, 0.0, f32::INFINITY);
        let size = limited_box.layout(constraints);

        // Child constraints are limited to (0..150, 0..200)
        // RenderBox expands to biggest = 150x200
        assert_eq!(size.width, 150.0);
        assert_eq!(size.height, 200.0);
    }

    #[test]
    fn test_render_limited_box_both_unbounded() {
        let mut limited_box = RenderLimitedBox::new(100.0, 200.0);
        limited_box.set_child(Box::new(RenderBox::new()));

        // Both unbounded
        let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
        let size = limited_box.layout(constraints);

        // Child constraints are limited to (0..100, 0..200)
        // RenderBox expands to biggest = 100x200
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 200.0);
    }

    #[test]
    fn test_render_limited_box_bounded_constraints() {
        let mut limited_box = RenderLimitedBox::new(100.0, 200.0);
        limited_box.set_child(Box::new(RenderBox::new()));

        // Both bounded - should pass through unchanged
        let constraints = BoxConstraints::tight(Size::new(50.0, 75.0));
        let size = limited_box.layout(constraints);

        // Tight constraints - child must match
        assert_eq!(size.width, 50.0);
        assert_eq!(size.height, 75.0);
    }

    #[test]
    fn test_render_limited_box_set_max_width() {
        let mut limited_box = RenderLimitedBox::new(100.0, 200.0);
        assert_eq!(limited_box.max_width(), 100.0);

        limited_box.set_max_width(150.0);
        assert_eq!(limited_box.max_width(), 150.0);
        assert!(limited_box.needs_layout());
    }

    #[test]
    fn test_render_limited_box_set_max_height() {
        let mut limited_box = RenderLimitedBox::new(100.0, 200.0);
        assert_eq!(limited_box.max_height(), 200.0);

        limited_box.set_max_height(250.0);
        assert_eq!(limited_box.max_height(), 250.0);
        assert!(limited_box.needs_layout());
    }

    #[test]
    fn test_render_limited_box_remove_child() {
        let mut limited_box = RenderLimitedBox::new(100.0, 200.0);
        limited_box.set_child(Box::new(RenderBox::new()));

        assert!(limited_box.child().is_some());

        let removed = limited_box.remove_child();
        assert!(removed.is_some());
        assert!(limited_box.child().is_none());
        assert!(limited_box.needs_layout());
    }

    #[test]
    fn test_render_limited_box_visit_children() {
        let mut limited_box = RenderLimitedBox::new(100.0, 200.0);
        limited_box.set_child(Box::new(RenderBox::new()));

        let mut count = 0;
        limited_box.visit_children(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_limited_box_visit_children_no_child() {
        let limited_box = RenderLimitedBox::new(100.0, 200.0);

        let mut count = 0;
        limited_box.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }
}
