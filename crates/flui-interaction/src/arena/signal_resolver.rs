//! Pointer signal resolver for conflict resolution
//!
//! The PointerSignalResolver manages conflicts between multiple handlers
//! for pointer signals (scroll, hover, etc.). Similar to GestureArena but
//! specifically for signal events.
//!
//! # Purpose
//!
//! When multiple widgets listen to the same signal (e.g., nested scroll
//! regions), the resolver determines which widget should actually receive the
//! signal.
//!
//! # Architecture
//!
//! ```text
//! Pointer Signal Event (scroll, hover, etc.)
//!     ↓
//! PointerSignalResolver
//!     ├─ Collect all interested handlers
//!     ├─ Apply resolution rules
//!     └─ Notify winner
//!         ↓
//! Handler receives signal
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::signal_resolver::PointerSignalResolver;
//!
//! let resolver = PointerSignalResolver::new();
//!
//! // Register handlers
//! resolver.register(pointer_id, handler1);
//! resolver.register(pointer_id, handler2);
//!
//! // Resolve conflict
//! resolver.resolve(pointer_id, signal_event);
//! ```

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{
    events::PointerEvent,
    ids::{HandlerId, PointerId},
};

/// Callback for handling pointer signals
pub type SignalCallback = Rc<dyn Fn(PointerEvent)>;

/// Priority level for signal handlers
///
/// Higher priority handlers win conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SignalPriority {
    /// Low priority (background handlers)
    Low = 0,
    /// Normal priority (default)
    #[default]
    Normal = 1,
    /// High priority (foreground handlers)
    High = 2,
    /// Critical priority (system handlers)
    Critical = 3,
}

/// A registered signal handler
struct SignalHandler {
    /// Unique ID for this handler
    id: HandlerId,
    /// Priority level
    priority: SignalPriority,
    /// Callback to invoke
    callback: SignalCallback,
}

/// Find the highest priority handler from a list.
///
/// If multiple handlers have the same priority, the last registered wins.
fn find_winner(handlers: &[SignalHandler]) -> Option<&SignalHandler> {
    handlers
        .iter()
        .max_by(|a, b| match a.priority.cmp(&b.priority) {
            std::cmp::Ordering::Equal => a.id.cmp(&b.id),
            other => other,
        })
}

/// Resolver for pointer signal conflicts
///
/// Manages multiple handlers for pointer signals and resolves conflicts
/// based on priority and registration order.
///
/// # Thread affinity
///
/// This type is owner-local under ADR-0027. It stores executable callbacks in
/// `Rc`/`RefCell`; cross-thread input should enter through typed data-plane
/// events before owner-thread dispatch.
#[derive(Clone)]
pub struct PointerSignalResolver {
    inner: Rc<RefCell<ResolverInner>>,
}

struct ResolverInner {
    /// Next handler ID to assign
    next_handler_id: u64,
    /// Handlers registered for each pointer
    handlers: HashMap<PointerId, Vec<SignalHandler>>,
}

// Manual impl: the registered handlers hold `dyn Fn` callbacks, which have no
// useful `Debug` representation.
impl std::fmt::Debug for PointerSignalResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerSignalResolver")
            .finish_non_exhaustive()
    }
}

