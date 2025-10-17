//! RenderObject trait - the rendering layer
//!
//! RenderObjects perform layout and painting. This is the third tree in
//! Flutter's three-tree architecture: Widget → Element → RenderObject

use std::any::Any;
use std::fmt;

use crate::{BoxConstraints, Offset, Size};

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
///
/// # Example
///
/// ```rust,ignore
/// struct MyRenderObject {
///     size: Size,
///     needs_layout: bool,
/// }
///
/// impl RenderObject for MyRenderObject {
///     fn layout(&mut self, constraints: BoxConstraints) -> Size {
///         self.size = constraints.biggest();
///         self.needs_layout = false;
///         self.size
///     }
///
///     fn paint(&self, painter: &egui::Painter, offset: Offset) {
///         // Paint at offset position
///         let rect = egui::Rect::from_min_size(
///             offset.to_pos2(),
///             egui::vec2(self.size.width, self.size.height),
///         );
///         painter.rect_filled(rect, 0.0, egui::Color32::BLUE);
///     }
///
///     fn size(&self) -> Size {
///         self.size
///     }
///
///     fn mark_needs_layout(&mut self) {
///         self.needs_layout = true;
///     }
///
///     // ... other methods
/// }
/// ```
pub trait RenderObject: Any + fmt::Debug + Send + Sync {
    /// Perform layout with given constraints
    ///
    /// Returns the size this render object chose within the constraints.
    /// Must satisfy: `constraints.is_satisfied_by(returned_size)`
    ///
    /// # Layout Rules
    ///
    /// - Child must respect parent's constraints
    /// - Child cannot read its own size during layout (causes cycles)
    /// - Parent sets child's offset AFTER child's layout returns
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint this render object
    ///
    /// The painter is positioned at the render object's offset.
    /// Offset is relative to parent's coordinate space.
    ///
    /// # Painting Rules
    ///
    /// - Paint yourself first (background)
    /// - Then paint children in order
    /// - Children are clipped to parent bounds (optional)
    fn paint(&self, painter: &egui::Painter, offset: Offset);

    /// Get the current size (after layout)
    ///
    /// Returns the size chosen during the most recent layout pass.
    fn size(&self) -> Size;

    /// Get the constraints used in last layout
    ///
    /// Useful for debugging and introspection.
    fn constraints(&self) -> Option<BoxConstraints> {
        None
    }

    /// Check if this render object needs layout
    ///
    /// Returns true if layout() needs to be called.
    fn needs_layout(&self) -> bool {
        false
    }

    /// Mark this render object as needing layout
    ///
    /// Called when configuration changes or parent requests relayout.
    fn mark_needs_layout(&mut self);

    /// Check if this render object needs paint
    ///
    /// Returns true if paint() needs to be called.
    fn needs_paint(&self) -> bool {
        false
    }

    /// Mark this render object as needing paint
    ///
    /// Called when appearance changes or parent requests repaint.
    fn mark_needs_paint(&mut self);

    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Convert to Any for mutable downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // Intrinsic sizing methods
    //
    // These help determine natural sizes before layout.
    // Used by widgets like IntrinsicWidth/IntrinsicHeight.

    /// Get minimum intrinsic width for given height
    ///
    /// Returns the smallest width this render object can have while
    /// maintaining its proportions if given this height.
    fn get_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Get maximum intrinsic width for given height
    ///
    /// Returns the largest width this render object would need
    /// if given this height.
    fn get_max_intrinsic_width(&self, _height: f32) -> f32 {
        f32::INFINITY
    }

    /// Get minimum intrinsic height for given width
    ///
    /// Returns the smallest height this render object can have while
    /// maintaining its proportions if given this width.
    fn get_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Get maximum intrinsic height for given width
    ///
    /// Returns the largest height this render object would need
    /// if given this width.
    fn get_max_intrinsic_height(&self, _width: f32) -> f32 {
        f32::INFINITY
    }

