//! PointerListenerLayer - Layer that registers pointer event handlers
//!
//! This layer wraps another layer and adds hit testing with event handlers.
//! When the pointer hits this layer's bounds, the handler is called.

use crate::layer::Layer;
use crate::painter::Painter;
use flui_types::events::{Event, HitTestEntry, HitTestResult, PointerEventHandler};
use flui_types::{Offset, Rect};

/// Layer that listens for pointer events
///
/// Wraps a child layer and registers an event handler during hit testing.
/// The handler is called when pointer events occur within the layer's bounds.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::layer::PointerListenerLayer;
/// use std::sync::Arc;
///
/// let child = PictureLayer::new();
/// let handler = Arc::new(|event| println!("Pointer event: {:?}", event));
/// let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
///
/// let layer = PointerListenerLayer::new(Box::new(child), handler, bounds);
/// ```
pub struct PointerListenerLayer {
    /// Child layer to render
    child: Box<dyn Layer>,

    /// Event handler for pointer events
    handler: PointerEventHandler,

    /// Bounds for hit testing
    bounds: Rect,
}

impl PointerListenerLayer {
    /// Create a new PointerListenerLayer
    ///
    /// # Arguments
    ///
    /// * `child` - The child layer to wrap
    /// * `handler` - The event handler to call when pointer events occur
    /// * `bounds` - The bounds for hit testing
    pub fn new(child: Box<dyn Layer>, handler: PointerEventHandler, bounds: Rect) -> Self {
        Self {
            child,
            handler,
            bounds,
        }
    }

    /// Get the child layer
    pub fn child(&self) -> &dyn Layer {
        &*self.child
    }

    /// Get mutable access to the child layer
    pub fn child_mut(&mut self) -> &mut dyn Layer {
        &mut *self.child
    }

    /// Get the event handler
    pub fn handler(&self) -> &PointerEventHandler {
        &self.handler
    }

    /// Set new bounds
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
}

impl Layer for PointerListenerLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        // Simply delegate painting to child
        self.child.paint(painter);
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn is_visible(&self) -> bool {
        self.child.is_visible()
    }

    fn mark_needs_paint(&mut self) {
        self.child.mark_needs_paint();
    }

    fn dispose(&mut self) {
        self.child.dispose();
    }

    fn is_disposed(&self) -> bool {
        self.child.is_disposed()
    }

    fn debug_description(&self) -> String {
        format!("PointerListenerLayer({:?})", self.bounds)
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Check if position is within our bounds
        if !self.bounds.contains(position) {
            return false;
        }

        // Add ourselves to hit test result with handler
        // Convert min (Point) to Offset for subtraction
        let local_pos = position - Offset::from(self.bounds.min);
        let entry = HitTestEntry::with_handler(local_pos, self.bounds.size(), self.handler.clone());
        result.add(entry);

        // Also hit test child
        // Child might add additional entries (e.g., nested listeners)
        let child_hit = self.child.hit_test(position, result);

        // We're hit if either we're in bounds OR child was hit
        true || child_hit
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Delegate to child (for scroll events, etc.)
        self.child.handle_event(event)
    }
}

impl std::fmt::Debug for PointerListenerLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerListenerLayer")
            .field("bounds", &self.bounds)
            .field("has_handler", &true)
            .field("child", &self.child.debug_description())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::PictureLayer;
    use flui_types::Size;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_pointer_listener_layer_creation() {
        let child = PictureLayer::new();
        let handler = Arc::new(move |_event: &PointerEvent| {});
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let layer = PointerListenerLayer::new(Box::new(child), handler, bounds);

        assert_eq!(layer.bounds(), bounds);
        assert!(!layer.is_disposed());
    }

    #[test]
    fn test_hit_test_within_bounds() {
        let child = PictureLayer::new();
        let handler = Arc::new(move |_event: &PointerEvent| {});
        let bounds = Rect::from_xywh(10.0, 10.0, 100.0, 100.0);

        let layer = PointerListenerLayer::new(Box::new(child), handler, bounds);
        let mut result = HitTestResult::new();

        // Hit within bounds
        let hit = layer.hit_test(Offset::new(50.0, 50.0), &mut result);
        assert!(hit);
        assert!(!result.is_empty());

        // Verify entry has handler
        let entry = result.front().unwrap();
        assert!(entry.handler.is_some());
    }

    #[test]
    fn test_hit_test_outside_bounds() {
        let child = PictureLayer::new();
        let handler = Arc::new(move |_event: &PointerEvent| {});
        let bounds = Rect::from_xywh(10.0, 10.0, 100.0, 100.0);

        let layer = PointerListenerLayer::new(Box::new(child), handler, bounds);
        let mut result = HitTestResult::new();

        // Hit outside bounds
        let hit = layer.hit_test(Offset::new(5.0, 5.0), &mut result);
        assert!(!hit);
        assert!(result.is_empty());
    }

    #[test]
    fn test_handler_called_on_dispatch() {
        use flui_types::events::{PointerDeviceKind, PointerEvent, PointerEventData};

        let child = PictureLayer::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let handler = Arc::new(move |_event: &PointerEvent| {
            called_clone.store(true, Ordering::SeqCst);
        });

        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let layer = PointerListenerLayer::new(Box::new(child), handler, bounds);

        let mut result = HitTestResult::new();
        layer.hit_test(Offset::new(50.0, 50.0), &mut result);

        // Dispatch event
        let data = PointerEventData::new(Offset::new(50.0, 50.0), PointerDeviceKind::Mouse);
        let event = PointerEvent::Down(data);
        result.dispatch(&event);

        // Handler should have been called
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_local_position_calculation() {
        let child = PictureLayer::new();
        let handler = Arc::new(move |_event: &PointerEvent| {});
        let bounds = Rect::from_xywh(10.0, 20.0, 100.0, 100.0);

        let layer = PointerListenerLayer::new(Box::new(child), handler, bounds);
        let mut result = HitTestResult::new();

        // Hit at global position (30, 40)
        layer.hit_test(Offset::new(30.0, 40.0), &mut result);

        // Local position should be (20, 20) = (30-10, 40-20)
        let entry = result.front().unwrap();
        assert_eq!(entry.local_position, Offset::new(20.0, 20.0));
    }

    #[test]
    fn test_set_bounds() {
        let child = PictureLayer::new();
        let handler = Arc::new(move |_event: &PointerEvent| {});
        let bounds1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let mut layer = PointerListenerLayer::new(Box::new(child), handler, bounds1);
        assert_eq!(layer.bounds(), bounds1);

        let bounds2 = Rect::from_xywh(10.0, 10.0, 200.0, 200.0);
        layer.set_bounds(bounds2);
        assert_eq!(layer.bounds(), bounds2);
    }
}
