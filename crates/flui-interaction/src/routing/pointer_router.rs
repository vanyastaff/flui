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
    closed: std::cell::Cell<bool>,
    close_mode: crate::__runtime::CloseTombstone,
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
            .finish_non_exhaustive()
    }
}

impl Default for PointerRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PointerRouter {
    fn drop(&mut self) {
        // Exclusive ownership lets teardown detach both registries without a
        // dynamic borrow. Retire the complete outgoing set only after both
        // states are empty, with one first-failure fence across them.
        let routes = std::mem::take(self.routes.get_mut());
        let global_handlers = std::mem::take(self.global_handlers.get_mut());
        Self::retire_handlers(routes.into_values().flatten().chain(global_handlers));
    }
}

impl PointerRouter {
    /// Create a new pointer router.
    pub fn new() -> Self {
        Self {
            closed: std::cell::Cell::new(false),
            close_mode: crate::__runtime::CloseTombstone::default(),
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
        if self.closed.get() {
            let mut failure = crate::__runtime::ClosePanic::for_rejection(self.close_mode.mode());
            failure.retire(crate::retain::Owned(handler));
            failure.finish();
            return;
        }
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
    /// The routes are detached before their captures are destroyed, so a
    /// destructor can reenter this router and register the next route. A capture
    /// destructor panic propagates after removal; the registry remains usable.
    /// Once retirement fails, remaining captures are retained. During an active
    /// unwind all removed captures are retained without running their destructors.
    pub fn remove_all_routes(&self, pointer: PointerId) {
        let removed = self.routes.borrow_mut().remove(&pointer);
        let had_routes = removed.is_some();
        // Callback captures are user code. Retire them after the registry's
        // borrow ends, including when they register another route for this ID.
        Self::retire_handlers(removed.into_iter().flatten());
        if had_routes && !std::thread::panicking() {
            tracing::trace!(?pointer, "Removed all routes for pointer");
        }
    }

    /// Add a global handler that receives all pointer events.
    ///
    /// Global handlers are called after per-pointer handlers.
    /// Useful for logging, debugging, or modal event capture.
    pub fn add_global_handler(&self, handler: GlobalPointerHandler) {
        if self.closed.get() {
            let mut failure = crate::__runtime::ClosePanic::for_rejection(self.close_mode.mode());
            failure.retire(crate::retain::Owned(handler));
            failure.finish();
            return;
        }
        self.global_handlers.borrow_mut().push(handler);
        tracing::trace!("Added global pointer handler");
    }

    pub(crate) fn close_with_mode(&self, mode: crate::__runtime::CloseMode) {
        let mut failure = crate::__runtime::ClosePanic::for_close(mode, self.close_mode.clone());
        self.closed.set(true);
        let routes = std::mem::take(&mut *self.routes.borrow_mut());
        let globals = std::mem::take(&mut *self.global_handlers.borrow_mut());
        for handler in routes.into_values().flatten().chain(globals) {
            failure.retire(crate::retain::Owned(handler));
        }
        failure.finish();
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
    ///
    /// Captures retire outside registry borrows, independently. The first
    /// destructor panic propagates; subsequent captures are retained. During
    /// an active unwind all removed captures are retained.
    pub fn clear_global_handlers(&self) {
        let removed = std::mem::take(&mut *self.global_handlers.borrow_mut());
        Self::retire_handlers(removed);
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
    /// turn is skipped in the current event.
    ///
    /// Dispatch order is **per-pointer handlers first, then global handlers**.
    /// Per-pointer handlers run in their registration order (insertion order in the
    /// HashMap entry's Vec); global handlers fire afterward.
    pub fn route(&self, event: &PointerEvent) {
        if self.closed.get() {
            return;
        }
        if let Some(panic) = self.route_capturing_panics(event) {
            panic.resume();
        }
    }

    /// Route every callback while returning the first captured panic to the
    /// binding transaction that owns later hit-route/lifecycle cleanup.
    pub(crate) fn route_capturing_panics(&self, event: &PointerEvent) -> Option<RoutePanic> {
        if self.closed.get() {
            return None;
        }
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

        // Per-pointer handlers first.
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
    ///
    /// Both registries are detached before any capture is destroyed. Retirement
    /// has the same first-failure and active-unwind policy as
    /// [`Self::clear_global_handlers`].
    pub fn clear(&self) {
        let routes = std::mem::take(&mut *self.routes.borrow_mut());
        let global_handlers = std::mem::take(&mut *self.global_handlers.borrow_mut());
        Self::retire_handlers(routes.into_values().flatten().chain(global_handlers));
    }

    /// Callback handles are independent framework-owned values. Retire them
    /// individually, without letting Vec drop glue combine competing failures.
    fn retire_handlers(handlers: impl IntoIterator<Item = PointerRouteHandler>) {
        let mut first_panic = None;
        for handler in handlers {
            if first_panic.is_some() || std::thread::panicking() {
                // This callback's opaque capture may contain several hostile
                // destructors. After failure, do not start another retirement;
                // a clone some other owner still holds is released normally.
                crate::retain::Retain::retain(handler);
            } else {
                let retirement = RoutePanic::capture(|| drop(handler));
                RoutePanic::preserve_first(
                    &mut first_panic,
                    retirement,
                    "pointer router callback retirement",
                );
            }
        }
        if let Some(panic) = first_panic {
            panic.resume();
        }
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

    use flui_foundation::geometry::Offset;

    use super::*;
    use crate::events::{PointerType, make_move_event};

    fn make_event(device: i32, position: Offset<f64>) -> PointerEvent {
        // For testing, use make_move_event with the position
        // The device ID will be PRIMARY (0) by default
        let _ = device; // device ID is not directly settable in ui-events
        make_move_event(position, PointerType::Touch)
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

        let event = make_event(0, Offset::new(50.0, 50.0));
        router.route(&event); // Should not deadlock

        assert_eq!(call_count.get(), 1);
        assert!(!router.has_routes(pointer));
    }
}
