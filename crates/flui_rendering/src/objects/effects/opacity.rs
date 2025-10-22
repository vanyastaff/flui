//! RenderOpacity - applies opacity (alpha) to child rendering
//!
//! Used by Opacity widget to create fade effects and transparency.

use flui_core::{BoxConstraints, DynRenderObject};
use flui_types::{Offset, Size};

/// RenderOpacity applies opacity to child rendering
///
/// # Parameters
///
/// - `opacity`: Alpha value from 0.0 (fully transparent) to 1.0 (fully opaque)
///
/// # Layout Algorithm
///
/// Simply passes constraints to child and adopts child size.
///
/// # Paint Algorithm
///
/// In egui, opacity is typically handled by modifying alpha channel of colors.
/// Since we don't control child painting directly, this is a placeholder
/// implementation that demonstrates the pattern. In a full implementation,
/// this would need layer compositing or painter state management.
///
/// For now, this serves as the structural foundation for Opacity widget.
///
/// # Examples
///
/// ```rust
/// # use flui_rendering::RenderOpacity;
/// // Fully opaque (no transparency)
/// let mut render = RenderOpacity::new(1.0);
///
/// // Semi-transparent
/// let mut render = RenderOpacity::new(0.5);
///
/// // Fully transparent
/// let mut render = RenderOpacity::new(0.0);
/// ```
#[derive(Debug)]
pub struct RenderOpacity {
    /// Element ID for cache invalidation
    element_id: Option<flui_core::ElementId>,
    /// Opacity value (0.0 = transparent, 1.0 = opaque)
    opacity: f32,
    /// Child render object
    child: Option<Box<dyn DynRenderObject>>,
    /// Current size
    size: Size,
    /// Current constraints
    constraints: Option<BoxConstraints>,
    /// Layout dirty flag
    needs_layout_flag: bool,
    /// Paint dirty flag
    needs_paint_flag: bool,
}

impl RenderOpacity {
    /// Creates a new RenderOpacity
    ///
    /// # Parameters
    ///
    /// - `opacity`: Alpha value (0.0 to 1.0)
    ///
    /// # Panics
    ///
    /// Panics if opacity is not in range [0.0, 1.0]
    pub fn new(opacity: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&opacity),
            "opacity must be between 0.0 and 1.0, got {}",
            opacity
        );

        Self {
            element_id: None,
            opacity,
            child: None,
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Create RenderOpacity with element ID for caching
    pub fn with_element_id(element_id: flui_core::ElementId, opacity: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&opacity),
            "opacity must be between 0.0 and 1.0, got {}",
            opacity
        );

        Self {
            element_id: Some(element_id),
            opacity,
            child: None,
            size: Size::zero(),
            constraints: None,
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Get the element ID
    pub fn element_id(&self) -> Option<flui_core::ElementId> {
        self.element_id
    }

    /// Set the element ID for caching
    pub fn set_element_id(&mut self, element_id: Option<flui_core::ElementId>) {
        self.element_id = element_id;
    }

    /// Sets the child
    pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
        self.child = child;
        self.mark_needs_layout();
    }

    /// Returns a reference to the child
    pub fn child(&self) -> Option<&dyn DynRenderObject> {
        self.child.as_deref()
    }

    /// Sets the opacity
    ///
    /// # Panics
    ///
    /// Panics if opacity is not in range [0.0, 1.0]
    pub fn set_opacity(&mut self, opacity: f32) {
        assert!(
            (0.0..=1.0).contains(&opacity),
            "opacity must be between 0.0 and 1.0, got {}",
            opacity
        );
        if (self.opacity - opacity).abs() > f32::EPSILON {
            self.opacity = opacity;
            self.mark_needs_paint();
        }
    }

    /// Returns the current opacity
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Returns true if this render object is fully transparent
    pub fn is_transparent(&self) -> bool {
        self.opacity < f32::EPSILON
    }

    /// Returns true if this render object is fully opaque
    pub fn is_opaque(&self) -> bool {
        (self.opacity - 1.0).abs() < f32::EPSILON
    }
}

impl DynRenderObject for RenderOpacity {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        crate::impl_cached_layout!(self, constraints, {
            if let Some(child) = &mut self.child {
                // Pass constraints through to child
                child.layout(constraints)
            } else {
                // Without child, use smallest size
                constraints.smallest()
            }
        })
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            // NOTE: In a full implementation with layer support, we would:
            // 1. Create a new layer/surface
            // 2. Paint child to that layer
            // 3. Composite layer with opacity
            //
            // For now, we simply paint the child directly.
            // The opacity value is stored and could be used by:
            // - Custom painters that check parent opacity
            // - Layer system when implemented
            // - Widget-level color modifications