    /// Hit test - check if point is within this render object
    ///
    /// Used for mouse/touch event handling. Position is relative to
    /// this render object's coordinate space.
    ///
    /// Default implementation checks if point is within bounds.
    fn hit_test(&self, position: Offset) -> bool {
        let size = self.size();
        position.dx >= 0.0
            && position.dx < size.width
            && position.dy >= 0.0
            && position.dy < size.height
    }

    /// Visit all children (read-only)
    ///
    /// Default: no children (leaf render object)
    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn RenderObject)) {
        // Default: no children
    }

    /// Visit all children (mutable)
    ///
    /// Default: no children (leaf render object)
    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        // Default: no children
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test render object implementation
    #[derive(Debug)]
    struct TestRenderObject {
        size: Size,
        constraints: Option<BoxConstraints>,
        needs_layout_flag: bool,
        needs_paint_flag: bool,
    }

    impl TestRenderObject {
        fn new() -> Self {
            Self {
                size: Size::zero(),
                constraints: None,
                needs_layout_flag: true,
                needs_paint_flag: true,
            }
        }
    }

    impl RenderObject for TestRenderObject {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.constraints = Some(constraints);
            self.needs_layout_flag = false;
            self.size = constraints.biggest();
            self.size
        }

        fn paint(&self, _painter: &egui::Painter, _offset: Offset) {
            // Test implementation - don't paint
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

    #[test]
    fn test_render_object_creation() {
        let render_obj = TestRenderObject::new();
        assert_eq!(render_obj.size(), Size::zero());
        assert!(render_obj.needs_layout());
        assert!(render_obj.needs_paint());
    }

    #[test]
    fn test_render_object_layout() {
        let mut render_obj = TestRenderObject::new();
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));

        let size = render_obj.layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
        assert_eq!(render_obj.size(), Size::new(100.0, 50.0));
        assert!(!render_obj.needs_layout());
        assert_eq!(render_obj.constraints(), Some(constraints));
    }

    #[test]
    fn test_render_object_mark_dirty() {
        let mut render_obj = TestRenderObject::new();
        render_obj.layout(BoxConstraints::tight(Size::new(100.0, 50.0)));

        assert!(!render_obj.needs_layout());

        render_obj.mark_needs_layout();
        assert!(render_obj.needs_layout());

        render_obj.mark_needs_paint();
        assert!(render_obj.needs_paint());
    }

    #[test]
    fn test_hit_test() {
        let mut render_obj = TestRenderObject::new();
        render_obj.layout(BoxConstraints::tight(Size::new(100.0, 50.0)));

        // Inside
        assert!(render_obj.hit_test(Offset::new(50.0, 25.0)));
        assert!(render_obj.hit_test(Offset::new(0.0, 0.0)));
        assert!(render_obj.hit_test(Offset::new(99.9, 49.9)));

        // Outside
        assert!(!render_obj.hit_test(Offset::new(100.0, 25.0)));
        assert!(!render_obj.hit_test(Offset::new(50.0, 50.0)));
        assert!(!render_obj.hit_test(Offset::new(-1.0, 25.0)));
    }

    #[test]
    fn test_intrinsic_sizes() {
        let render_obj = TestRenderObject::new();

        assert_eq!(render_obj.get_min_intrinsic_width(100.0), 0.0);
        assert_eq!(render_obj.get_max_intrinsic_width(100.0), f32::INFINITY);
        assert_eq!(render_obj.get_min_intrinsic_height(100.0), 0.0);
        assert_eq!(render_obj.get_max_intrinsic_height(100.0), f32::INFINITY);
    }

    #[test]
    fn test_downcast() {
        let mut render_obj = TestRenderObject::new();

        // Test downcast
        let any = render_obj.as_any();
        assert!(any.downcast_ref::<TestRenderObject>().is_some());

        let any_mut = render_obj.as_any_mut();
        assert!(any_mut.downcast_mut::<TestRenderObject>().is_some());
    }
}
