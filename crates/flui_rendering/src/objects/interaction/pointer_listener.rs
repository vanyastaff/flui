//! RenderPointerListener - handles pointer events
//!
//! This RenderObject wraps a child and listens for pointer events,
//! calling the appropriate callbacks when events occur.

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::{BoxedLayer, PointerListenerLayer};
use flui_types::events::{PointerEvent, PointerEventHandler};
use flui_types::{constraints::BoxConstraints, Offset, Rect, Size};
use std::sync::Arc;

/// Pointer event callbacks
///
/// These callbacks are called when pointer events occur within the widget's bounds.
#[derive(Clone)]
pub struct PointerCallbacks {
    /// Called when pointer is pressed down
    pub on_pointer_down: Option<PointerEventHandler>,

    /// Called when pointer is released
    pub on_pointer_up: Option<PointerEventHandler>,

    /// Called when pointer moves
    pub on_pointer_move: Option<PointerEventHandler>,

    /// Called when pointer is cancelled
    pub on_pointer_cancel: Option<PointerEventHandler>,
}

impl PointerCallbacks {
    /// Create new empty callbacks
    pub fn new() -> Self {
        Self {
            on_pointer_down: None,
            on_pointer_up: None,
            on_pointer_move: None,
            on_pointer_cancel: None,
        }
    }

    /// Set on_pointer_down callback
    pub fn with_on_pointer_down<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_down = Some(Arc::new(callback));
        self
    }

    /// Set on_pointer_up callback
    pub fn with_on_pointer_up<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_up = Some(Arc::new(callback));
        self
    }

    /// Set on_pointer_move callback
    pub fn with_on_pointer_move<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_move = Some(Arc::new(callback));
        self
    }

    /// Set on_pointer_cancel callback
    pub fn with_on_pointer_cancel<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_cancel = Some(Arc::new(callback));
        self
    }
}

impl Default for PointerCallbacks {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PointerCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerCallbacks")
            .field("on_pointer_down", &self.on_pointer_down.is_some())
            .field("on_pointer_up", &self.on_pointer_up.is_some())
            .field("on_pointer_move", &self.on_pointer_move.is_some())
            .field("on_pointer_cancel", &self.on_pointer_cancel.is_some())
            .finish()
    }
}

/// RenderObject that listens for pointer events
///
/// This widget detects pointer events (mouse clicks, touches) and
/// calls the appropriate callbacks. It wraps a child and doesn't affect
/// layout, but creates a PointerListenerLayer for hit testing.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderPointerListener, PointerCallbacks};
/// use std::sync::Arc;
///
/// let callbacks = PointerCallbacks::new()
///     .with_on_pointer_down(|event| println!("Pointer down: {:?}", event.position()));
///
/// let mut listener = RenderPointerListener::new(callbacks);
/// ```
#[derive(Debug)]
pub struct RenderPointerListener {
    /// Event callbacks
    pub callbacks: PointerCallbacks,

    /// Cached size from last layout
    size: Size,
}

impl RenderPointerListener {
    /// Create new RenderPointerListener
    pub fn new(callbacks: PointerCallbacks) -> Self {
        Self {
            callbacks,
            size: Size::ZERO,
        }
    }

    /// Get the callbacks
    pub fn callbacks(&self) -> &PointerCallbacks {
        &self.callbacks
    }

    /// Set new callbacks
    pub fn set_callbacks(&mut self, callbacks: PointerCallbacks) {
        self.callbacks = callbacks;
        // No need to mark needs_layout or needs_paint - callbacks don't affect rendering
    }

    /// Create the unified event handler from individual callbacks
    fn create_handler(&self) -> PointerEventHandler {
        let callbacks = self.callbacks.clone();
        Arc::new(move |event: &PointerEvent| {
            match event {
                PointerEvent::Down(_) => {
                    if let Some(callback) = &callbacks.on_pointer_down {
                        callback(event);
                    }
                }
                PointerEvent::Up(_) => {
                    if let Some(callback) = &callbacks.on_pointer_up {
                        callback(event);
                    }
                }
                PointerEvent::Move(_) => {
                    if let Some(callback) = &callbacks.on_pointer_move {
                        callback(event);
                    }
                }
                PointerEvent::Cancel(_) => {
                    if let Some(callback) = &callbacks.on_pointer_cancel {
                        callback(event);
                    }
                }
                _ => {}
            }
        })
    }
}

impl SingleRender for RenderPointerListener {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child with same constraints
        let size = tree.layout_child(child_id, constraints);

        // Cache size for use in paint
        self.size = size;

        size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Paint child first
        let child_layer = tree.paint_child(child_id, offset);

        // Create bounds for hit testing
        let bounds = Rect::from_min_size(offset, self.size);

        // Create unified event handler
        let handler = self.create_handler();

        // Wrap in PointerListenerLayer for hit testing
        Box::new(PointerListenerLayer::new(child_layer, handler, bounds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_pointer_listener_new() {
        let callbacks = PointerCallbacks::new();
        let listener = RenderPointerListener::new(callbacks);
        assert!(listener.callbacks().on_pointer_down.is_none());
        assert!(listener.callbacks().on_pointer_up.is_none());
        assert!(listener.callbacks().on_pointer_move.is_none());
        assert!(listener.callbacks().on_pointer_cancel.is_none());
    }

    #[test]
    fn test_render_pointer_listener_with_callbacks() {
        let callbacks = PointerCallbacks::new()
            .with_on_pointer_down(|_| {})
            .with_on_pointer_up(|_| {});

        let listener = RenderPointerListener::new(callbacks);
        assert!(listener.callbacks().on_pointer_down.is_some());
        assert!(listener.callbacks().on_pointer_up.is_some());
        assert!(listener.callbacks().on_pointer_move.is_none());
    }

    #[test]
    fn test_render_pointer_listener_set_callbacks() {
        let callbacks1 = PointerCallbacks::new();
        let mut listener = RenderPointerListener::new(callbacks1);

        let callbacks2 = PointerCallbacks::new().with_on_pointer_down(|_| {});
        listener.set_callbacks(callbacks2);
        assert!(listener.callbacks().on_pointer_down.is_some());
    }

    #[test]
    fn test_pointer_callbacks_debug() {
        let callbacks = PointerCallbacks::new()
            .with_on_pointer_down(|_| {})
            .with_on_pointer_move(|_| {});

        let debug_str = format!("{:?}", callbacks);
        assert!(debug_str.contains("PointerCallbacks"));
        assert!(debug_str.contains("on_pointer_down"));
    }

    #[test]
    fn test_pointer_callbacks_builder() {
        let callbacks = PointerCallbacks::new()
            .with_on_pointer_down(|_| {})
            .with_on_pointer_up(|_| {})
            .with_on_pointer_move(|_| {})
            .with_on_pointer_cancel(|_| {});

        assert!(callbacks.on_pointer_down.is_some());
        assert!(callbacks.on_pointer_up.is_some());
        assert!(callbacks.on_pointer_move.is_some());
        assert!(callbacks.on_pointer_cancel.is_some());
    }
}
