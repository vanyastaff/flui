//! Event routing infrastructure
//!
//! EventRouter is the central hub for routing input events to UI elements.
//! It uses hit testing for pointer events and focus management for keyboard events.

use crate::{
    focus_manager::FocusManager,
    hit_test::{HitTestResult, HitTestable},
};
use flui_types::events::{Event, KeyEvent, PointerEvent, ScrollEventData};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Central event router
///
/// Routes events to appropriate UI elements based on:
/// - **Pointer events**: Spatial hit testing
/// - **Keyboard events**: Focus management
/// - **Scroll events**: Hit testing + bubbling
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::EventRouter;
///
/// let mut router = EventRouter::new();
///
/// // Route pointer event
/// router.route_event(&mut root_layer, &Event::Pointer(pointer_event));
///
/// // Route keyboard event (goes to focused element)
/// router.route_event(&mut root_layer, &Event::Key(key_event));
/// ```
pub struct EventRouter {
    /// Pointer state tracking (for drag gestures)
    pointer_state: Arc<RwLock<HashMap<u32, PointerState>>>,
}

/// State for a single pointer (finger/mouse)
#[derive(Debug, Clone)]
struct PointerState {
    /// Is pointer currently down?
    is_down: bool,

    /// Last known position
    last_position: flui_types::geometry::Offset,

    /// Target that received the down event (for drag tracking)
    down_target: Option<HitTestResult>,
}

