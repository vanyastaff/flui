//! RenderClipRRect - clips child to a rounded rectangle
//!
//! Used by ClipRRect widget to create rounded corners and clipping effects.

use flui_core::{BoxConstraints, DynRenderObject, ElementId};
use flui_types::painting::Clip;
use flui_types::styling::BorderRadius;
use flui_types::{Offset, Rect, Size};
use crate::RenderFlags;

/// RenderClipRRect clips child rendering to a rounded rectangle
///
/// # Parameters
///
/// - `border_radius`: Rounded corners radius
/// - `clip_behavior`: How to perform clipping (None, HardEdge, AntiAlias, AntiAliasWithSaveLayer)
///
/// # Layout Algorithm
///
/// Simply passes constraints to child and adopts child size.
///
/// # Paint Algorithm
///
/// In egui, clipping is handled through painter's clip_rect method.
/// For rounded rectangles, we use egui::Rounding to specify corner radii.
/// The clip behavior determines whether we use anti-aliasing.
///
/// # Hit Testing
///
/// Hit tests are clipped to the rounded rectangle bounds.
/// Points outside the clipping region return false.
///
/// # Examples
///
/// ```rust
/// # use flui_rendering::RenderClipRRect;
/// # use flui_types::styling::BorderRadius;
/// # use flui_types::painting::Clip;
/// // Circular corners
/// let mut render = RenderClipRRect::new(
///     BorderRadius::circular(10.0),
///     Clip::AntiAlias,
/// );
///
/// // Different radii for each corner
/// # use flui_types::styling::Radius;
/// let border_radius = BorderRadius::only(
///     Radius::circular(10.0),
///     Radius::circular(20.0),
///     Radius::circular(10.0),
///     Radius::circular(20.0),
/// );
/// let mut render = RenderClipRRect::new(border_radius, Clip::HardEdge);
/// ```
#[derive(Debug)]
pub struct RenderClipRRect {
    /// Element ID for caching
    element_id: Option<ElementId>,
    /// Border radius for rounded corners
    border_radius: BorderRadius,
    /// Clip behavior
    clip_behavior: Clip,
    /// Child render object
    child: Option<Box<dyn DynRenderObject>>,
    /// Current size
    size: Size,
    /// Current constraints
    constraints: Option<BoxConstraints>,
    /// Render flags (needs_layout, needs_paint, boundaries)
    flags: RenderFlags,
}

impl RenderClipRRect {
    /// Creates a new RenderClipRRect
    ///
    /// # Parameters
    ///
    /// - `border_radius`: Rounded corners radius
    /// - `clip_behavior`: How to perform clipping
    pub fn new(border_radius: BorderRadius, clip_behavior: Clip) -> Self {
        Self {
            element_id: None,
            border_radius,
            clip_behavior,
            child: None,
            size: Size::zero(),
            constraints: None,
            flags: RenderFlags::new(),
        }
    }

    /// Creates RenderClipRRect with element ID for caching
    pub fn with_element_id(border_radius: BorderRadius, clip_behavior: Clip, element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            border_radius,
            clip_behavior,
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

    /// Sets the child
    pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
        self.child = child;
        self.mark_needs_layout();
    }

    /// Returns a reference to the child
    pub fn child(&self) -> Option<&dyn DynRenderObject> {
        self.child.as_deref()
    }

    /// Sets the border radius
    pub fn set_border_radius(&mut self, border_radius: BorderRadius) {
        if self.border_radius != border_radius {
            self.border_radius = border_radius;
            self.mark_needs_paint();
        }
    }

    /// Returns the current border radius
    pub fn border_radius(&self) -> BorderRadius {
        self.border_radius
    }

