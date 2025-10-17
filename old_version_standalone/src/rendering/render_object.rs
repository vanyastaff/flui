//! RenderObject - the rendering layer
//!
//! RenderObjects perform layout and painting. This is the third tree in
//! Flutter's three-tree architecture: Widget → Element → RenderObject

use std::any::Any;
use std::fmt;
use crate::types::core::{Size, Offset};
use super::BoxConstraints;

/// RenderObject - handles layout and painting
///
/// Similar to Flutter's RenderObject. These are created by RenderObjectWidgets
/// and handle the actual layout computation and painting.
///
/// # Layout Protocol
///
/// 1. Parent sets constraints on child
/// 2. Child chooses size within constraints
/// 3. Parent positions child (sets offset)
/// 4. Parent returns its own size
///
/// # Painting Protocol
///
/// 1. Paint yourself
/// 2. Paint children in order
/// 3. Children are painted at their offsets
pub trait RenderObject: Any + fmt::Debug {
    /// Perform layout with given constraints
    ///
    /// Returns the size this render object chose within the constraints.
    /// Must satisfy: `constraints.is_satisfied_by(returned_size)`
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint this render object
    ///
    /// The painter is positioned at the render object's offset.
    /// Offset is relative to parent's coordinate space.
    fn paint(&self, painter: &egui::Painter, offset: Offset);

    /// Get the current size (after layout)
    fn size(&self) -> Size;

    /// Get the constraints used in last layout
    fn constraints(&self) -> Option<BoxConstraints> {
        None
    }

    /// Check if this render object needs layout
    fn needs_layout(&self) -> bool {
        false
    }

    /// Mark this render object as needing layout
    fn mark_needs_layout(&mut self);

    /// Check if this render object needs paint
    fn needs_paint(&self) -> bool {
        false
    }

    /// Mark this render object as needing paint
    fn mark_needs_paint(&mut self);

    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Convert to Any for mutable downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Get intrinsic width for given height
    ///
    /// Intrinsics help determine natural sizes before layout.
    fn get_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Get max intrinsic width
    fn get_max_intrinsic_width(&self, _height: f32) -> f32 {
        f32::INFINITY
    }

    /// Get intrinsic height for given width
    fn get_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Get max intrinsic height
    fn get_max_intrinsic_height(&self, _width: f32) -> f32 {
        f32::INFINITY
    }

    /// Hit test - check if point is within this render object
    ///
    /// Used for mouse/touch event handling.
    fn hit_test(&self, position: Offset) -> bool {
        let size = self.size();
        position.dx >= 0.0
            && position.dx < size.width
            && position.dy >= 0.0
            && position.dy < size.height
    }
}

/// RenderBox - base implementation for box protocol
///
/// Most common render object type. Uses BoxConstraints for layout.
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
            size: Size::ZERO,
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Helper for computing size based on constraints and child size
    pub fn compute_size_from_child(
        &self,
        constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
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
    pub fn set_child(&mut self, child: Box<dyn RenderObject>) {
        self.child = Some(child);
        self.base.mark_needs_layout();
    }

    /// Get child reference
    pub fn child(&self) -> Option<&dyn RenderObject> {
        self.child.as_ref().map(|c| c.as_ref())
    }

    /// Get mutable child reference
    pub fn child_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.child.as_mut().map(|c| c.as_mut())
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_box_creation() {
        let render_box = RenderBox::new();
        assert_eq!(render_box.size(), Size::ZERO);
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
        render_box.layout(BoxConstraints::unbounded());

        assert!(!render_box.needs_layout());

        render_box.mark_needs_layout();
        assert!(render_box.needs_layout());
    }

    #[test]
    fn test_render_proxy_box() {
        let mut proxy = RenderProxyBox::new();
        assert!(proxy.child().is_none());

        // Add child
        let child = Box::new(RenderBox::new());
        proxy.set_child(child);

        assert!(proxy.child().is_some());
        assert!(proxy.needs_layout());
    }

    #[test]
    fn test_render_proxy_box_layout() {
        let mut proxy = RenderProxyBox::new();

        // Layout without child - should use smallest
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let size = proxy.layout(constraints);

        assert_eq!(size, Size::new(50.0, 30.0)); // smallest

        // Add child and layout again
        let mut child = Box::new(RenderBox::new());
        proxy.set_child(child);

        let size2 = proxy.layout(constraints);
        assert_eq!(size2, Size::new(150.0, 100.0)); // child uses biggest
    }

    #[test]
    fn test_hit_test() {
        let mut render_box = RenderBox::new();
        render_box.layout(BoxConstraints::tight(Size::new(100.0, 50.0)));

        // Inside
        assert!(render_box.hit_test(Offset::new(50.0, 25.0)));

        // Outside
        assert!(!render_box.hit_test(Offset::new(150.0, 25.0)));
        assert!(!render_box.hit_test(Offset::new(50.0, 75.0)));
    }

    #[test]
    fn test_intrinsic_sizes() {
        let render_box = RenderBox::new();

        assert_eq!(render_box.get_min_intrinsic_width(100.0), 0.0);
        assert_eq!(render_box.get_max_intrinsic_width(100.0), f32::INFINITY);
        assert_eq!(render_box.get_min_intrinsic_height(100.0), 0.0);
        assert_eq!(render_box.get_max_intrinsic_height(100.0), f32::INFINITY);
    }
}