impl PointerSignalResolver {
    /// Creates a new signal resolver
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(ResolverInner {
                next_handler_id: 1,
                handlers: HashMap::new(),
            })),
        }
    }

    /// Registers a signal handler for a pointer
    ///
    /// Returns a handler ID that can be used to unregister later.
    ///
    /// # Arguments
    ///
    /// * `pointer_id` - The pointer device to listen to
    /// * `priority` - Priority level (higher wins conflicts)
    /// * `callback` - Function to call when this handler wins
    pub fn register<F>(
        &self,
        pointer_id: PointerId,
        priority: SignalPriority,
        callback: F,
    ) -> HandlerId
    where
        F: Fn(PointerEvent) + 'static,
    {
        let mut inner = self.inner.borrow_mut();

        let next_handler_id = inner
            .next_handler_id
            .checked_add(1)
            .expect("BUG: pointer signal handler ID exhausted");
        let handler_id = HandlerId::new(inner.next_handler_id);
        inner.next_handler_id = next_handler_id;

        let handler = SignalHandler {
            id: handler_id,
            priority,
            callback: Rc::new(callback),
        };

        inner.handlers.entry(pointer_id).or_default().push(handler);

        handler_id
    }

    /// Unregisters a signal handler
    ///
    /// # Arguments
    ///
    /// * `pointer_id` - The pointer device
    /// * `handler_id` - The handler ID returned from `register()`
    pub fn unregister(&self, pointer_id: PointerId, handler_id: HandlerId) {
        let mut inner = self.inner.borrow_mut();

        if let Some(handlers) = inner.handlers.get_mut(&pointer_id) {
            handlers.retain(|h| h.id != handler_id);

            // Clean up empty vectors
            if handlers.is_empty() {
                inner.handlers.remove(&pointer_id);
            }
        }
    }

    /// Resolves a signal event
    ///
    /// Finds the highest priority handler and invokes it.
    /// If multiple handlers have the same priority, the last registered wins.
    ///
    /// # Arguments
    ///
    /// * `pointer_id` - The pointer device
    /// * `event` - The signal event to resolve
    pub fn resolve(&self, pointer_id: PointerId, event: PointerEvent) {
        let inner = self.inner.borrow();

        let Some(handlers) = inner.handlers.get(&pointer_id) else {
            return; // No handlers registered
        };

        if let Some(handler) = find_winner(handlers) {
            let callback = handler.callback.clone();
            // Release the borrow before calling callback.
            drop(inner);
            callback(event);
        }
    }

    /// Resolves and accepts a signal
    ///
    /// This is a convenience method that both resolves the conflict
    /// and marks the signal as accepted (preventing bubbling).
    ///
    /// Returns true if a handler was found and invoked.
    pub fn resolve_and_accept(&self, pointer_id: PointerId, event: PointerEvent) -> bool {
        let inner = self.inner.borrow();

        let Some(handlers) = inner.handlers.get(&pointer_id) else {
            return false;
        };

        if let Some(handler) = find_winner(handlers) {
            let callback = handler.callback.clone();
            drop(inner);
            callback(event);
            true
        } else {
            false
        }
    }

    /// Clears all handlers for a pointer
    pub fn clear(&self, pointer_id: PointerId) {
        let mut inner = self.inner.borrow_mut();
        inner.handlers.remove(&pointer_id);
    }

    /// Clears all handlers for all pointers
    pub fn clear_all(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.handlers.clear();
    }

    /// Returns the number of handlers registered for a pointer
    pub fn handler_count(&self, pointer_id: PointerId) -> usize {
        self.inner
            .borrow()
            .handlers
            .get(&pointer_id)
            .map_or(0, std::vec::Vec::len)
    }
}

impl Default for PointerSignalResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
    };

    use super::*;

    struct CaptureRetirement(Rc<Cell<u32>>);

    impl Drop for CaptureRetirement {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    // The terminal counter needs a private seam; exhausting it through public
    // registration would require more operations than a test can perform.
    #[test]
    fn signal_handler_exhaustion_refuses_permanently_without_losing_registration() {
        let resolver = PointerSignalResolver::new();
        let pointer = PointerId::PRIMARY;
        resolver.inner.borrow_mut().next_handler_id = u64::MAX - 1;
        let last = resolver.register(pointer, SignalPriority::Normal, |_| {});
        assert_eq!(last.get(), u64::MAX - 1);

        for _ in 0..2 {
            let dropped = Rc::new(Cell::new(0));
            let capture = CaptureRetirement(Rc::clone(&dropped));
            let failure = catch_unwind(AssertUnwindSafe(|| {
                resolver.register(pointer, SignalPriority::Normal, move |_| {
                    let _keep = &capture;
                });
            }))
            .expect_err("terminal registration must refuse instead of wrapping");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("BUG: pointer signal handler ID exhausted"),
            );
            assert_eq!(resolver.handler_count(pointer), 1);
            assert_eq!(
                dropped.get(),
                0,
                "refused callback ownership is retained after failure"
            );
        }
        resolver.unregister(pointer, last);
        assert_eq!(resolver.handler_count(pointer), 0);
    }
}
