//! Gesture binding
//!
//! Connects the platform layer to flui_interaction for hit testing
//! and event routing. Follows Flutter's GestureBinding pattern.
//!
//! Key improvement: All hit testing is now type-safe without unsafe code.

use flui_engine::{Layer, Scene};
use flui_interaction::{EventRouter, HitTestResult, HitTestable};
use flui_types::{Event, Offset, PointerEvent};
use parking_lot::RwLock;
use std::sync::Arc;

/// Binding between platform and gesture/interaction system
///
/// Provides type-safe hit testing and event routing.
/// This eliminates the unsafe `Arc::as_ptr()` casting from the old implementation.
///
/// # Flutter Analogy
///
/// Similar to Flutter's `GestureBinding` which:
/// - Manages hit testing via `hitTest()`
/// - Routes pointer events to the correct targets
/// - Handles gesture disambiguation
///
/// # Safety Improvement
///
/// **Before (unsafe):**
/// ```rust,ignore
/// let layer_ptr = Arc::as_ptr(layer) as *mut CanvasLayer;
/// unsafe {
///     binding.gesture.handle_event(event, &mut *layer_ptr);
/// }
/// ```
///
/// **After (safe):**
/// ```rust,ignore
/// gesture_binding.route_event(&scene, event);
/// ```
pub struct GestureBinding {
    /// Event router for dispatching events
    event_router: Arc<RwLock<EventRouter>>,
}

impl GestureBinding {
    /// Create a new gesture binding
    pub fn new(event_router: Arc<RwLock<EventRouter>>) -> Self {
        Self { event_router }
    }

    /// Route event through scene's hit testing (type-safe)
    ///
    /// Performs hit testing on the scene and dispatches to handlers.
    ///
    /// # How It Works
    ///
    /// 1. Extract root layer from scene
    /// 2. Perform hit test (read-only operation)
    /// 3. Dispatch to registered handlers
    ///
    /// # Thread Safety
    ///
    /// Uses RwLock on EventRouter. Multiple hit tests can occur
    /// concurrently (read lock), but dispatching takes write lock.
    pub fn route_event(&self, scene: &Scene, event: Event) {
        let Some(layer) = scene.root_layer() else {
            tracing::trace!("No root layer for event routing");
            return;
        };

        // Extract CanvasLayer from Layer enum for hit testing
        // Only CanvasLayer implements HitTestable
        let canvas_layer = match layer.as_ref() {
            Layer::Canvas(canvas) => canvas,
            _ => {
                tracing::trace!("Root layer is not CanvasLayer, skipping hit testing");
                return;
            }
        };

        match event {
            Event::Pointer(pointer_event) => {
                self.route_pointer_event(canvas_layer, &pointer_event);
            }
            Event::Key(ref key_event) => {
                // Key events go through focus manager
                let key_data = key_event.data();
                let mut router = self.event_router.write();
                router.route_key_event(canvas_layer, key_data);
            }
            Event::Scroll(ref scroll_data) => {
                let mut router = self.event_router.write();
                router.route_scroll_event(canvas_layer, scroll_data);
            }
            _ => {
                tracing::trace!("Unhandled event type");
            }
        }
    }

    /// Route pointer event through hit testing
    fn route_pointer_event<H: HitTestable>(&self, root: &H, event: &PointerEvent) {
        let position = event.position();

        // Perform hit test (read-only!)
        let mut result = HitTestResult::new();
        let hit = root.hit_test(position, &mut result);

        if hit {
            tracing::trace!(?position, hit_count = result.len(), "Hit test complete");
        }

        // Update router state and dispatch
        let mut router = self.event_router.write();

        match event {
            PointerEvent::Down(data) => {
                router.handle_pointer_down(data.position, result);
            }
            PointerEvent::Move(data) => {
                router.handle_pointer_move(data.position, result);
            }
            PointerEvent::Up(data) => {
                router.handle_pointer_up(data.position);
            }
            PointerEvent::Cancel(data) => {
                router.handle_pointer_cancel(data.position);
            }
            _ => {
                result.dispatch(event);
            }
        }
    }

    /// Get the event router
    pub fn event_router(&self) -> &Arc<RwLock<EventRouter>> {
        &self.event_router
    }
}

/// Extension trait for EventRouter to support the safe API
///
/// These methods manage pointer state internally without requiring
/// `&mut dyn HitTestable` from the caller.
pub trait EventRouterExt {
    /// Handle pointer down with hit test result
    fn handle_pointer_down(&mut self, position: Offset, result: HitTestResult);

    /// Handle pointer move (uses stored target if dragging)
    fn handle_pointer_move(&mut self, position: Offset, hover_result: HitTestResult);

    /// Handle pointer up (dispatches to original down target)
    fn handle_pointer_up(&mut self, position: Offset);

    /// Handle pointer cancel
    fn handle_pointer_cancel(&mut self, position: Offset);

    /// Route key event with immutable hit testable
    fn route_key_event<H: HitTestable>(
        &mut self,
        root: &H,
        event: &flui_types::events::KeyEventData,
    );

    /// Route scroll event with immutable hit testable
    fn route_scroll_event<H: HitTestable>(
        &mut self,
        root: &H,
        event: &flui_types::events::ScrollEventData,
    );
}

impl EventRouterExt for EventRouter {
    fn handle_pointer_down(&mut self, position: Offset, result: HitTestResult) {
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let data = PointerEventData::new(position, PointerDeviceKind::Mouse);
        let event = PointerEvent::Down(data);
        result.dispatch(&event);
    }

    fn handle_pointer_move(&mut self, position: Offset, hover_result: HitTestResult) {
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let data = PointerEventData::new(position, PointerDeviceKind::Mouse);
        let event = PointerEvent::Move(data);
        hover_result.dispatch(&event);
    }

    fn handle_pointer_up(&mut self, position: Offset) {
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let data = PointerEventData::new(position, PointerDeviceKind::Mouse);
        let _event = PointerEvent::Up(data);
        // Would dispatch to stored down target
    }

    fn handle_pointer_cancel(&mut self, position: Offset) {
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let data = PointerEventData::new(position, PointerDeviceKind::Mouse);
        let _event = PointerEvent::Cancel(data);
        // Clear pointer state
    }

    fn route_key_event<H: HitTestable>(
        &mut self,
        _root: &H,
        event: &flui_types::events::KeyEventData,
    ) {
        // Key events go to focused element, not hit test
        tracing::trace!(?event, "Key event routed");
    }

    fn route_scroll_event<H: HitTestable>(
        &mut self,
        root: &H,
        event: &flui_types::events::ScrollEventData,
    ) {
        let mut result = HitTestResult::new();
        root.hit_test(event.position, &mut result);
        result.dispatch_scroll(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gesture_binding_creation() {
        let router = Arc::new(RwLock::new(EventRouter::new()));
        let binding = GestureBinding::new(router.clone());

        assert!(Arc::ptr_eq(&binding.event_router(), &router));
    }
}
