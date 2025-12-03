//! Gesture binding - bridge to EventRouter
//!
//! GestureBinding converts platform events (from winit) to FLUI events
//! and routes them through the EventRouter to the appropriate handlers.

use super::BindingBase;
use flui_interaction::EventRouter;
use flui_types::Event;
use parking_lot::RwLock;
use std::sync::Arc;

/// Gesture binding - bridges platform events to EventRouter
///
/// # Architecture
///
/// ```text
/// winit events → GestureBinding → EventRouter → Layer tree → Gesture handlers
/// ```
///
/// # Thread-Safety
///
/// Uses `Arc<RwLock<EventRouter>>` for thread-safe event routing.
/// Multiple readers can access the router concurrently, but writes are exclusive.
pub struct GestureBinding {
    /// Event router for dispatching events to layer tree
    event_router: Arc<RwLock<EventRouter>>,
}

impl GestureBinding {
    /// Create a new GestureBinding
    ///
    /// Initializes an empty EventRouter that will be populated with
    /// gesture recognizers during widget build.
    pub fn new() -> Self {
        Self {
            event_router: Arc::new(RwLock::new(EventRouter::new())),
        }
    }

    /// Handle a platform event
    ///
    /// Routes the event through the EventRouter to find and invoke
    /// the appropriate gesture handlers in the layer tree.
    ///
    /// # Parameters
    ///
    /// - `event`: The FLUI event to route (Pointer, Key, etc.)
    /// - `root`: The root HitTestable object (typically a Layer)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let pointer_event = Event::Pointer(PointerEvent::Down { ... });
    /// binding.handle_event(pointer_event, &mut root_layer);
    /// ```
    pub fn handle_event(&self, event: Event, root: &mut dyn flui_interaction::HitTestable) {
        let mut router = self.event_router.write();
        router.route_event(root, &event);
    }

    /// Get shared reference to the event router
    ///
    /// Used by embedder to route events and by widgets to register handlers.
    #[must_use]
    pub fn event_router(&self) -> Arc<RwLock<EventRouter>> {
        self.event_router.clone()
    }
}

impl Default for GestureBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingBase for GestureBinding {
    fn init(&mut self) {
        tracing::debug!("GestureBinding initialized");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gesture_binding_creation() {
        let binding = GestureBinding::new();
        let _router = binding.event_router();
        // Should not panic
    }

    #[test]
    fn test_gesture_binding_init() {
        let mut binding = GestureBinding::new();
        binding.init();
        // Should not panic
    }
}
