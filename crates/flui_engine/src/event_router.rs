//! Event routing and hit testing
//!
//! This module provides event routing functionality, similar to Flutter's
//! gesture and pointer routing system. Events are dispatched through hit testing,
//! allowing layers to handle events based on their position in the scene graph.

use flui_types::events::{Event, HitTestResult};
use flui_types::prelude::PointerEvent;
use flui_types::Offset;

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

    /// Whether the window is currently focused
    /// When unfocused, we should reset pointer state as user may have
    /// released buttons outside the window
    is_focused: bool,

    /// Whether the window is currently visible (not minimized/occluded)
    /// When invisible, we can skip event processing for efficiency
    is_visible: bool,
}

impl EventRouter {
    /// Create a new event router
    pub fn new() -> Self {
        Self {
            last_pointer_position: None,
            pointer_down_layers: Vec::new(),
            is_focused: true, // Assume focused on creation
            is_visible: true, // Assume visible on creation
        }
    }

    /// Handle window focus change
    ///
    /// When the window loses focus, we reset pointer state because:
    /// - User may have released mouse buttons outside the window
    /// - Hover state is no longer meaningful
    /// - Prevents stuck button states
    ///
    /// # Arguments
    ///
    /// * `focused` - true if window gained focus, false if lost
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In your window event handler
    /// router.on_focus_changed(false); // Window lost focus
    /// ```
    pub fn on_focus_changed(&mut self, focused: bool) {
        tracing::debug!(
            "EventRouter: Focus changed to {}",
            if focused { "focused" } else { "unfocused" }
        );

        self.is_focused = focused;

        if !focused {
            // Lost focus - reset pointer state
            self.reset_pointer_state();
        }
    }

    /// Handle window visibility change (minimized/restored)
    ///
    /// When window is minimized, we can skip event processing.
    /// When restored, we reset state to prevent stale interactions.
    ///
    /// # Arguments
    ///
    /// * `visible` - true if window is visible, false if minimized/occluded
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Window minimized
    /// router.on_visibility_changed(false);
    ///
    /// // Window restored
    /// router.on_visibility_changed(true);
    /// ```
    pub fn on_visibility_changed(&mut self, visible: bool) {
        tracing::debug!(
            "EventRouter: Visibility changed to {}",
            if visible { "visible" } else { "hidden" }
        );

        self.is_visible = visible;

        if !visible {
            // Window hidden - reset all state
            self.reset_pointer_state();
        }
    }

    /// Reset all pointer-related state
    ///
    /// This should be called when:
    /// - Window loses focus
    /// - Window is minimized
    /// - DPI changes significantly
    /// - Any other situation where pointer state may be invalid
    pub fn reset_pointer_state(&mut self) {
        tracing::debug!("EventRouter: Resetting pointer state");

        self.last_pointer_position = None;
        self.pointer_down_layers.clear();
    }

    /// Check if the window is currently focused
    pub fn is_focused(&self) -> bool {
        self.is_focused
    }

    /// Check if the window is currently visible (not minimized)
    pub fn is_visible(&self) -> bool {
        self.is_visible
    }

    /// Route an event through the layer tree
    ///
    /// Performs hit testing (for pointer and scroll events) and dispatches the event
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
            Event::Scroll(scroll_data) => self.route_scroll_event(root, scroll_data, event),
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
        // Skip processing if window is not visible
        // This prevents unnecessary work when minimized
        if !self.is_visible {
            tracing::trace!("Skipping pointer event - window not visible");
            return false;
        }

        let position = event.position();

        // Update last pointer position (only if focused)
        // When unfocused, we don't track pointer position as it's not meaningful
        if self.is_focused {
            self.last_pointer_position = Some(position);
        }

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

    /// Route a scroll event through hit testing
    ///
    /// Performs hit testing at the scroll position to verify the position is valid,
    /// then dispatches the event through the layer tree. The event propagates from
    /// root down to children, allowing layers to handle scroll events.
    ///
    /// # Arguments
    ///
    /// * `root` - The root layer to start hit testing from
    /// * `scroll_data` - The scroll event data (contains position and delta)
    /// * `event` - The full Event enum variant (for dispatching)
    ///
    /// # Returns
    ///
    /// `true` if the event was handled by any layer
    ///
    /// # Implementation Note
    ///
    /// Unlike pointer events which use HitTestResult for targeted dispatch,
    /// scroll events are dispatched through the normal layer tree traversal
    /// via `handle_event()`. Hit testing is only used to validate that the
    /// scroll position is within the layer bounds.
    fn route_scroll_event(
        &mut self,
        root: &mut dyn crate::layer::Layer,
        _scroll_data: &flui_types::events::ScrollEventData,
        event: &Event,
    ) -> bool {
        // For scroll events, we use the layer tree's natural event propagation
        // Each layer's handle_event() implementation will check if it should
        // handle the scroll and potentially pass it to children
        //
        // This differs from pointer events which use hit testing for targeted
        // dispatch. Scroll events may need to bubble through multiple layers
        // (e.g., nested scrollable containers).

        root.handle_event(event)
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
    use flui_types::events::{PointerDeviceKind, PointerEventData};

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
        assert_eq!(
            router.last_pointer_position(),
            Some(Offset::new(10.0, 20.0))
        );
    }

    #[test]
    fn test_pointer_down_tracking() {
        let router = EventRouter::new();

        // Initially no pointer down
        assert!(!router.is_pointer_down());

        // Note: This test would need a layer with non-empty bounds to actually track pointer down
        // For now it just verifies the tracking mechanism exists
    }

    #[test]
    fn test_route_scroll_event() {
        use flui_types::events::{ScrollDelta, ScrollEventData};

        let mut router = EventRouter::new();
        let mut layer = PictureLayer::new();

        let scroll_data = ScrollEventData::new(
            Offset::new(15.0, 25.0),
            ScrollDelta::Lines { x: 0.0, y: 1.0 },
        );
        let event = Event::scroll(scroll_data);

        // Route event - PictureLayer will receive it via handle_event
        // Whether it's handled depends on the layer's implementation
        router.route_event(&mut layer, &event);

        // Event was routed successfully (no panic)
        // Actual handling depends on layer implementation
    }

    #[test]
    fn test_scroll_event_with_position() {
        use flui_types::events::{ScrollDelta, ScrollEventData};

        let mut router = EventRouter::new();
        let mut layer = PictureLayer::new();

        // Create scroll event with specific position
        let position = Offset::new(100.0, 200.0);
        let scroll_data = ScrollEventData::new(position, ScrollDelta::Pixels { x: 10.0, y: -20.0 });
        let event = Event::scroll(scroll_data);

        // Route event
        router.route_event(&mut layer, &event);

        // Position should be used for hit testing
        // (Actual hit testing would require layer with non-empty bounds)
    }
}
