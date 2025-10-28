//! Event routing and hit testing
//!
//! This module provides event routing functionality, similar to Flutter's
//! gesture and pointer routing system. Events are dispatched through hit testing,
//! allowing layers to handle events based on their position in the scene graph.

use crate::layer::BoxedLayer;
use flui_types::{Event, HitTestResult, Offset, PointerEvent};

/// Event router for dispatching events through the layer tree
///
/// The EventRouter performs hit testing to determine which layers should
/// receive events, then dispatches those events to the appropriate handlers.
///
/// # Example
///
/// ```rust,ignore
/// let mut router = EventRouter::new();
///
/// // Handle pointer down event
/// let event = Event::pointer(PointerEvent::Down(data));
/// router.route_event(&mut scene.root_mut(), &event);
/// ```
pub struct EventRouter {
    /// Last known pointer position for hover tracking
    last_pointer_position: Option<Offset>,

    /// Layers that were hit on the last pointer down
    /// Used for tracking dragging and ensuring up events go to the same layer
    pointer_down_layers: Vec<usize>,
}

impl EventRouter {
    /// Create a new event router
    pub fn new() -> Self {
        Self {
            last_pointer_position: None,
            pointer_down_layers: Vec::new(),
        }
    }

    /// Route an event through the layer tree
    ///
    /// Performs hit testing (for pointer events) and dispatches the event
    /// to appropriate layers.
    ///
    /// # Arguments
    ///
    /// * `root` - The root layer to start routing from
    /// * `event` - The event to route
    ///
    /// # Returns
    ///
    /// `true` if the event was handled by any layer
    pub fn route_event(&mut self, root: &mut dyn crate::layer::Layer, event: &Event) -> bool {
        match event {
            Event::Pointer(pointer_event) => self.route_pointer_event(root, pointer_event),
            Event::Key(_key_event) => {
                // Keyboard events go to focused layer (not implemented yet)
                // For now, just send to root
                root.handle_event(event)
            }
            Event::Scroll(_scroll_data) => {
                // Scroll events need custom handling since HitTestResult expects PointerEvent
                // For now, just send to root
                // TODO: Implement proper scroll event routing with hit testing
                root.handle_event(event)
            }
            Event::Window(_) => {
                // Window events go to all layers
                root.handle_event(event)
            }
        }
    }

    /// Route a pointer event through hit testing
    fn route_pointer_event(
        &mut self,
        root: &mut dyn crate::layer::Layer,
        event: &PointerEvent,
    ) -> bool {
        let position = event.position();

        // Update last pointer position
        self.last_pointer_position = Some(position);

        // Perform hit testing
        let mut result = HitTestResult::new();
        let hit = root.hit_test(position, &mut result);

        if !hit {
            return false;
        }

        // Track pointer down for drag handling
        if matches!(event, PointerEvent::Down(_)) {
            self.pointer_down_layers.clear();
            for (i, _) in result.entries().iter().enumerate() {
                self.pointer_down_layers.push(i);
            }
        }

        // Dispatch to hit test result
        result.dispatch(event);

        // Clear pointer down tracking on pointer up
        if matches!(event, PointerEvent::Up(_)) {
            self.pointer_down_layers.clear();
        }

        true
    }

    /// Get the last known pointer position
    pub fn last_pointer_position(&self) -> Option<Offset> {
        self.last_pointer_position
    }

    /// Check if a pointer button is currently pressed
    pub fn is_pointer_down(&self) -> bool {
        !self.pointer_down_layers.is_empty()
    }
}

impl Default for EventRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::PictureLayer;
    use flui_types::{PointerDeviceKind, PointerEventData, Rect};

    #[test]
    fn test_event_router_creation() {
        let router = EventRouter::new();
        assert_eq!(router.last_pointer_position(), None);
        assert!(!router.is_pointer_down());
    }

    #[test]
    fn test_route_pointer_event() {
        let mut router = EventRouter::new();
        let mut layer = PictureLayer::new();

        let data = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        let event = Event::pointer(PointerEvent::Down(data));

        // Route event (will not hit since PictureLayer has empty bounds by default)
        let handled = router.route_event(&mut layer, &event);

        // Event not handled (no bounds)
        assert!(!handled);
        assert_eq!(router.last_pointer_position(), Some(Offset::new(10.0, 20.0)));
    }

    #[test]
    fn test_pointer_down_tracking() {
        let mut router = EventRouter::new();
        let mut layer = PictureLayer::new();

        // Initially no pointer down
        assert!(!router.is_pointer_down());

        // Note: This test would need a layer with non-empty bounds to actually track pointer down
        // For now it just verifies the tracking mechanism exists
    }
}