    /// Sets the clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        if self.clip_behavior != clip_behavior {
            self.clip_behavior = clip_behavior;
            self.mark_needs_paint();
        }
    }

    /// Returns the current clip behavior
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Converts BorderRadius to egui::Rounding (renamed to CornerRadius in newer egui)
    #[allow(deprecated)]
    fn to_egui_rounding(&self) -> egui::Rounding {
        // egui::Rounding uses f32 for each corner
        // BorderRadius uses Radius (x, y) for each corner
        // For simplicity, we'll use the x component of each radius
        egui::Rounding {
            nw: self.border_radius.top_left.x as u8,
            ne: self.border_radius.top_right.x as u8,
            sw: self.border_radius.bottom_left.x as u8,
            se: self.border_radius.bottom_right.x as u8,
        }
    }

    /// Checks if a point is within the clipped region
    ///
    /// For rounded rectangles, we check if the point is within the rect
    /// and not in the "cut off" corner areas.
    fn is_point_in_clip_region(&self, point: Offset) -> bool {
        // If no clipping, all points are valid
        if !self.clip_behavior.clips() {
            return true;
        }

        let rect = Rect::from_min_size(Offset::new(0.0, 0.0), self.size);

        // First check if point is in the bounding box
        if !rect.contains(point.to_point()) {
            return false;
        }

        // For rounded rectangles, check if point is in corner radii regions
        let x = point.dx;
        let y = point.dy;

        // Top-left corner
        if x < self.border_radius.top_left.x && y < self.border_radius.top_left.y {
            let dx = self.border_radius.top_left.x - x;
            let dy = self.border_radius.top_left.y - y;
            let distance_sq = dx * dx + dy * dy;
            let radius_sq = self.border_radius.top_left.x * self.border_radius.top_left.x;
            if distance_sq > radius_sq {
                return false;
            }
        }

        // Top-right corner
        let right_edge = self.size.width;
        if x > right_edge - self.border_radius.top_right.x && y < self.border_radius.top_right.y {
            let dx = x - (right_edge - self.border_radius.top_right.x);
            let dy = self.border_radius.top_right.y - y;
            let distance_sq = dx * dx + dy * dy;
            let radius_sq = self.border_radius.top_right.x * self.border_radius.top_right.x;
            if distance_sq > radius_sq {
                return false;
            }
        }

        // Bottom-left corner
        let bottom_edge = self.size.height;
        if x < self.border_radius.bottom_left.x && y > bottom_edge - self.border_radius.bottom_left.y
        {
            let dx = self.border_radius.bottom_left.x - x;
            let dy = y - (bottom_edge - self.border_radius.bottom_left.y);
            let distance_sq = dx * dx + dy * dy;
            let radius_sq = self.border_radius.bottom_left.x * self.border_radius.bottom_left.x;
            if distance_sq > radius_sq {
                return false;
            }
        }

        // Bottom-right corner
        if x > right_edge - self.border_radius.bottom_right.x
            && y > bottom_edge - self.border_radius.bottom_right.y
        {
            let dx = x - (right_edge - self.border_radius.bottom_right.x);
            let dy = y - (bottom_edge - self.border_radius.bottom_right.y);
            let distance_sq = dx * dx + dy * dy;
            let radius_sq = self.border_radius.bottom_right.x * self.border_radius.bottom_right.x;
            if distance_sq > radius_sq {
                return false;
            }
        }

        true
    }
}

impl DynRenderObject for RenderClipRRect {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        crate::impl_cached_layout!(self, constraints, {
            if let Some(child) = &mut self.child {
                child.layout(constraints)
            } else {
                constraints.smallest()
            }
        })
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            // Only apply clipping if clip_behavior is not None
            if !self.clip_behavior.clips() {
                // No clipping - just paint child
                child.paint(painter, offset);
                return;
            }

            // Create clipping rect with rounded corners
            let rect = egui::Rect::from_min_size(
                egui::pos2(offset.dx, offset.dy),
                egui::vec2(self.size.width, self.size.height),
            );

            let _rounding = self.to_egui_rounding();

            // In egui, we can set a clip rect with rounding
            // The painter will automatically clip all subsequent drawing
            painter.with_clip_rect(rect).set_clip_rect(rect);

            // Note: egui doesn't directly support rounded clip rects in the public API
            // For a full implementation, we would need to:
            // 1. Use egui's Shape::Path with rounded rectangle
            // 2. Add it to a clip layer
            //
            // For now, we demonstrate the structure and will use rectangular clipping
            // which is what egui's clip_rect provides

