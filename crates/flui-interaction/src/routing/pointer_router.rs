//! Centralized pointer event routing
//!
//! PointerRouter is an owner-local registry for pointer event handlers.
//! Unlike hit testing (spatial routing), PointerRouter allows any handler
//! to receive events for a specific pointer regardless of position.
//!
//! This is useful for:
//! - Gesture recognizers that need to track pointers across the screen
//! - Drag gestures that continue even when pointer leaves the original target
//! - Modal dialogs that capture all pointer events
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/PointerRouter-class.html>
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::{PointerRouter, PointerId};
//! use std::rc::Rc;
//!
//! let router = PointerRouter::new();
//!
//! // Register a handler for a specific pointer
//! let handler = Rc::new(|event: &PointerEvent| {
//!     tracing::trace!(?event, "pointer event");
//! });
//! router.add_route(pointer_id, handler);
//!
//! // Route an event - all registered handlers receive it
//! router.route(&pointer_event);
//!
//! // Remove when done
//! router.remove_route(pointer_id, handler);
//! ```

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use smallvec::SmallVec;

use super::interaction_lane::RoutePanic;
use crate::{events::PointerEvent, ids::PointerId};

/// Handler for routed pointer events.
///
/// Unlike hit test handlers, these don't return propagation control -
/// all registered handlers always receive the event.
pub type PointerRouteHandler = Rc<dyn Fn(&PointerEvent)>;

/// Global handler that receives all pointer events.
pub type GlobalPointerHandler = Rc<dyn Fn(&PointerEvent)>;

/// Centralized pointer event router.
///
/// Allows handlers to register for pointer events by pointer ID,
/// regardless of spatial position. Events are delivered to all
/// registered handlers for that pointer.
///
/// # Thread affinity
///
/// `PointerRouter` is owner-local under ADR-0027. It deliberately stores
/// executable callbacks in `Rc`/`RefCell` rather than thread-safe shared
/// storage; render hit-test data stays on the separate `Send + Sync` data
/// plane.
///
/// # Example
///
/// ```rust,ignore
/// let router = PointerRouter::new();
///
/// // Gesture recognizer registers for pointer events
/// let recognizer_handler = Rc::new(|event| {
///     // Handle drag updates even when pointer leaves original target
/// });
/// router.add_route(pointer_id, recognizer_handler);
///
/// // Later, platform layer routes events
/// router.route(&pointer_event);
/// ```
pub struct PointerRouter {
    /// Routes per pointer ID
    routes: RefCell<HashMap<PointerId, Vec<PointerRouteHandler>>>,

    /// Global handlers (receive all events)
    global_handlers: RefCell<Vec<GlobalPointerHandler>>,
}

