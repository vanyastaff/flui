//! RenderOffstage - controls whether child is painted
//!
//! Used by Offstage widget to hide widgets without removing them from the tree.

use flui_core::{BoxConstraints, DynRenderObject, ElementId};
use flui_types::events::HitTestResult;
use flui_types::{Offset, Size};
use crate::RenderFlags;

/// RenderOffstage controls whether its child is painted and hit tested
///
/// When `offstage` is true:
/// - Child is NOT painted (invisible)
/// - Child is NOT hit tested (doesn't receive pointer events)
/// - Child IS still laid out (maintains its size)
///
/// This is useful for:
/// - Keeping a widget in the tree but hiding it
/// - Preserving widget state while hiding
/// - Animating visibility without rebuilding
///
/// # Layout Algorithm
///
/// Simply passes constraints to child and adopts child size (like RenderProxyBox).
///
/// # Painting
///
/// When `offstage` is true, skips painting the child entirely.
///
/// # Hit Testing
///
/// When `offstage` is true, always returns false (no hit testing).
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderOffstage;
///
/// // Create and hide child
/// let mut render = RenderOffstage::new(true);
/// ```
pub struct RenderOffstage {
    /// Element ID for caching
    element_id: Option<ElementId>,
    /// Whether the child is offstage (hidden)
    offstage: bool,
    /// Child render object
    child: Option<Box<dyn DynRenderObject>>,
    /// Current size
    size: Size,
    /// Current constraints
    constraints: Option<BoxConstraints>,
    /// Render flags (needs_layout, needs_paint, boundaries)
    flags: RenderFlags,
}

impl RenderOffstage {
    /// Creates a new RenderOffstage
    ///
    /// # Parameters
    ///
    /// - `offstage`: If true, child is hidden (not painted or hit tested)
    pub fn new(offstage: bool) -> Self {
        Self {
            element_id: None,
            offstage,
            child: None,
            size: Size::zero(),
            constraints: None,
            flags: RenderFlags::new(),
        }
    }

    /// Creates RenderOffstage with element ID for caching
    pub fn with_element_id(offstage: bool, element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            offstage,
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

    /// Sets whether the child is offstage
    pub fn set_offstage(&mut self, offstage: bool) {
        if self.offstage != offstage {
            self.offstage = offstage;
            // Only need to repaint, not re-layout
            self.mark_needs_paint();
        }
    }

    /// Returns whether the child is offstage
    pub fn offstage(&self) -> bool {
        self.offstage
    }
}

impl DynRenderObject for RenderOffstage {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        crate::impl_cached_layout!(self, constraints, {
            if let Some(child) = &mut self.child {
                // Always layout the child, even when offstage
                // This preserves the child's size and state
                child.layout(constraints)
            } else {
                // Without child, use smallest size
                constraints.smallest()
            }
        })
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if self.offstage {
            // Don't paint child when offstage
            return;
        }

        if let Some(child) = &self.child {
            // Paint child normally when not offstage
            child.paint(painter, offset);
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        // Never hit ourselves
        false
    }

    fn hit_test_children(
        &self,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        if self.offstage {
            // Don't hit test children when offstage
            return false;
        }

        if let Some(child) = &self.child {
            // Normal hit testing when not offstage
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

impl std::fmt::Debug for RenderOffstage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderOffstage")
            .field("offstage", &self.offstage)
            .field("has_child", &self.child.is_some())
            .field("size", &self.size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderConstrainedBox;

    #[test]
    fn test_offstage_new() {
        let render = RenderOffstage::new(true);
        assert!(render.offstage);
        assert!(render.child.is_none());
    }

    #[test]
    fn test_offstage_layout() {
        let mut render = RenderOffstage::new(true);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout should work even when offstage
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        let size = render.layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_offstage_hit_test_when_offstage() {
        let mut render = RenderOffstage::new(true);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Hit test should fail when offstage
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(!hit);
        assert!(result.is_empty());
    }

    #[test]
    fn test_offstage_hit_test_when_not_offstage() {
        let mut render = RenderOffstage::new(false);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Hit test should succeed when not offstage
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(hit);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_offstage_toggle() {
        let mut render = RenderOffstage::new(false);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Initially not offstage - should hit
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));
        assert!(hit);

        // Toggle to offstage
        render.set_offstage(true);

        // Now should not hit
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));
        assert!(!hit);

        // Toggle back to not offstage
        render.set_offstage(false);

        // Should hit again
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));
        assert!(hit);
    }

    #[test]
    fn test_offstage_preserves_layout() {
        let mut render = RenderOffstage::new(false);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        let size_before = render.layout(constraints);

        // Toggle to offstage
        render.set_offstage(true);

        // Re-layout should give same size (layout is preserved)
        let size_after = render.layout(constraints);

        assert_eq!(size_before, size_after);
        assert_eq!(size_after, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_offstage_set_offstage_marks_paint() {
        let mut render = RenderOffstage::new(false);

        // Clear paint flag
        render.flags.clear_needs_paint();

        // Toggle offstage
        render.set_offstage(true);

        // Should mark needs paint
        assert!(render.needs_paint());
    }
}
