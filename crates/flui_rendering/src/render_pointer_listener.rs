//! RenderPointerListener - intercepts pointer events for GestureDetector
//!
//! Used by GestureDetector widget to handle pointer events like taps, drags, etc.

use crate::render_object::RenderObject;
use flui_core::BoxConstraints;
use flui_types::events::{HitTestEntry, HitTestResult, PointerEventHandler};
use flui_types::{Offset, Size};

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
    /// Event handler for pointer events
    handler: PointerEventHandler,
    /// Child render object
    child: Option<Box<dyn RenderObject>>,
    /// Current size
    size: Size,
    /// Layout dirty flag
    needs_layout_flag: bool,
    /// Paint dirty flag
    needs_paint_flag: bool,
}

impl RenderPointerListener {
    /// Creates a new RenderPointerListener with event handler
    ///
    /// # Parameters
    ///
    /// - `handler`: Callback to invoke when pointer events occur
    pub fn new(handler: PointerEventHandler) -> Self {
        Self {
            handler,
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

    /// Sets the event handler
    pub fn set_handler(&mut self, handler: PointerEventHandler) {
        self.handler = handler;
    }

    /// Returns a reference to the event handler
    pub fn handler(&self) -> &PointerEventHandler {
        &self.handler
    }
}

impl RenderObject for RenderPointerListener {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = &mut self.child {
            // Pass constraints through to child
            self.size = child.layout(constraints);
        } else {
            // Without child, use smallest size
            self.size = constraints.smallest();
        }

        self.needs_layout_flag = false;
        self.size
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

impl std::fmt::Debug for RenderPointerListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderPointerListener")
            .field("has_handler", &true)
            .field("has_child", &self.child.is_some())
            .field("size", &self.size)
            .finish()
    }
}
