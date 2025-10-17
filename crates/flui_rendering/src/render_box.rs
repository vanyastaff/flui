//! RenderBox - base implementation for box protocol
//!
//! Most common render object type. Uses BoxConstraints for layout.

use std::any::Any;

use crate::{BoxConstraints, Offset, RenderObject, Size};

/// RenderBox - base implementation for box protocol
///
/// Most common render object type. Uses BoxConstraints for layout.
/// Provides default implementations for common operations.
///
/// # Example
///
/// ```rust,ignore
/// let mut render_box = RenderBox::new();
/// let size = render_box.layout(BoxConstraints::tight(Size::new(100.0, 50.0)));
/// assert_eq!(size, Size::new(100.0, 50.0));
/// ```
#[derive(Debug)]
pub struct RenderBox {
    /// Current size
    size: Size,

    /// Current constraints
    constraints: Option<BoxConstraints>,

    /// Whether layout is needed
    needs_layout_flag: bool,

    /// Whether paint is needed
    needs_paint_flag: bool,
}

impl RenderBox {
    /// Create a new RenderBox
    pub fn new() -> Self {
        Self {
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Helper for computing size based on constraints and child size
    ///
    /// Constrains the child size to fit within the constraints.
    pub fn compute_size_from_child(&self, constraints: BoxConstraints, child_size: Size) -> Size {
        constraints.constrain(child_size)
    }
}

impl Default for RenderBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.constraints = Some(constraints);
        self.needs_layout_flag = false;

        // Default: use biggest size allowed
        self.size = constraints.biggest();
        self.size
    }

    fn paint(&self, _painter: &egui::Painter, _offset: Offset) {
        // Default: paint nothing
    }

    fn size(&self) -> Size {
        self.size
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout_flag = true;
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint_flag = true;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// RenderProxyBox - passes layout to child
///
/// Base for widgets that have one child and don't modify layout
/// (like Opacity, Transform, etc.)
///
/// # Example
///
/// ```rust,ignore
/// let mut proxy = RenderProxyBox::new();
/// proxy.set_child(Box::new(RenderBox::new()));
///
/// let size = proxy.layout(BoxConstraints::loose(Size::new(200.0, 100.0)));
/// // Child determines the size
/// ```
#[derive(Debug)]
pub struct RenderProxyBox {
    base: RenderBox,
    child: Option<Box<dyn RenderObject>>,
}

impl RenderProxyBox {
    /// Create new proxy box
    pub fn new() -> Self {
        Self {
            base: RenderBox::new(),
            child: None,
        }
    }

    /// Set child
    ///
    /// Replaces the current child and marks as needing layout.
    pub fn set_child(&mut self, child: Box<dyn RenderObject>) {
        self.child = Some(child);
        self.base.mark_needs_layout();
    }

    /// Remove child
    ///
    /// Removes the child and marks as needing layout.
    pub fn remove_child(&mut self) -> Option<Box<dyn RenderObject>> {
        let child = self.child.take();
        if child.is_some() {
            self.base.mark_needs_layout();
        }
        child
    }

    /// Get child reference
    pub fn child(&self) -> Option<&dyn RenderObject> {
        self.child.as_ref().map(|c| c.as_ref())
    }

    /// Get mutable child reference
    pub fn child_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.child.as_mut().map(|c| c.as_mut())
    }

    /// Check if has child
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }
}

impl Default for RenderProxyBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderProxyBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = &mut self.child {
            // Pass constraints to child
            let child_size = child.layout(constraints);
            self.base.size = child_size;
            self.base.constraints = Some(constraints);
            self.base.needs_layout_flag = false;
            child_size
        } else {
            // No child - use smallest size
            self.base.size = constraints.smallest();
            self.base.constraints = Some(constraints);
            self.base.needs_layout_flag = false;
            self.base.size
        }
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            child.paint(painter, offset);
        }
    }

    fn size(&self) -> Size {
        self.base.size
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        self.base.constraints
    }

    fn needs_layout(&self) -> bool {
        self.base.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.base.mark_needs_layout();
    }

    fn needs_paint(&self) -> bool {
        self.base.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.base.mark_needs_paint();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = &self.child {
            visitor(child.as_ref());
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(child.as_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_box_creation() {
        let render_box = RenderBox::new();
        assert_eq!(render_box.size(), Size::zero());
        assert!(render_box.needs_layout());
        assert!(render_box.needs_paint());
    }

    #[test]
    fn test_render_box_layout() {
        let mut render_box = RenderBox::new();
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));

        let size = render_box.layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
        assert_eq!(render_box.size(), Size::new(100.0, 50.0));
        assert!(!render_box.needs_layout());
    }

    #[test]
    fn test_render_box_mark_needs_layout() {
        let mut render_box = RenderBox::new();
        let constraints = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
        render_box.layout(constraints);

        assert!(!render_box.needs_layout());

        render_box.mark_needs_layout();
        assert!(render_box.needs_layout());
    }

    #[test]
    fn test_render_box_compute_size_from_child() {
        let render_box = RenderBox::new();
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let child_size = Size::new(200.0, 150.0); // Too large

        let size = render_box.compute_size_from_child(constraints, child_size);

        assert_eq!(size, Size::new(150.0, 100.0)); // Constrained
    }

    #[test]
    fn test_render_proxy_box_creation() {
        let proxy = RenderProxyBox::new();
        assert!(proxy.child().is_none());
        assert!(!proxy.has_child());
    }

    #[test]
    fn test_render_proxy_box_set_child() {
        let mut proxy = RenderProxyBox::new();

        // Add child
        let child = Box::new(RenderBox::new());
        proxy.set_child(child);

        assert!(proxy.child().is_some());
        assert!(proxy.has_child());
        assert!(proxy.needs_layout());
    }

    #[test]
    fn test_render_proxy_box_remove_child() {
        let mut proxy = RenderProxyBox::new();

        // Add child
        proxy.set_child(Box::new(RenderBox::new()));
        assert!(proxy.has_child());

        // Remove child
        let removed = proxy.remove_child();
        assert!(removed.is_some());
        assert!(!proxy.has_child());
        assert!(proxy.needs_layout());
    }

    #[test]
    fn test_render_proxy_box_layout_without_child() {
        let mut proxy = RenderProxyBox::new();

        // Layout without child - should use smallest
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let size = proxy.layout(constraints);

        assert_eq!(size, Size::new(50.0, 30.0)); // smallest
    }

    #[test]
    fn test_render_proxy_box_layout_with_child() {
        let mut proxy = RenderProxyBox::new();

        // Add child and layout
        proxy.set_child(Box::new(RenderBox::new()));

        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let size = proxy.layout(constraints);

        // Child (RenderBox) uses biggest by default
        assert_eq!(size, Size::new(150.0, 100.0));
    }

    #[test]
    fn test_render_proxy_box_visit_children() {
        let mut proxy = RenderProxyBox::new();
        proxy.set_child(Box::new(RenderBox::new()));

        let mut count = 0;
        proxy.visit_children(&mut |_child| {
            count += 1;
        });

        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_proxy_box_visit_children_mut() {
        let mut proxy = RenderProxyBox::new();
        proxy.set_child(Box::new(RenderBox::new()));

        let mut count = 0;
        proxy.visit_children_mut(&mut |child| {
            child.mark_needs_layout();
            count += 1;
        });

        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_proxy_box_no_children() {
        let mut proxy = RenderProxyBox::new();

        let mut count = 0;
        proxy.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);

        proxy.visit_children_mut(&mut |_| count += 1);
        assert_eq!(count, 0);
    }
}
