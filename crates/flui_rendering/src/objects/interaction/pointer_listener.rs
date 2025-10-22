//! RenderPointerListener - intercepts pointer events for GestureDetector
//!
//! Used by GestureDetector widget to handle pointer events like taps, drags, etc.

use flui_core::{BoxConstraints, DynRenderObject, ElementId};
use flui_types::events::{HitTestEntry, HitTestResult, PointerEventHandler};
use flui_types::{Offset, Size};
use crate::RenderFlags;

/// RenderPointerListener intercepts pointer events
///
/// This is the proper Flutter-style implementation for pointer event handling.
/// Instead of a global registry, each RenderPointerListener registers its handler
/// during hit testing by adding it to the HitTestEntry.
///
/// # Layout Algorithm
///
/// Simply passes constraints to child and adopts child size (like RenderProxyBox).
///
/// # Hit Testing
///
/// During hit testing, adds an entry with the event handler so that when events
/// are dispatched, only the widgets that were actually hit receive the events.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderPointerListener;
/// use std::sync::Arc;
///
/// let handler = Arc::new(|event: &PointerEvent| {
///     println!("Pointer event: {:?}", event);
/// });
///
/// let mut render = RenderPointerListener::new(handler);
/// ```
pub struct RenderPointerListener {
    /// Element ID for caching
    element_id: Option<ElementId>,
    /// Event handler for pointer events
    handler: PointerEventHandler,
    /// Child render object
    child: Option<Box<dyn DynRenderObject>>,
    /// Current size
    size: Size,
    /// Current constraints
    constraints: Option<BoxConstraints>,
    /// Render flags
    flags: RenderFlags,
}

impl RenderPointerListener {
    /// Creates a new RenderPointerListener with event handler
    ///
    /// # Parameters
    ///
    /// - `handler`: Callback to invoke when pointer events occur
    pub fn new(handler: PointerEventHandler) -> Self {
        Self {
            element_id: None,
            handler,
            child: None,
            size: Size::zero(),
            constraints: None,
            flags: RenderFlags::new(),
        }
    }

    /// Create with element ID
    pub fn with_element_id(handler: PointerEventHandler, element_id: ElementId) -> Self {
        Self {
            element_id: Some(element_id),
            handler,
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

    /// Sets the event handler
    pub fn set_handler(&mut self, handler: PointerEventHandler) {
        self.handler = handler;
    }

    /// Returns a reference to the event handler
    pub fn handler(&self) -> &PointerEventHandler {
        &self.handler
    }
}

impl DynRenderObject for RenderPointerListener {
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
            // Simply paint child - we don't modify rendering, only intercept events
            child.paint(painter, offset);
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        // Always respond to hit tests (even if transparent)
        // This ensures we can intercept pointer events
        true
    }

    fn hit_test_children(
        &self,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        if let Some(child) = &self.child {
            child.hit_test(result, position)
        } else {
            false
        }
    }

    /// Override hit_test to add our handler to the result
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        // Check bounds
        if position.dx < 0.0
            || position.dx >= self.size().width
            || position.dy < 0.0
            || position.dy >= self.size().height
        {
            return false;
        }

        // Check children first (front to back)
        let hit_child = self.hit_test_children(result, position);

        // Then check self
        let hit_self = self.hit_test_self(position);

        // Add to result if hit, INCLUDING our handler
        if hit_child || hit_self {
            result.add(HitTestEntry::with_handler(
                position,
                self.size(),
                self.handler.clone(),
            ));
            return true;
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

impl std::fmt::Debug for RenderPointerListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderPointerListener")
            .field("has_handler", &true)
            .field("has_child", &self.child.is_some())
            .field("size", &self.size)
            .finish()
    }
}
