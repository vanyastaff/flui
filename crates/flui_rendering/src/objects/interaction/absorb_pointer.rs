//! RenderAbsorbPointer - absorbs pointer events preventing them from reaching widgets behind
//!
//! Used by AbsorbPointer widget to conditionally block pointer events.

use flui_core::{BoxConstraints, DynRenderObject, ElementId};
use flui_types::events::{HitTestEntry, HitTestResult};
use flui_types::{Offset, Size};
use crate::RenderFlags;

/// RenderAbsorbPointer absorbs pointer events, preventing them from passing through
///
/// When `absorbing` is true, this render object will participate in hit testing
/// and prevent events from reaching widgets behind it, but won't forward them to children.
///
/// # Layout Algorithm
///
/// Simply passes constraints to child and adopts child size (like RenderProxyBox).
///
/// # Hit Testing
///
/// - When `absorbing` is true: Returns true from hit_test, adding entry to result,
///   but doesn't test children. This blocks events from passing through.
/// - When `absorbing` is false: Performs normal hit testing.
///
/// # Difference from RenderIgnorePointer
///
/// - **IgnorePointer**: Transparent to hit testing - events pass through to widgets behind
/// - **AbsorbPointer**: Opaque to hit testing - events don't pass through but are absorbed
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderAbsorbPointer;
///
/// // Create and absorb pointer events
/// let mut render = RenderAbsorbPointer::new(true);
/// ```
pub struct RenderAbsorbPointer {
    /// Element ID for caching
    element_id: Option<ElementId>,
    /// Whether to absorb pointer events
    absorbing: bool,
    /// Child render object
    child: Option<Box<dyn DynRenderObject>>,
    /// Current size
    size: Size,
    /// Current constraints
    constraints: Option<BoxConstraints>,
    /// Render flags (needs_layout, needs_paint, boundaries)
    flags: RenderFlags,
}

impl RenderAbsorbPointer {
    /// Creates a new RenderAbsorbPointer
    ///
    /// # Parameters
    ///
    /// - `absorbing`: If true, absorbs pointer events
    pub fn new(absorbing: bool) -> Self {
        Self {
            element_id: None,
            absorbing,
            child: None,
            size: Size::zero(),
            constraints: None,
            flags: RenderFlags::new(),
        }
    }

    /// Create with element ID for caching
    pub fn with_element_id(absorbing: bool, element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            absorbing,
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

    /// Sets whether to absorb pointer events
    pub fn set_absorbing(&mut self, absorbing: bool) {
        if self.absorbing != absorbing {
            self.absorbing = absorbing;
            // No need to mark_needs_layout or mark_needs_paint,
            // but hit testing behavior changes
        }
    }

    /// Returns whether absorbing pointer events
    pub fn absorbing(&self) -> bool {
        self.absorbing
    }
}

impl DynRenderObject for RenderAbsorbPointer {
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
            // Simply paint child - absorbing doesn't affect rendering
            child.paint(painter, offset);
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        // When absorbing, we hit ourselves to block events
        // When not absorbing, we don't hit ourselves
        self.absorbing
    }

    fn hit_test_children(
        &self,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        if self.absorbing {
            // Don't test children when absorbing - we block the events
            false
        } else if let Some(child) = &self.child {
            // Normal hit testing when not absorbing
            child.hit_test(result, position)
        } else {
            false
        }
    }

    /// Override hit_test to implement absorbing behavior
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        // Check bounds first
        if position.dx < 0.0
            || position.dx >= self.size().width
            || position.dy < 0.0
            || position.dy >= self.size().height
        {
            return false;
        }

        if self.absorbing {
            // When absorbing, we consume the hit test
            // Add ourselves to the result to block events from passing through
            result.add(HitTestEntry::new(position, self.size()));
            return true;
        }

        // When not absorbing, perform normal hit testing on children
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

impl std::fmt::Debug for RenderAbsorbPointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderAbsorbPointer")
            .field("absorbing", &self.absorbing)
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
    fn test_absorb_pointer_layout() {
        let mut render = RenderAbsorbPointer::new(true);

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
    fn test_absorb_pointer_hit_test_when_absorbing() {
        let mut render = RenderAbsorbPointer::new(true);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Hit test should succeed when absorbing (but block children)
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(hit);
        // Result should contain only the absorber, not the child
        assert_eq!(result.entries().len(), 1);
    }

    #[test]
    fn test_absorb_pointer_hit_test_when_not_absorbing() {
        let mut render = RenderAbsorbPointer::new(false);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Hit test should pass through to child when not absorbing
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(hit);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_absorb_pointer_toggle() {
        let mut render = RenderAbsorbPointer::new(false);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Initially not absorbing - should pass through to child
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));
        assert!(hit);
        let initial_count = result.entries().len();

        // Toggle to absorbing
        render.set_absorbing(true);

        // Now should absorb (only absorber in result, not child)
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));
        assert!(hit);
        assert_eq!(result.entries().len(), 1); // Only the absorber

        // Toggle back to not absorbing
        render.set_absorbing(false);

        // Should pass through to child again
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));
        assert!(hit);
        assert_eq!(result.entries().len(), initial_count);
    }

    #[test]
    fn test_absorb_pointer_out_of_bounds() {
        let mut render = RenderAbsorbPointer::new(true);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Out of bounds should not hit even when absorbing
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(150.0, 25.0));

        assert!(!hit);
        assert!(result.is_empty());
    }

    #[test]
    fn test_absorb_vs_ignore_behavior() {
        // This test demonstrates the key difference:
        // - IgnorePointer: transparent (returns false, no entry added)
        // - AbsorbPointer: opaque (returns true, entry added to block)

        let mut absorb = RenderAbsorbPointer::new(true);
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        absorb.set_child(Some(child));

        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        absorb.layout(constraints);

        // AbsorbPointer returns true and adds entry
        let mut absorb_result = HitTestResult::new();
        let absorb_hit = absorb.hit_test(&mut absorb_result, Offset::new(50.0, 25.0));

        assert!(absorb_hit); // Absorb DOES hit
        assert!(!absorb_result.is_empty()); // And adds entry to block

        // Compare with IgnorePointer (would return false and add no entry)
        use crate::objects::interaction::RenderIgnorePointer;

        let mut ignore = RenderIgnorePointer::new(true);
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        ignore.set_child(Some(child));
        ignore.layout(constraints);

        let mut ignore_result = HitTestResult::new();
        let ignore_hit = ignore.hit_test(&mut ignore_result, Offset::new(50.0, 25.0));

        assert!(!ignore_hit); // Ignore does NOT hit
        assert!(ignore_result.is_empty()); // And adds no entry (transparent)
    }
}
