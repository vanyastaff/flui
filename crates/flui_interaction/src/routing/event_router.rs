//! Event routing infrastructure
//!
//! EventRouter is the central hub for routing input events to UI elements.
//! It uses hit testing for pointer events and focus management for keyboard events.

use super::focus::FocusManager;
use flui_types::geometry::Pixels;

use super::hit_test::{HitTestResult, HitTestable};
use crate::events::{Event, KeyEvent, PointerEvent, PointerEventExt, ScrollEventData};
use crate::ids::PointerId;
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
    pointer_state: Arc<RwLock<HashMap<PointerId, PointerStateTracking>>>,
}

/// State for a single pointer (finger/mouse)
#[derive(Debug, Clone)]
struct PointerStateTracking {
    /// Is pointer currently down?
    is_down: bool,

    /// Last known position
    last_position: flui_types::geometry::Offset<Pixels>,

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
            Event::Keyboard(key_event) | Event::Key(key_event) => {
                self.route_key_event(root, key_event);
            }
            Event::Scroll(scroll_event_data) => {
                self.route_scroll_event(root, scroll_event_data);
            }
        }
    }

    /// Route pointer event via hit testing
    fn route_pointer_event(&mut self, root: &mut dyn HitTestable, event: &PointerEvent) {
        let position = event.position();
        let pointer_id = get_pointer_id(event);

        match event {
            PointerEvent::Down(_) => {
                // Perform hit test
                let mut result = HitTestResult::new();
                root.hit_test(position, &mut result);

                tracing::trace!(
                    position = ?position,
                    hit_count = result.len(),
                    has_handlers = result.iter().any(|e| e.handler.is_some()),
                    "EventRouter: hit test complete for Down event"
                );

                // Store pointer state for drag tracking
                self.pointer_state.write().insert(
                    pointer_id,
                    PointerStateTracking {
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
                // Other pointer events (Enter, Leave, Scroll, Gesture)
                let mut result = HitTestResult::new();
                root.hit_test(position, &mut result);
                result.dispatch(event);
            }
        }
    }

    /// Route keyboard event to focused element.
    ///
    /// Events are dispatched via FocusManager:
    /// 1. Global key handlers (shortcuts)
    /// 2. Focused node's handler
    ///
    /// Returns `true` if the event was handled.
    fn route_key_event(&mut self, _root: &mut dyn HitTestable, event: &KeyEvent) -> bool {
        let handled = FocusManager::global().dispatch_key_event(event);

        if !handled {
            if FocusManager::global().focused().is_some() {
                tracing::trace!("Key event not handled by focused element");
            } else {
                tracing::trace!("No focused element for key event");
            }
        }

        handled
    }

    /// Route scroll event via hit testing with bubbling.
    ///
    /// Scroll events bubble from innermost (first hit) to outermost (last hit)
    /// until a handler returns `EventPropagation::Stop`.
    ///
    /// Returns `true` if the event was handled.
    fn route_scroll_event(&mut self, root: &mut dyn HitTestable, event: &ScrollEventData) -> bool {
        let position = event.position;

        // Hit test to find scrollable targets
        let mut result = HitTestResult::new();
        root.hit_test(position, &mut result);

        tracing::trace!(
            position = ?position,
            hit_count = result.len(),
            scroll_handlers = result.entries_with_scroll_handlers().count(),
            "Scroll event routing"
        );

        // Dispatch with bubbling (innermost → outermost until handled)
        let handled = result.dispatch_scroll(event);

        if !handled {
            tracing::trace!("Scroll event not handled by any element");
        }

        handled
    }

    /// Clear all pointer state (useful for testing or window focus loss)
    pub fn clear_pointer_state(&mut self) {
        self.pointer_state.write().clear();
    }
}

/// Helper to extract pointer ID from event
fn get_pointer_id(event: &PointerEvent) -> PointerId {
    let id = match event {
        PointerEvent::Down(e) => e.pointer.pointer_id,
        PointerEvent::Up(e) => e.pointer.pointer_id,
        PointerEvent::Move(e) => e.pointer.pointer_id,
        PointerEvent::Cancel(info) | PointerEvent::Enter(info) | PointerEvent::Leave(info) => {
            info.pointer_id
        }
        PointerEvent::Scroll(e) => e.pointer.pointer_id,
        PointerEvent::Gesture(e) => e.pointer.pointer_id,
    };
    // Use 0 for primary pointer, hash for others
    let raw_id = match id {
        Some(p) if p.is_primary_pointer() => 0,
        Some(p) => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            p.hash(&mut hasher);
            (hasher.finish() & 0x7FFFFFFF) as i32
        }
        None => 0,
    };
    PointerId::new(raw_id)
}

impl Default for EventRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::hit_test::HitTestEntry;
    use super::*;
    use crate::events::{make_down_event, PointerType};
    use flui_foundation::RenderId;
    use flui_types::geometry::{Offset, Rect};

    /// Mock layer for testing
    pub(crate) struct MockLayer {
        pub(crate) bounds: Rect,
    }

    impl HitTestable for MockLayer {
        fn hit_test(&self, position: Offset<Pixels>, result: &mut HitTestResult) -> bool {
            if self.bounds.contains(position.into()) {
                result.add(HitTestEntry::new(RenderId::new(1)));
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
        use crate::events::make_up_event;

        let mut router = EventRouter::new();
        let mut layer = MockLayer {
            bounds: Rect::from_xywh(Pixels(0.0), Pixels(0.0), Pixels(100.0), Pixels(100.0)),
        };

        // Down event
        let down = make_down_event(Offset::new(Pixels(50.0), Pixels(50.0)), PointerType::Mouse);
        router.route_event(&mut layer, &Event::Pointer(down));

        // Should track pointer
        assert_eq!(router.pointer_state.read().len(), 1);

        // Up event
        let up = make_up_event(Offset::new(Pixels(50.0), Pixels(50.0)), PointerType::Mouse);
        router.route_event(&mut layer, &Event::Pointer(up));

        // Should clear pointer
        assert_eq!(router.pointer_state.read().len(), 0);
    }

    #[test]
    fn test_clear_pointer_state() {
        use crate::ids::PointerId;

        let mut router = EventRouter::new();

        // Add some state
        router.pointer_state.write().insert(
            PointerId::new(0),
            PointerStateTracking {
                is_down: true,
                last_position: Offset::new(Pixels(0.0), Pixels(0.0)),
                down_target: None,
            },
        );

        router.clear_pointer_state();
        assert!(router.pointer_state.read().is_empty());
    }
}