            // Skip painting if fully transparent (optimization)
            if !self.is_transparent() {
                child.paint(painter, offset);
            }
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        // Fully transparent objects don't respond to hit tests
        !self.is_transparent()
    }

    fn hit_test_children(
        &self,
        result: &mut flui_types::events::HitTestResult,
        position: Offset,
    ) -> bool {
        if self.is_transparent() {
            // Fully transparent - no hit testing
            return false;
        }
        if let Some(child) = &self.child {
            child.hit_test(result, position)
        } else {
            false
        }
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
        self.mark_needs_paint();
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint_flag = true;
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
    fn test_render_opacity_new() {
        let render = RenderOpacity::new(0.5);
        assert_eq!(render.opacity(), 0.5);
        assert!(render.child().is_none());
        assert!(!render.is_transparent());
        assert!(!render.is_opaque());
    }

    #[test]
    fn test_render_opacity_fully_opaque() {
        let render = RenderOpacity::new(1.0);
        assert_eq!(render.opacity(), 1.0);
        assert!(render.is_opaque());
        assert!(!render.is_transparent());
    }

    #[test]
    fn test_render_opacity_fully_transparent() {
        let render = RenderOpacity::new(0.0);
        assert_eq!(render.opacity(), 0.0);
        assert!(render.is_transparent());
        assert!(!render.is_opaque());
    }

    #[test]
    #[should_panic(expected = "opacity must be between 0.0 and 1.0")]
    fn test_render_opacity_invalid_negative() {
        RenderOpacity::new(-0.5);
    }

    #[test]
    #[should_panic(expected = "opacity must be between 0.0 and 1.0")]
    fn test_render_opacity_invalid_greater_than_one() {
        RenderOpacity::new(1.5);
    }

    #[test]
    fn test_render_opacity_set_opacity() {
        let mut render = RenderOpacity::new(0.5);
        render.set_opacity(0.8);
        assert_eq!(render.opacity(), 0.8);
        assert!(render.needs_paint());
    }

    #[test]
    #[should_panic(expected = "opacity must be between 0.0 and 1.0")]
    fn test_render_opacity_set_invalid_opacity() {
        let mut render = RenderOpacity::new(0.5);
        render.set_opacity(2.0);
    }

    #[test]
    fn test_render_opacity_layout_with_child() {
        let mut render = RenderOpacity::new(0.5);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = render.layout(constraints);

        // Should adopt child size (RenderBox uses biggest())
        assert_eq!(size, Size::new(200.0, 200.0));
    }

    #[test]
    fn test_render_opacity_layout_without_child() {
        let mut render = RenderOpacity::new(0.5);

        let constraints = BoxConstraints::new(50.0, 200.0, 50.0, 200.0);
        let size = render.layout(constraints);

        // Without child, use smallest size
        assert_eq!(size, Size::new(50.0, 50.0));
    }

    #[test]
    fn test_render_opacity_hit_test_transparent() {
        use flui_types::events::HitTestResult;

        let mut render = RenderOpacity::new(0.0);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Fully transparent objects don't respond to hit tests
        let mut result = HitTestResult::new();
        assert!(!render.hit_test(&mut result, Offset::new(10.0, 10.0)));
    }

    #[test]
    fn test_render_opacity_hit_test_opaque() {
        use flui_types::events::HitTestResult;

        let mut render = RenderOpacity::new(1.0);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Layout first to set size
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        render.layout(constraints);

        // Opaque objects should pass hit test to child
        // RenderBox default hit_test returns true if position is within bounds
        let mut result = HitTestResult::new();
        assert!(render.hit_test(&mut result, Offset::new(10.0, 10.0))); // Position (10, 10) is within 100x100 bounds
    }

    #[test]
    fn test_render_opacity_visit_children() {
        let mut render = RenderOpacity::new(0.5);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let mut count = 0;
        render.visit_children(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_opacity_visit_children_no_child() {
        let render = RenderOpacity::new(0.5);

        let mut count = 0;
        render.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_render_opacity_remove_child() {
        let mut render = RenderOpacity::new(0.5);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        assert!(render.child().is_some());

        render.set_child(None);
        assert!(render.child().is_none());
        assert!(render.needs_layout());
    }

    #[test]
    fn test_render_opacity_mark_needs_paint_only() {
        let mut render = RenderOpacity::new(0.5);

        // Changing opacity should only trigger repaint, not relayout
        render.needs_layout_flag = false;
        render.needs_paint_flag = false;

        render.set_opacity(0.8);

        // Should mark needs_paint (and transitively needs_layout via mark_needs_layout in mark_needs_paint)
        assert!(render.needs_paint());
    }
}