impl EventRouter {
    /// Create a new event router
    pub fn new() -> Self {
        Self {
            pointer_state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Route an event to the appropriate target
    ///
    /// Dispatches based on event type:
    /// - Pointer → hit testing
    /// - Key → focused element
    /// - Scroll → hit testing + bubbling
    pub fn route_event(&mut self, root: &mut dyn HitTestable, event: &Event) {
        match event {
            Event::Pointer(pointer_event) => {
                self.route_pointer_event(root, pointer_event);
            }
            Event::Key(key_event) => {
                self.route_key_event(root, key_event);
            }
            Event::Scroll(scroll_event_data) => {
                self.route_scroll_event(root, scroll_event_data);
            }
            _ => {
                // Other events not yet implemented
                tracing::debug!("Unhandled event type: {:?}", event);
            }
        }
    }

    /// Route pointer event via hit testing
    fn route_pointer_event(&mut self, root: &mut dyn HitTestable, event: &PointerEvent) {
        let position = event.position();
        let pointer_id = event.pointer_id();

        match event {
            PointerEvent::Down(_) => {
                // Perform hit test
                let mut result = HitTestResult::new();
                root.hit_test(position, &mut result);

                tracing::debug!(
                    position = ?position,
                    hit_count = result.entries().len(),
                    has_handlers = result.entries().iter().any(|e| e.handler.is_some()),
                    "EventRouter: hit test complete for Down event"
                );

                // Store pointer state for drag tracking
                self.pointer_state.write().insert(
                    pointer_id,
                    PointerState {
                        is_down: true,
                        last_position: position,
                        down_target: Some(result.clone()),
                    },
                );

                // Dispatch to hit targets
                result.dispatch(event);
            }

            PointerEvent::Move(_) => {
                // Check if this is a drag (pointer is down)
                let is_dragging = self
                    .pointer_state
                    .read()
                    .get(&pointer_id)
                    .map(|s| s.is_down)
                    .unwrap_or(false);

                if is_dragging {
                    // Send to original down target (drag continuity)
                    if let Some(state) = self.pointer_state.read().get(&pointer_id) {
                        if let Some(target) = &state.down_target {
                            target.dispatch(event);
                        }
                    }
                } else {
                    // Normal hover - hit test at current position
                    let mut result = HitTestResult::new();
                    root.hit_test(position, &mut result);
                    result.dispatch(event);
                }

                // Update last position
                if let Some(state) = self.pointer_state.write().get_mut(&pointer_id) {
                    state.last_position = position;
                }
            }

            PointerEvent::Up(_) => {
                // Send to original down target
                if let Some(state) = self.pointer_state.write().remove(&pointer_id) {
                    if let Some(target) = state.down_target {
                        target.dispatch(event);
                    }
                }
            }

            PointerEvent::Cancel(_) => {
                // Cancel drag
                if let Some(state) = self.pointer_state.write().remove(&pointer_id) {
                    if let Some(target) = state.down_target {
                        target.dispatch(event);
                    }
                }
            }

            _ => {
                // Other pointer events (Enter, Exit, etc.)
                let mut result = HitTestResult::new();
                root.hit_test(position, &mut result);
                result.dispatch(event);
            }
        }
    }

    /// Route keyboard event to focused element
    fn route_key_event(&mut self, _root: &mut dyn HitTestable, event: &KeyEvent) {
        if let Some(focused_id) = FocusManager::global().focused() {
            tracing::trace!("Routing key event to focused element: {:?}", focused_id);

            // TODO: Look up focused element in tree and dispatch
            // For now, FocusNode callbacks handle this directly
            let _ = (focused_id, event); // Suppress unused warning
        } else {
            tracing::trace!("No focused element, key event ignored");
        }
    }

    /// Route scroll event via hit testing with bubbling
    fn route_scroll_event(&mut self, root: &mut dyn HitTestable, event: &ScrollEventData) {
        let position = event.position;

        // Hit test to find scrollable targets
        let mut result = HitTestResult::new();
        root.hit_test(position, &mut result);

        // TODO: Dispatch scroll event with bubbling
        // (innermost → outermost until handled)
        let _ = result; // Suppress unused warning
        tracing::trace!("Scroll event at {:?}", position);
    }

    /// Clear all pointer state (useful for testing or window focus loss)
    pub fn clear_pointer_state(&mut self) {
        self.pointer_state.write().clear();
    }
}

impl Default for EventRouter {
    fn default() -> Self {
        Self::new()
    }
}

// Helper extension trait for PointerEvent
trait PointerEventExt {
    /// Get pointer ID (for multi-touch)
    fn pointer_id(&self) -> u32;
}

impl PointerEventExt for PointerEvent {
    fn pointer_id(&self) -> u32 {
        // Use device() method which works for all variants
        self.device() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::ElementId;
    use flui_types::geometry::{Offset, Rect};

    /// Mock layer for testing
    struct MockLayer {
        bounds: Rect,
    }

    impl HitTestable for MockLayer {
        fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
            if self.bounds.contains(position) {
                result.add(crate::hit_test::HitTestEntry::new(ElementId::new(1), position, self.bounds));
                true
            } else {
                false
            }
        }
    }

    #[test]
    fn test_event_router_creation() {
        let router = EventRouter::new();
        assert!(router.pointer_state.read().is_empty());
    }

    #[test]
    fn test_pointer_down_up_tracking() {
        let mut router = EventRouter::new();
        let mut layer = MockLayer {
            bounds: Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        };

        // Down event
        let down = PointerEvent::Down(flui_types::events::PointerEventData::new(
            Offset::new(50.0, 50.0),
            flui_types::events::PointerDeviceKind::Mouse,
        ));
        router.route_event(&mut layer, &Event::Pointer(down));

        // Should track pointer
        assert_eq!(router.pointer_state.read().len(), 1);

        // Up event
        let up = PointerEvent::Up(flui_types::events::PointerEventData::new(
            Offset::new(50.0, 50.0),
            flui_types::events::PointerDeviceKind::Mouse,
        ));
        router.route_event(&mut layer, &Event::Pointer(up));

        // Should clear pointer
        assert_eq!(router.pointer_state.read().len(), 0);
    }

    #[test]
    fn test_clear_pointer_state() {
        let mut router = EventRouter::new();

        // Add some state
        router.pointer_state.write().insert(
            0,
            PointerState {
                is_down: true,
                last_position: Offset::new(0.0, 0.0),
                down_target: None,
            },
        );

        router.clear_pointer_state();
        assert!(router.pointer_state.read().is_empty());
    }
}
