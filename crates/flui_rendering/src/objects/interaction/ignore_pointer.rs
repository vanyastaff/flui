//! RenderIgnorePointer - makes widget invisible to hit testing
//!
//! Used by IgnorePointer widget to conditionally enable/disable pointer events.

use flui_core::{BoxConstraints, DynRenderObject, ElementId};
use crate::RenderFlags;
use flui_types::events::HitTestResult;
use flui_types::{Offset, Size};

/// RenderIgnorePointer makes a widget and its children invisible to hit testing
///
/// When `ignoring` is true, this render object will not participate in hit testing,
/// effectively making it and its subtree transparent to pointer events.
///
/// # Layout Algorithm
///
/// Simply passes constraints to child and adopts child size (like RenderProxyBox).
///
/// # Hit Testing
///
/// - When `ignoring` is true: Always returns false from hit_test, preventing any
///   hit test entries from being added.
/// - When `ignoring` is false: Performs normal hit testing.
///
/// # Difference from RenderAbsorbPointer
///
/// - **IgnorePointer**: Transparent to hit testing - events pass through to widgets behind
/// - **AbsorbPointer**: Opaque to hit testing - events don't pass through but are absorbed
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderIgnorePointer;
///
/// // Create and make invisible to pointer events
/// let mut render = RenderIgnorePointer::new(true);
/// ```
pub struct RenderIgnorePointer {
    /// Element ID for caching
    element_id: Option<ElementId>,
    /// Whether to ignore pointer events
    ignoring: bool,
    /// Child render object
    child: Option<Box<dyn DynRenderObject>>,
    /// Current size
    size: Size,
    /// Current constraints
    constraints: Option<BoxConstraints>,
    /// Render flags
    flags: RenderFlags,
}

impl RenderIgnorePointer {
    /// Creates a new RenderIgnorePointer
    ///
    /// # Parameters
    ///
    /// - `ignoring`: If true, ignores pointer events
    pub fn new(ignoring: bool) -> Self {
        Self {
            element_id: None,
            ignoring,
            child: None,
            size: Size::zero(),
            constraints: None,
            flags: RenderFlags::new(),
        }
    }

    /// Create with element ID
    pub fn with_element_id(ignoring: bool, element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            ignoring,
            child: None,
            size: Size::zero(),
            constraints: None,
            flags: RenderFlags::new(),
        }
    }

    /// Sets element ID
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

    /// Sets whether to ignore pointer events
    pub fn set_ignoring(&mut self, ignoring: bool) {
        if self.ignoring != ignoring {
            self.ignoring = ignoring;
            // No need to mark_needs_layout or mark_needs_paint,
            // but hit testing behavior changes
        }
    }

    /// Returns whether ignoring pointer events
    pub fn ignoring(&self) -> bool {
        self.ignoring
    }
}

impl DynRenderObject for RenderIgnorePointer {
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
            // Simply paint child - ignoring doesn't affect rendering
            child.paint(painter, offset);
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        // Never hit ourselves - we're just a pass-through
        false
    }

    fn hit_test_children(
        &self,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        if self.ignoring {
            // Don't test children when ignoring
            false
        } else if let Some(child) = &self.child {
            // Normal hit testing when not ignoring
            child.hit_test(result, position)
        } else {
            false
        }
    }

    /// Override hit_test to implement ignoring behavior
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        if self.ignoring {
            // When ignoring, completely skip hit testing
            // Events will pass through to widgets behind this one
            return false;
        }

        // When not ignoring, perform normal hit testing
        // Check bounds
        if position.dx < 0.0
            || position.dx >= self.size().width
            || position.dy < 0.0
            || position.dy >= self.size().height
        {
            return false;
        }

        // Only check children (we never hit self)
        self.hit_test_children(result, position)
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

impl std::fmt::Debug for RenderIgnorePointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderIgnorePointer")
            .field("ignoring", &self.ignoring)
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
    fn test_ignore_pointer_layout() {
        let mut render = RenderIgnorePointer::new(true);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout should pass through child size
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        let size = render.layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_ignore_pointer_hit_test_when_ignoring() {
        let mut render = RenderIgnorePointer::new(true);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Hit test should fail when ignoring
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(!hit);
        assert!(result.is_empty());
    }

    #[test]
    fn test_ignore_pointer_hit_test_when_not_ignoring() {
        let mut render = RenderIgnorePointer::new(false);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Hit test should succeed when not ignoring
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(hit);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_ignore_pointer_toggle() {
        let mut render = RenderIgnorePointer::new(false);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Initially not ignoring - should hit
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));
        assert!(hit);

        // Toggle to ignoring
        render.set_ignoring(true);

        // Now should not hit
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));
        assert!(!hit);

        // Toggle back to not ignoring
        render.set_ignoring(false);

        // Should hit again
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));
        assert!(hit);
    }

    #[test]
    fn test_ignore_pointer_out_of_bounds() {
        let mut render = RenderIgnorePointer::new(false);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Out of bounds should not hit
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(150.0, 25.0));

        assert!(!hit);
        assert!(result.is_empty());
    }
}