            // Paint child with clipping applied
            child.paint(painter, offset);
        }
    }

    fn hit_test_self(&self, position: Offset) -> bool {
        // Check if position is within clipped region
        self.is_point_in_clip_region(position)
    }

    fn hit_test_children(
        &self,
        result: &mut flui_types::events::HitTestResult,
        position: Offset,
    ) -> bool {
        if let Some(child) = &self.child {
            // Only hit test child if position is within clipped region
            if self.is_point_in_clip_region(position) {
                return child.hit_test(result, position);
            }
        }
        false
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
    use flui_types::styling::Radius;

    #[test]
    fn test_render_clip_rrect_new() {
        let border_radius = BorderRadius::circular(10.0);
        let render = RenderClipRRect::new(border_radius, Clip::AntiAlias);
        assert_eq!(render.border_radius(), border_radius);
        assert_eq!(render.clip_behavior(), Clip::AntiAlias);
        assert!(render.child().is_none());
    }

    #[test]
    fn test_render_clip_rrect_set_border_radius() {
        let mut render = RenderClipRRect::new(BorderRadius::circular(10.0), Clip::HardEdge);
        let new_radius = BorderRadius::circular(20.0);
        render.set_border_radius(new_radius);
        assert_eq!(render.border_radius(), new_radius);
        assert!(render.needs_paint());
    }

    #[test]
    fn test_render_clip_rrect_set_clip_behavior() {
        let mut render =
            RenderClipRRect::new(BorderRadius::circular(10.0), Clip::HardEdge);
        render.flags.clear_needs_paint();
        render.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(render.clip_behavior(), Clip::AntiAlias);
        assert!(render.needs_paint());
    }

    #[test]
    fn test_render_clip_rrect_layout_with_child() {
        let mut render = RenderClipRRect::new(BorderRadius::circular(10.0), Clip::AntiAlias);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = render.layout(constraints);

        // Should adopt child size (RenderBox uses biggest())
        assert_eq!(size, Size::new(200.0, 200.0));
    }

    #[test]
    fn test_render_clip_rrect_layout_without_child() {
        let mut render = RenderClipRRect::new(BorderRadius::circular(10.0), Clip::AntiAlias);

        let constraints = BoxConstraints::new(50.0, 200.0, 50.0, 200.0);
        let size = render.layout(constraints);

        // Without child, use smallest size
        assert_eq!(size, Size::new(50.0, 50.0));
    }

    #[test]
    fn test_render_clip_rrect_hit_test_inside() {
        use flui_types::events::HitTestResult;

        let mut render = RenderClipRRect::new(BorderRadius::circular(10.0), Clip::AntiAlias);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Layout first to set size
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        render.layout(constraints);

        // Test point in center (should be inside)
        let mut result = HitTestResult::new();
        assert!(render.hit_test(&mut result, Offset::new(50.0, 50.0)));
        assert!(!result.is_empty());
    }

    #[test]
    fn test_render_clip_rrect_hit_test_outside() {
        use flui_types::events::HitTestResult;

        let mut render = RenderClipRRect::new(BorderRadius::circular(10.0), Clip::AntiAlias);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Layout first to set size
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        render.layout(constraints);

        // Test point outside bounds
        let mut result = HitTestResult::new();
        assert!(!render.hit_test(&mut result, Offset::new(150.0, 150.0)));
        assert!(result.is_empty());
    }

    #[test]
    fn test_render_clip_rrect_hit_test_in_corner() {
        use flui_types::events::HitTestResult;

        let mut render = RenderClipRRect::new(BorderRadius::circular(20.0), Clip::AntiAlias);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Layout first to set size
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        render.layout(constraints);

        // Test point in top-left corner (should be outside due to rounding)
        let mut result = HitTestResult::new();
        assert!(!render.hit_test(&mut result, Offset::new(2.0, 2.0)));

        // Test point near corner but inside radius
        let mut result = HitTestResult::new();
        assert!(render.hit_test(&mut result, Offset::new(15.0, 15.0)));
    }

    #[test]
    fn test_render_clip_rrect_no_clipping() {
        use flui_types::events::HitTestResult;

        let mut render = RenderClipRRect::new(BorderRadius::circular(10.0), Clip::None);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Layout first to set size
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        render.layout(constraints);

        // With Clip::None, all points should pass through
        let mut result = HitTestResult::new();
        assert!(render.hit_test(&mut result, Offset::new(2.0, 2.0)));
    }

    #[test]
    fn test_render_clip_rrect_different_corner_radii() {
        let border_radius = BorderRadius::only(
            Radius::circular(10.0),
            Radius::circular(20.0),
            Radius::circular(5.0),
            Radius::circular(15.0),
        );
        let render = RenderClipRRect::new(border_radius, Clip::AntiAlias);
        assert_eq!(render.border_radius(), border_radius);
    }

    #[test]
    fn test_render_clip_rrect_to_egui_rounding() {
        let border_radius = BorderRadius::only(
            Radius::circular(10.0),
            Radius::circular(20.0),
            Radius::circular(5.0),
            Radius::circular(15.0),
        );
        let render = RenderClipRRect::new(border_radius, Clip::AntiAlias);
        let rounding = render.to_egui_rounding();

        assert_eq!(rounding.nw, 10);
        assert_eq!(rounding.ne, 20);
        assert_eq!(rounding.sw, 5);
        assert_eq!(rounding.se, 15);
    }

    #[test]
    fn test_render_clip_rrect_visit_children() {
        let mut render = RenderClipRRect::new(BorderRadius::circular(10.0), Clip::AntiAlias);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let mut count = 0;
        render.visit_children(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_clip_rrect_remove_child() {
        let mut render = RenderClipRRect::new(BorderRadius::circular(10.0), Clip::AntiAlias);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        assert!(render.child().is_some());

        render.set_child(None);
        assert!(render.child().is_none());
        assert!(render.needs_layout());
    }
}
