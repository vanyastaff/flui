//! Centralized pointer event routing
//!
//! PointerRouter is a global registry for pointer event handlers.
//! Unlike hit testing (spatial routing), PointerRouter allows any handler
//! to receive events for a specific pointer regardless of position.
//!
//! This is useful for:
//! - Gesture recognizers that need to track pointers across the screen
//! - Drag gestures that continue even when pointer leaves the original target
//! - Modal dialogs that capture all pointer events
//!
//! Flutter reference: https://api.flutter.dev/flutter/gestures/PointerRouter-class.html
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::{PointerRouter, PointerId};
//! use std::sync::Arc;
//!
//! let router = PointerRouter::new();
//!
//! // Register a handler for a specific pointer
//! let handler = Arc::new(|event: &PointerEvent| {
//!     println!("Pointer event: {:?}", event);
//! });
//! router.add_route(pointer_id, handler);
//!
//! // Route an event - all registered handlers receive it
//! router.route(&pointer_event);
//!
//! // Remove when done
//! router.remove_route(pointer_id, handler);
//! ```

use crate::events::PointerEvent;
use crate::ids::PointerId;
use flui_types::geometry::Pixels;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Handler for routed pointer events.
///
/// Unlike hit test handlers, these don't return propagation control -
/// all registered handlers always receive the event.
pub type PointerRouteHandler = Arc<dyn Fn(&PointerEvent) + Send + Sync>;

/// Global handler that receives all pointer events.
pub type GlobalPointerHandler = Arc<dyn Fn(&PointerEvent) + Send + Sync>;

/// Centralized pointer event router.
///
/// Allows handlers to register for pointer events by pointer ID,
/// regardless of spatial position. Events are delivered to all
/// registered handlers for that pointer.
///
/// # Thread Safety
///
/// PointerRouter uses `parking_lot::RwLock` for efficient concurrent access.
/// Multiple readers can check routes simultaneously, while writes are exclusive.
///
/// # Example
///
/// ```rust,ignore
/// let router = PointerRouter::new();
///
/// // Gesture recognizer registers for pointer events
/// let recognizer_handler = Arc::new(|event| {
///     // Handle drag updates even when pointer leaves original target
/// });
/// router.add_route(pointer_id, recognizer_handler);
///
/// // Later, platform layer routes events
/// router.route(&pointer_event);
/// ```
pub struct PointerRouter {
    /// Routes per pointer ID
    routes: RwLock<HashMap<PointerId, Vec<PointerRouteHandler>>>,

    /// Global handlers (receive all events)
    global_handlers: RwLock<Vec<GlobalPointerHandler>>,
}

impl std::fmt::Debug for PointerRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let routes = self.routes.read();
        let global_count = self.global_handlers.read().len();
        f.debug_struct("PointerRouter")
            .field("pointer_count", &routes.len())
            .field("global_handler_count", &global_count)
            .finish()
    }
}

impl Default for PointerRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl PointerRouter {
    /// Create a new pointer router.
    pub fn new() -> Self {
        Self {
            routes: RwLock::new(HashMap::new()),
            global_handlers: RwLock::new(Vec::new()),
        }
    }

