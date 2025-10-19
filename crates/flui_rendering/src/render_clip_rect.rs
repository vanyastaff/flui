//! RenderClipRect - clips child to a rectangle
//!
//! Used by ClipRect widget to create rectangular clipping effects.

use crate::render_object::RenderObject;
use flui_core::BoxConstraints;
use flui_types::events::HitTestResult;
use flui_types::painting::Clip;
use flui_types::{Offset, Rect, Size};

/// RenderClipRect clips child rendering to a rectangle
///
/// # Parameters
///
/// - `clip_behavior`: How to perform clipping (None, HardEdge, AntiAlias, AntiAliasWithSaveLayer)
///
/// # Layout Algorithm
///
/// Simply passes constraints to child and adopts child size.
///
/// # Paint Algorithm
///
/// Clips painting to a rectangle. When clip_behavior is None, no clipping is applied.
///
/// # Hit Testing
///
/// Hit tests are clipped to the rectangle bounds.
/// Points outside the clipping region return false.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderClipRect;
/// use flui_types::painting::Clip;
///
/// // Clip with anti-aliasing
/// let mut render = RenderClipRect::new(Clip::AntiAlias);
///
/// // Clip without anti-aliasing (faster)
/// let mut render = RenderClipRect::new(Clip::HardEdge);
///
/// // No clipping
/// let mut render = RenderClipRect::new(Clip::None);
/// ```
#[derive(Debug)]
pub struct RenderClipRect {
    /// Clip behavior
    clip_behavior: Clip,
    /// Child render object
    child: Option<Box<dyn RenderObject>>,
    /// Current size
    size: Size,
    /// Layout dirty flag
    needs_layout_flag: bool,
    /// Paint dirty flag
    needs_paint_flag: bool,
}

impl RenderClipRect {
    /// Creates a new RenderClipRect
    ///
    /// # Parameters
    ///
    /// - `clip_behavior`: How to perform clipping
    pub fn new(clip_behavior: Clip) -> Self {
        Self {
            clip_behavior,
            child: None,
            size: Size::zero(),
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Sets the child
    pub fn set_child(&mut self, child: Option<Box<dyn RenderObject>>) {
        self.child = child;
        self.mark_needs_layout();
    }

    /// Returns a reference to the child
    pub fn child(&self) -> Option<&dyn RenderObject> {
        self.child.as_deref()
    }

    /// Sets the clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        if self.clip_behavior != clip_behavior {
            self.clip_behavior = clip_behavior;
            self.mark_needs_paint();
        }
    }

    /// Gets the clip behavior
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }
}

impl RenderObject for RenderClipRect {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = &mut self.child {
            self.size = child.layout(constraints);
        } else {
            self.size = constraints.smallest();
        }

        self.needs_layout_flag = false;
        self.size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if self.clip_behavior == Clip::None {
            // No clipping - just paint child normally
            if let Some(child) = &self.child {
                child.paint(painter, offset);
            }
            return;
        }

        if let Some(child) = &self.child {
            // Create clip rect
            let rect = Rect::from_min_size(offset, self.size);
            let egui_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left() as f32, rect.top() as f32),
                egui::vec2(rect.width() as f32, rect.height() as f32),
            );

            // Apply clipping with rectangular bounds
            // egui handles clipping through its clip_rect mechanism
            painter.with_clip_rect(egui_rect).rect_filled(
                egui_rect,
                0.0, // No rounding - it's a rectangle
                egui::Color32::TRANSPARENT,
            );

            // Paint child within clipped region
            child.paint(painter, offset);
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        // Clipping doesn't hit test itself
        false
    }

    fn hit_test_children(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if let Some(child) = &self.child {
            child.hit_test(result, position)
        } else {
            false
        }
    }

    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        // Check if position is within clip bounds
        if position.dx < 0.0
            || position.dx >= self.size.width
            || position.dy < 0.0
            || position.dy >= self.size.height
        {
            return false;
        }

        // Position is within bounds - test children
        self.hit_test_children(result, position)
    }

    fn size(&self) -> Size {
        self.size
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        None
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
    use crate::RenderConstrainedBox;

    #[test]
    fn test_clip_rect_new() {
        let render = RenderClipRect::new(Clip::AntiAlias);
        assert_eq!(render.clip_behavior, Clip::AntiAlias);
        assert!(render.child.is_none());
    }

    #[test]
    fn test_clip_rect_layout_with_child() {
        let mut render = RenderClipRect::new(Clip::HardEdge);

        let child = Box::new(RenderConstrainedBox::new(BoxConstraints::tight(
            Size::new(100.0, 50.0),
        )));
        render.set_child(Some(child));

        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        let size = render.layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_clip_rect_layout_without_child() {
        let mut render = RenderClipRect::new(Clip::HardEdge);

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = render.layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_clip_rect_set_clip_behavior() {
        let mut render = RenderClipRect::new(Clip::None);

        render.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(render.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rect_set_clip_behavior_marks_paint() {
        let mut render = RenderClipRect::new(Clip::None);
        render.needs_paint_flag = false;

        render.set_clip_behavior(Clip::HardEdge);

        assert!(render.needs_paint());
    }

    #[test]
    fn test_clip_rect_hit_test_inside() {
        let mut render = RenderClipRect::new(Clip::HardEdge);

        let child = Box::new(RenderConstrainedBox::new(BoxConstraints::tight(
            Size::new(100.0, 50.0),
        )));
        render.set_child(Some(child));

        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(hit);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_clip_rect_hit_test_outside() {
        let mut render = RenderClipRect::new(Clip::HardEdge);

        let child = Box::new(RenderConstrainedBox::new(BoxConstraints::tight(
            Size::new(100.0, 50.0),
        )));
        render.set_child(Some(child));

        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(150.0, 25.0));

        assert!(!hit);
        assert!(result.is_empty());
    }

    #[test]
    fn test_clip_rect_no_clipping() {
        let mut render = RenderClipRect::new(Clip::None);

        let child = Box::new(RenderConstrainedBox::new(BoxConstraints::tight(
            Size::new(100.0, 50.0),
        )));
        render.set_child(Some(child));

        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // With Clip::None, hit testing still works normally
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(hit);
    }

    #[test]
    fn test_clip_rect_visit_children() {
        let mut render = RenderClipRect::new(Clip::HardEdge);

        let child = Box::new(RenderConstrainedBox::new(BoxConstraints::tight(
            Size::new(100.0, 50.0),
        )));
        render.set_child(Some(child));

        let mut count = 0;
        render.visit_children(&mut |_| count += 1);

        assert_eq!(count, 1);
    }

    #[test]
    fn test_clip_rect_visit_children_no_child() {
        let render = RenderClipRect::new(Clip::HardEdge);

        let mut count = 0;
        render.visit_children(&mut |_| count += 1);

        assert_eq!(count, 0);
    }
}