impl std::fmt::Debug for PointerRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let routes = self.routes.borrow();
        let global_count = self.global_handlers.borrow().len();
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
            routes: RefCell::new(HashMap::new()),
            global_handlers: RefCell::new(Vec::new()),
        }
    }

    /// Add a route handler for a specific pointer.
    ///
    /// The handler will receive all events for this pointer until removed.
    /// Multiple handlers can be registered for the same pointer.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let handler = Rc::new(|event: &PointerEvent| {
    ///     tracing::trace!(?event, "received pointer event");
    /// });
    /// router.add_route(pointer_id, handler);
    /// ```
    pub fn add_route(&self, pointer: PointerId, handler: PointerRouteHandler) {
        let mut routes = self.routes.borrow_mut();
        routes.entry(pointer).or_default().push(handler);

        tracing::trace!(?pointer, "Added pointer route");
    }

    /// Remove a route handler for a specific pointer.
    ///
    /// Uses `Rc` pointer equality to find and remove the handler.
    /// Returns `true` if the handler was found and removed.
    pub fn remove_route(&self, pointer: PointerId, handler: &PointerRouteHandler) -> bool {
        let mut routes = self.routes.borrow_mut();

        if let Some(handlers) = routes.get_mut(&pointer) {
            let initial_len = handlers.len();
            handlers.retain(|h| !Rc::ptr_eq(h, handler));

            let removed = handlers.len() < initial_len;

            // Clean up empty entries
            if handlers.is_empty() {
                routes.remove(&pointer);
            }

            if removed {
                tracing::trace!(?pointer, "Removed pointer route");
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
        let mut routes = self.routes.borrow_mut();
        if routes.remove(&pointer).is_some() {
            tracing::trace!(?pointer, "Removed all routes for pointer");
        }
    }

    /// Add a global handler that receives all pointer events.
    ///
    /// Global handlers are called after per-pointer handlers.
    /// Useful for logging, debugging, or modal event capture.
    pub fn add_global_handler(&self, handler: GlobalPointerHandler) {
        self.global_handlers.borrow_mut().push(handler);
        tracing::trace!("Added global pointer handler");
    }

    /// Remove a global handler.
    ///
    /// Returns `true` if the handler was found and removed.
    pub fn remove_global_handler(&self, handler: &GlobalPointerHandler) -> bool {
        let mut handlers = self.global_handlers.borrow_mut();
        let initial_len = handlers.len();
        handlers.retain(|h| !Rc::ptr_eq(h, handler));
        let removed = handlers.len() < initial_len;

        if removed {
            tracing::trace!("Removed global pointer handler");
        }

        removed
    }

    /// Clear all global handlers.
    pub fn clear_global_handlers(&self) {
        self.global_handlers.borrow_mut().clear();
    }

    /// Route a pointer event to all registered handlers.
    ///
    /// Delivery order:
    /// 1. Per-pointer handlers (in registration order)
    /// 2. Global handlers (in registration order)
    ///
    /// Every handler is isolated from unwinding in its peers. All callbacks
    /// still registered when their turn arrives receive the event, then the
    /// first captured panic resumes after the complete router snapshot has run.
    ///
    /// # Reentrancy Safety
    ///
    /// Dispatch snapshots the candidate callbacks before the first handler
    /// fires, so additions take effect on the next event. Before invoking each
    /// candidate it checks the live registry, so a callback removed before its
    /// turn is skipped in the current event. This matches Flutter
    /// [`pointer_router.dart::route`](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/gestures/pointer_router.dart)
    /// behavior.
    ///
    /// Dispatch order is **per-pointer handlers first, then global handlers**
    /// matching Flutter `pointer_router.dart:124` ordering. Per-pointer
    /// handlers run in their registration order (insertion order in the
    /// HashMap entry's Vec); global handlers fire afterward.
    pub fn route(&self, event: &PointerEvent) {
        if let Some(panic) = self.route_capturing_panics(event) {
            panic.resume();
        }
    }

    /// Route every callback while returning the first captured panic to the
    /// binding transaction that owns later hit-route/lifecycle cleanup.
    pub(crate) fn route_capturing_panics(&self, event: &PointerEvent) -> Option<RoutePanic> {
        let pointer = get_pointer_id(event);

        // Snapshot per-pointer handlers (clone the `Rc`s) so the borrow is
        // released before dispatch — a handler may re-enter the router. A
        // `SmallVec` keeps the common ≤4-handler case off the heap.
        let pointer_handlers: SmallVec<[PointerRouteHandler; 4]> = self
            .routes
            .borrow()
            .get(&pointer)
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default();

        // Snapshot global handlers before the first callback for the same
        // reentrancy contract as the per-pointer snapshot.
        let global_handlers: SmallVec<[GlobalPointerHandler; 4]> =
            self.global_handlers.borrow().iter().cloned().collect();

        let mut first_panic = None;

        // Per-pointer handlers first (Flutter ordering).
        for handler in pointer_handlers {
            if self.contains_route(pointer, &handler) {
                let delivered = RoutePanic::capture(|| handler(event));
                RoutePanic::preserve_first(
                    &mut first_panic,
                    delivered,
                    "per-pointer router callback",
                );
            }

            // Removing a callback during dispatch can leave the snapshot as
            // its final owner. Capture that destructor independently so the
            // rest of the already-snapshotted router transaction still runs.
            let snapshot_cleanup = RoutePanic::capture(|| drop(handler));
            RoutePanic::preserve_first(
                &mut first_panic,
                snapshot_cleanup,
                "per-pointer router snapshot cleanup",
            );
        }

        // Global handlers after per-pointer.
        for handler in global_handlers {
            if self.contains_global_handler(&handler) {
                let delivered = RoutePanic::capture(|| handler(event));
                RoutePanic::preserve_first(&mut first_panic, delivered, "global router callback");
            }

            let snapshot_cleanup = RoutePanic::capture(|| drop(handler));
            RoutePanic::preserve_first(
                &mut first_panic,
                snapshot_cleanup,
                "global router snapshot cleanup",
            );
        }

        first_panic
    }

    /// Whether a snapshotted per-pointer callback is still registered.
    fn contains_route(&self, pointer: PointerId, handler: &PointerRouteHandler) -> bool {
        self.routes.borrow().get(&pointer).is_some_and(|handlers| {
            handlers
                .iter()
                .any(|candidate| Rc::ptr_eq(candidate, handler))
        })
    }

    /// Whether a snapshotted global callback is still registered.
    fn contains_global_handler(&self, handler: &GlobalPointerHandler) -> bool {
        self.global_handlers
            .borrow()
            .iter()
            .any(|candidate| Rc::ptr_eq(candidate, handler))
    }

    /// Check if any handlers are registered for a pointer.
    pub fn has_routes(&self, pointer: PointerId) -> bool {
        self.routes
            .borrow()
            .get(&pointer)
            .is_some_and(|h| !h.is_empty())
    }

    /// Get the number of handlers registered for a pointer.
    pub fn route_count(&self, pointer: PointerId) -> usize {
        self.routes
            .borrow()
            .get(&pointer)
            .map_or(0, std::vec::Vec::len)
    }

    /// Get the total number of pointers with registered handlers.
    pub fn pointer_count(&self) -> usize {
        self.routes.borrow().len()
    }

    /// Clear all routes (for testing or cleanup).
    pub fn clear(&self) {
        self.routes.borrow_mut().clear();
        self.global_handlers.borrow_mut().clear();
    }
}

/// Helper to extract pointer ID from event.
#[inline]
fn get_pointer_id(event: &PointerEvent) -> PointerId {
    crate::events::extract_pointer_id(event)
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use flui_types::geometry::{Offset, Pixels};
    use std::cell::RefCell;

    use super::*;
    use crate::events::{PointerType, make_move_event};

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
        let pointer = PointerId::new(2).expect("nonzero pointer id");

        let handler = Rc::new(|_: &PointerEvent| {});
        router.add_route(pointer, handler);

        assert!(router.has_routes(pointer));
        assert_eq!(router.route_count(pointer), 1);
    }

    #[test]
    fn test_remove_route() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(2).expect("nonzero pointer id");

        let handler: PointerRouteHandler = Rc::new(|_: &PointerEvent| {});
        router.add_route(pointer, handler.clone());
        assert!(router.has_routes(pointer));

        let removed = router.remove_route(pointer, &handler);
        assert!(removed);
        assert!(!router.has_routes(pointer));
    }

    #[test]
    fn test_multiple_handlers() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(2).expect("nonzero pointer id");

        let handler1: PointerRouteHandler = Rc::new(|_: &PointerEvent| {});
        let handler2: PointerRouteHandler = Rc::new(|_: &PointerEvent| {});

        router.add_route(pointer, handler1.clone());
        router.add_route(pointer, handler2.clone());

        assert_eq!(router.route_count(pointer), 2);

        router.remove_route(pointer, &handler1);
        assert_eq!(router.route_count(pointer), 1);
    }

    #[test]
    fn test_route_event() {
        let router = PointerRouter::new();
        let pointer = PointerId::PRIMARY; // PRIMARY pointer

        let call_count = Rc::new(Cell::new(0));
        let count_clone = call_count.clone();

        let handler = Rc::new(move |_: &PointerEvent| {
            count_clone.set(count_clone.get() + 1);
        });

        router.add_route(pointer, handler);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        assert_eq!(call_count.get(), 1);
    }

    #[test]
    fn test_route_to_multiple_handlers() {
        let router = PointerRouter::new();
        let pointer = PointerId::PRIMARY; // PRIMARY pointer

        let call_count = Rc::new(Cell::new(0));

        let count1 = call_count.clone();
        let handler1 = Rc::new(move |_: &PointerEvent| {
            count1.set(count1.get() + 1);
        });

        let count2 = call_count.clone();
        let handler2 = Rc::new(move |_: &PointerEvent| {
            count2.set(count2.get() + 1);
        });

        router.add_route(pointer, handler1);
        router.add_route(pointer, handler2);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        // Both handlers should be called
        assert_eq!(call_count.get(), 2);
    }

    #[test]
    fn test_global_handler() {
        let router = PointerRouter::new();

        let call_count = Rc::new(Cell::new(0));
        let count_clone = call_count.clone();

        let handler = Rc::new(move |_: &PointerEvent| {
            count_clone.set(count_clone.get() + 1);
        });

        router.add_global_handler(handler);

        // Route event for any pointer
        let event = make_event(42, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        assert_eq!(call_count.get(), 1);
    }

    #[test]
    fn test_per_pointer_before_global() {
        // Flutter parity: pointer_router.dart:124 dispatches per-pointer
        // handlers first, then global handlers. This router matches that
        // ordering.
        let router = PointerRouter::new();
        let pointer = PointerId::PRIMARY; // PRIMARY pointer

        let order = Rc::new(RefCell::new(Vec::new()));

        let order1 = order.clone();
        let global_handler = Rc::new(move |_: &PointerEvent| {
            order1.borrow_mut().push("global");
        });

        let order2 = order.clone();
        let pointer_handler = Rc::new(move |_: &PointerEvent| {
            order2.borrow_mut().push("pointer");
        });

        router.add_global_handler(global_handler);
        router.add_route(pointer, pointer_handler);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        let calls = order.borrow();
        assert_eq!(*calls, vec!["pointer", "global"]);
    }

    #[test]
    fn test_remove_all_routes() {
        let router = PointerRouter::new();
        let pointer = PointerId::new(2).expect("nonzero pointer id");

        let handler1 = Rc::new(|_: &PointerEvent| {});
        let handler2 = Rc::new(|_: &PointerEvent| {});

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

        let handler = Rc::new(|_: &PointerEvent| {});
        router.add_route(
            PointerId::new(2).expect("nonzero pointer id"),
            handler.clone(),
        );
        router.add_route(PointerId::new(3).expect("nonzero pointer id"), handler);
        router.add_global_handler(Rc::new(|_: &PointerEvent| {}));

        router.clear();

        assert_eq!(router.pointer_count(), 0);
    }

    #[test]
    fn test_wrong_pointer_not_called() {
        let router = PointerRouter::new();
        let pointer1 = PointerId::new(2).expect("nonzero pointer id");
        let pointer2 = PointerId::new(3).expect("nonzero pointer id");

        let called = Rc::new(Cell::new(0));
        let called_clone = called.clone();

        let handler = Rc::new(move |_: &PointerEvent| {
            called_clone.set(called_clone.get() + 1);
        });

        // Register for pointer 1
        router.add_route(pointer1, handler);

        // Route event for pointer 0 (PRIMARY - default from make_event)
        let event = make_event(2, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        // Handler should NOT be called (registered for pointer1, event is for pointer0)
        assert_eq!(called.get(), 0);

        let _ = pointer2; // silence unused warning
    }

    #[test]
    fn test_reentrancy_remove_self() {
        // Test that a handler can remove itself during dispatch
        let router = Rc::new(PointerRouter::new());
        let pointer = PointerId::PRIMARY;

        let call_count = Rc::new(Cell::new(0));
        let count_clone = call_count.clone();

        let router_clone = router.clone();
        let handler: PointerRouteHandler = Rc::new(move |_: &PointerEvent| {
            count_clone.set(count_clone.get() + 1);
            // Remove self during dispatch - this should work without deadlock
            // Note: We can't easily remove self here because we don't have the handler Rc
            // But we can remove all routes which exercises the same code path
            router_clone.remove_all_routes(PointerId::PRIMARY);
        });

        router.add_route(pointer, handler);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event); // Should not deadlock

        assert_eq!(call_count.get(), 1);
        assert!(!router.has_routes(pointer));
    }

    #[test]
    fn test_reentrancy_add_handler() {
        // Test that a handler can add new handlers during dispatch
        let router = Rc::new(PointerRouter::new());
        let pointer = PointerId::PRIMARY;

        let second_called = Rc::new(Cell::new(0));
        let second_called_clone = second_called.clone();

        let router_clone = router.clone();
        let handler1: PointerRouteHandler = Rc::new(move |_: &PointerEvent| {
            // Add a new handler during dispatch
            let called = second_called_clone.clone();
            let new_handler: PointerRouteHandler = Rc::new(move |_: &PointerEvent| {
                called.set(called.get() + 1);
            });
            router_clone.add_route(PointerId::PRIMARY, new_handler);
        });

        router.add_route(pointer, handler1);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event); // Should not deadlock

        // The new handler should NOT be called during this dispatch
        // (it takes effect on the next event)
        assert_eq!(second_called.get(), 0);

        // But should be called on the next event
        router.route(&event);
        assert_eq!(second_called.get(), 1);
    }

    #[test]
    fn test_reentrancy_remove_other_handler() {
        // Test that a handler can remove another handler during dispatch
        let router = Rc::new(PointerRouter::new());
        let pointer = PointerId::PRIMARY;

        let handler2_called = Rc::new(Cell::new(0));
        let handler2_called_clone = handler2_called.clone();

        let handler2: PointerRouteHandler = Rc::new(move |_: &PointerEvent| {
            handler2_called_clone.set(handler2_called_clone.get() + 1);
        });
        let handler2_for_remove = handler2.clone();

        let router_clone = router.clone();
        let handler1: PointerRouteHandler = Rc::new(move |_: &PointerEvent| {
            // Remove handler2 during dispatch
            router_clone.remove_route(PointerId::PRIMARY, &handler2_for_remove);
        });

        // Add handler1 first, then handler2
        router.add_route(pointer, handler1);
        router.add_route(pointer, handler2);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event); // Should not deadlock

        // Flutter snapshots additions, but consults the live registration map
        // before each invocation. A handler removed before its turn is skipped
        // in this same dispatch.
        assert_eq!(handler2_called.get(), 0);

        // Second dispatch sees post-removal snapshot — handler2 not called.
        let event2 = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event2);
        assert_eq!(handler2_called.get(), 0);
    }

    #[test]
    fn removing_a_later_global_handler_skips_it_in_the_current_dispatch() {
        let router = Rc::new(PointerRouter::new());
        let later_called = Rc::new(Cell::new(0));
        let later_count = Rc::clone(&later_called);
        let later: GlobalPointerHandler = Rc::new(move |_| {
            later_count.set(later_count.get() + 1);
        });

        let router_for_first = Rc::clone(&router);
        let later_for_remove = Rc::clone(&later);
        let first: GlobalPointerHandler = Rc::new(move |_| {
            router_for_first.remove_global_handler(&later_for_remove);
        });
        router.add_global_handler(first);
        router.add_global_handler(later);

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        assert_eq!(later_called.get(), 0);
    }

    #[test]
    fn pointer_route_handler_accepts_owner_local_rc_state() {
        let router = PointerRouter::new();
        let pointer = PointerId::PRIMARY;
        let total = Rc::new(Cell::new(0));
        let captured = Rc::clone(&total);

        router.add_route(
            pointer,
            Rc::new(move |_: &PointerEvent| captured.set(captured.get() + 1)),
        );

        let event = make_event(0, Offset::new(Pixels(50.0), Pixels(50.0)));
        router.route(&event);

        assert_eq!(total.get(), 1);
    }
}