    /// Get the global pointer router instance.
    ///
    /// This is a singleton - the same instance is returned every time.
    pub fn global() -> &'static PointerRouter {
        static INSTANCE: std::sync::OnceLock<PointerRouter> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(PointerRouter::new)
    }

    /// Add a route handler for a specific pointer.
    ///
    /// The handler will receive all events for this pointer until removed.
    /// Multiple handlers can be registered for the same pointer.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let handler = Arc::new(|event: &PointerEvent| {
    ///     println!("Received: {:?}", event);
    /// });
    /// router.add_route(pointer_id, handler);
    /// ```
    pub fn add_route(&self, pointer: PointerId, handler: PointerRouteHandler) {
        let mut routes = self.routes.write();
        routes.entry(pointer).or_default().push(handler);

        tracing::trace!(pointer = pointer.get(), "Added pointer route");
    }

    /// Remove a route handler for a specific pointer.
    ///
    /// Uses Arc pointer equality to find and remove the handler.
    /// Returns `true` if the handler was found and removed.
    pub fn remove_route(&self, pointer: PointerId, handler: &PointerRouteHandler) -> bool {
        let mut routes = self.routes.write();

        if let Some(handlers) = routes.get_mut(&pointer) {
            let initial_len = handlers.len();
            handlers.retain(|h| !Arc::ptr_eq(h, handler));

            let removed = handlers.len() < initial_len;

            // Clean up empty entries
            if handlers.is_empty() {
                routes.remove(&pointer);
            }

            if removed {
                tracing::trace!(pointer = pointer.get(), "Removed pointer route");
            }

            removed
        } else {
            false
        }
    }

    /// Remove all routes for a specific pointer.
    ///
    /// Call this when a pointer is released or cancelled.
    pub fn remove_all_routes(&self, pointer: PointerId) {
        let mut routes = self.routes.write();
        if routes.remove(&pointer).is_some() {
            tracing::trace!(pointer = pointer.get(), "Removed all routes for pointer");
        }
    }

    /// Add a global handler that receives all pointer events.
    ///
    /// Global handlers are called before per-pointer handlers.
    /// Useful for logging, debugging, or modal event capture.
    pub fn add_global_handler(&self, handler: GlobalPointerHandler) {
        self.global_handlers.write().push(handler);
        tracing::trace!("Added global pointer handler");
    }

    /// Remove a global handler.
    ///
    /// Returns `true` if the handler was found and removed.
    pub fn remove_global_handler(&self, handler: &GlobalPointerHandler) -> bool {
        let mut handlers = self.global_handlers.write();
        let initial_len = handlers.len();
        handlers.retain(|h| !Arc::ptr_eq(h, handler));
        let removed = handlers.len() < initial_len;

        if removed {
            tracing::trace!("Removed global pointer handler");
        }

        removed
    }

    /// Clear all global handlers.
    pub fn clear_global_handlers(&self) {
        self.global_handlers.write().clear();
    }

    /// Route a pointer event to all registered handlers.
    ///
    /// Delivery order:
    /// 1. Global handlers (in registration order)
    /// 2. Per-pointer handlers (in registration order)
    ///
    /// All handlers receive the event regardless of what others do.
    ///
    /// # Reentrancy Safety
    ///
    /// Handlers can safely add or remove routes during dispatch:
    /// - Routes added during dispatch take effect on the next event
    /// - Routes removed during dispatch take effect immediately
    ///
    /// This is achieved by copying the route lists before dispatch and
    /// checking if routes still exist before calling each handler.
    pub fn route(&self, event: &PointerEvent) {
        let pointer = get_pointer_id(event);

        // Copy global handlers (for reentrancy safety)
        let global_handlers: Vec<GlobalPointerHandler> =
            self.global_handlers.read().iter().cloned().collect();

        // Dispatch to global handlers, checking they still exist
        for handler in global_handlers {
            // Check if still registered (may have been removed by previous handler)
            let still_registered = self
                .global_handlers
                .read()
                .iter()
                .any(|h| Arc::ptr_eq(h, &handler));

            if still_registered {
                handler(event);
            }
        }

        // Copy per-pointer handlers (for reentrancy safety)
        let pointer_handlers: Vec<PointerRouteHandler> = self
            .routes
            .read()
            .get(&pointer)
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default();

        // Dispatch to per-pointer handlers, checking they still exist
        for handler in pointer_handlers {
            // Check if still registered (may have been removed by previous handler)
            let still_registered = self
                .routes
                .read()
                .get(&pointer)
                .is_some_and(|handlers| handlers.iter().any(|h| Arc::ptr_eq(h, &handler)));

            if still_registered {
                handler(event);
            }
        }
    }

    /// Check if any handlers are registered for a pointer.
    pub fn has_routes(&self, pointer: PointerId) -> bool {
        self.routes
            .read()
            .get(&pointer)
            .is_some_and(|h| !h.is_empty())
    }

    /// Get the number of handlers registered for a pointer.
    pub fn route_count(&self, pointer: PointerId) -> usize {
        self.routes
            .read()
            .get(&pointer)
            .map(|h| h.len())
            .unwrap_or(0)
    }

    /// Get the total number of pointers with registered handlers.
    pub fn pointer_count(&self) -> usize {
        self.routes.read().len()
    }

    /// Clear all routes (for testing or cleanup).
    pub fn clear(&self) {
        self.routes.write().clear();
        self.global_handlers.write().clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{make_move_event, PointerType};
    use flui_types::geometry::Offset;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_event(device: i32, position: Offset<Pixels>) -> PointerEvent {
        // For testing, use make_move_event with the position
        // The device ID will be PRIMARY (0) by default
        let _ = device; // device ID is not directly settable in ui-events
        make_move_event(position, PointerType::Touch)
    }

    #[test]
    fn test_router_creation() {
        let router = PointerRouter::new();
        assert_eq!(router.pointer_count(), 0);
    }

    #[test]
    fn test_add_route() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(1);

        let handler = Arc::new(|_: &PointerEvent| {});
        router.add_route(pointer, handler);

        assert!(router.has_routes(pointer));
        assert_eq!(router.route_count(pointer), 1);
    }

    #[test]
    fn test_remove_route() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(1);

        let handler: PointerRouteHandler = Arc::new(|_: &PointerEvent| {});
        router.add_route(pointer, handler.clone());
        assert!(router.has_routes(pointer));

        let removed = router.remove_route(pointer, &handler);
        assert!(removed);
        assert!(!router.has_routes(pointer));
    }

    #[test]
    fn test_multiple_handlers() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(1);

        let handler1: PointerRouteHandler = Arc::new(|_: &PointerEvent| {});
        let handler2: PointerRouteHandler = Arc::new(|_: &PointerEvent| {});

        router.add_route(pointer, handler1.clone());
        router.add_route(pointer, handler2.clone());

        assert_eq!(router.route_count(pointer), 2);

        router.remove_route(pointer, &handler1);
        assert_eq!(router.route_count(pointer), 1);
    }

    #[test]
    fn test_route_event() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(0); // PRIMARY pointer

        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = call_count.clone();

        let handler = Arc::new(move |_: &PointerEvent| {
            count_clone.fetch_add(1, Ordering::Relaxed);
        });

        router.add_route(pointer, handler);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        assert_eq!(call_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_route_to_multiple_handlers() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(0); // PRIMARY pointer

        let call_count = Arc::new(AtomicUsize::new(0));

        let count1 = call_count.clone();
        let handler1 = Arc::new(move |_: &PointerEvent| {
            count1.fetch_add(1, Ordering::Relaxed);
        });

        let count2 = call_count.clone();
        let handler2 = Arc::new(move |_: &PointerEvent| {
            count2.fetch_add(1, Ordering::Relaxed);
        });

        router.add_route(pointer, handler1);
        router.add_route(pointer, handler2);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        // Both handlers should be called
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_global_handler() {
        let router = PointerRouter::new();

        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = call_count.clone();

        let handler = Arc::new(move |_: &PointerEvent| {
            count_clone.fetch_add(1, Ordering::Relaxed);
        });

        router.add_global_handler(handler);

        // Route event for any pointer
        let event = make_event(42, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        assert_eq!(call_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_global_before_per_pointer() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(0); // PRIMARY pointer

        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        let order1 = order.clone();
        let global_handler = Arc::new(move |_: &PointerEvent| {
            order1.lock().unwrap().push("global");
        });

        let order2 = order.clone();
        let pointer_handler = Arc::new(move |_: &PointerEvent| {
            order2.lock().unwrap().push("pointer");
        });

        router.add_global_handler(global_handler);
        router.add_route(pointer, pointer_handler);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        let calls = order.lock().unwrap();
        assert_eq!(*calls, vec!["global", "pointer"]);
    }

    #[test]
    fn test_remove_all_routes() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(1);

        let handler1 = Arc::new(|_: &PointerEvent| {});
        let handler2 = Arc::new(|_: &PointerEvent| {});

        router.add_route(pointer, handler1);
        router.add_route(pointer, handler2);
        assert_eq!(router.route_count(pointer), 2);

        router.remove_all_routes(pointer);
        assert!(!router.has_routes(pointer));
        assert_eq!(router.route_count(pointer), 0);
    }

    #[test]
    fn test_clear() {
        let router = PointerRouter::new();

        let handler = Arc::new(|_: &PointerEvent| {});
        router.add_route(PointerId::new(1), handler.clone());
        router.add_route(PointerId::new(2), handler);
        router.add_global_handler(Arc::new(|_: &PointerEvent| {}));

        router.clear();

        assert_eq!(router.pointer_count(), 0);
    }

    #[test]
    fn test_wrong_pointer_not_called() {
        let router = PointerRouter::new();
        let pointer1 = PointerId::new(1);
        let pointer2 = PointerId::new(2);

        let called = Arc::new(AtomicUsize::new(0));
        let called_clone = called.clone();

        let handler = Arc::new(move |_: &PointerEvent| {
            called_clone.fetch_add(1, Ordering::Relaxed);
        });

        // Register for pointer 1
        router.add_route(pointer1, handler);

        // Route event for pointer 0 (PRIMARY - default from make_event)
        let event = make_event(2, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        // Handler should NOT be called (registered for pointer1, event is for pointer0)
        assert_eq!(called.load(Ordering::Relaxed), 0);

        let _ = pointer2; // silence unused warning
    }

    #[test]
    fn test_global_singleton() {
        let router1 = PointerRouter::global();
        let router2 = PointerRouter::global();
        assert!(std::ptr::eq(router1, router2));
    }

    #[test]
    fn test_reentrancy_remove_self() {
        // Test that a handler can remove itself during dispatch
        let router = Arc::new(PointerRouter::new());
        let pointer = PointerId::new(0);

        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = call_count.clone();

        let router_clone = router.clone();
        let handler: PointerRouteHandler = Arc::new(move |_: &PointerEvent| {
            count_clone.fetch_add(1, Ordering::Relaxed);
            // Remove self during dispatch - this should work without deadlock
            // Note: We can't easily remove self here because we don't have the handler Arc
            // But we can remove all routes which exercises the same code path
            router_clone.remove_all_routes(PointerId::new(0));
        });

        router.add_route(pointer, handler);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event); // Should not deadlock

        assert_eq!(call_count.load(Ordering::Relaxed), 1);
        assert!(!router.has_routes(pointer));
    }

    #[test]
    fn test_reentrancy_add_handler() {
        // Test that a handler can add new handlers during dispatch
        let router = Arc::new(PointerRouter::new());
        let pointer = PointerId::new(0);

        let second_called = Arc::new(AtomicUsize::new(0));
        let second_called_clone = second_called.clone();

        let router_clone = router.clone();
        let handler1: PointerRouteHandler = Arc::new(move |_: &PointerEvent| {
            // Add a new handler during dispatch
            let called = second_called_clone.clone();
            let new_handler: PointerRouteHandler = Arc::new(move |_: &PointerEvent| {
                called.fetch_add(1, Ordering::Relaxed);
            });
            router_clone.add_route(PointerId::new(0), new_handler);
        });

        router.add_route(pointer, handler1);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event); // Should not deadlock

        // The new handler should NOT be called during this dispatch
        // (it takes effect on the next event)
        assert_eq!(second_called.load(Ordering::Relaxed), 0);

        // But should be called on the next event
        router.route(&event);
        assert_eq!(second_called.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_reentrancy_remove_other_handler() {
        // Test that a handler can remove another handler during dispatch
        let router = Arc::new(PointerRouter::new());
        let pointer = PointerId::new(0);

        let handler2_called = Arc::new(AtomicUsize::new(0));
        let handler2_called_clone = handler2_called.clone();

        let handler2: PointerRouteHandler = Arc::new(move |_: &PointerEvent| {
            handler2_called_clone.fetch_add(1, Ordering::Relaxed);
        });
        let handler2_for_remove = handler2.clone();

        let router_clone = router.clone();
        let handler1: PointerRouteHandler = Arc::new(move |_: &PointerEvent| {
            // Remove handler2 during dispatch
            router_clone.remove_route(PointerId::new(0), &handler2_for_remove);
        });

        // Add handler1 first, then handler2
        router.add_route(pointer, handler1);
        router.add_route(pointer, handler2);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event); // Should not deadlock

        // handler2 should NOT be called because handler1 removed it
        assert_eq!(handler2_called.load(Ordering::Relaxed), 0);
    }
}
