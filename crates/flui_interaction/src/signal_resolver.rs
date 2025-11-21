//! Pointer signal resolver for conflict resolution
//!
//! The PointerSignalResolver manages conflicts between multiple handlers
//! for pointer signals (scroll, hover, etc.). Similar to GestureArena but
//! specifically for signal events.
//!
//! # Purpose
//!
//! When multiple widgets listen to the same signal (e.g., nested scroll regions),
//! the resolver determines which widget should actually receive the signal.
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

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use flui_types::events::PointerEvent;

/// Unique identifier for a pointer device
pub type PointerId = i32;

/// Unique identifier for a signal handler
pub type HandlerId = u64;

/// Callback for handling pointer signals
pub type SignalCallback = Arc<dyn Fn(PointerEvent) + Send + Sync>;

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

/// Resolver for pointer signal conflicts
///
/// Manages multiple handlers for pointer signals and resolves conflicts
/// based on priority and registration order.
///
/// # Thread Safety
///
/// This type is thread-safe using Arc<Mutex<_>> internally.
#[derive(Clone)]
pub struct PointerSignalResolver {
    inner: Arc<Mutex<ResolverInner>>,
}

struct ResolverInner {
    /// Next handler ID to assign
    next_handler_id: HandlerId,
    /// Handlers registered for each pointer
    handlers: HashMap<PointerId, Vec<SignalHandler>>,
}

impl PointerSignalResolver {
    /// Creates a new signal resolver
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ResolverInner {
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
        F: Fn(PointerEvent) + Send + Sync + 'static,
    {
        let mut inner = self.inner.lock();

        let handler_id = inner.next_handler_id;
        inner.next_handler_id += 1;

        let handler = SignalHandler {
            id: handler_id,
            priority,
            callback: Arc::new(callback),
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
        let mut inner = self.inner.lock();

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
        let inner = self.inner.lock();

        let Some(handlers) = inner.handlers.get(&pointer_id) else {
            return; // No handlers registered
        };

        // Find highest priority handler
        let winner = handlers.iter().max_by(|a, b| {
            // First compare by priority
            match a.priority.cmp(&b.priority) {
                std::cmp::Ordering::Equal => {
                    // If same priority, later registration wins
                    a.id.cmp(&b.id)
                }
                other => other,
            }
        });

        // Invoke winner's callback
        if let Some(handler) = winner {
            let callback = handler.callback.clone();
            // Release lock before calling callback
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
        let inner = self.inner.lock();

        let Some(handlers) = inner.handlers.get(&pointer_id) else {
            return false;
        };

        let winner = handlers
            .iter()
            .max_by(|a, b| match a.priority.cmp(&b.priority) {
                std::cmp::Ordering::Equal => a.id.cmp(&b.id),
                other => other,
            });

        if let Some(handler) = winner {
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
        let mut inner = self.inner.lock();
        inner.handlers.remove(&pointer_id);
    }

    /// Clears all handlers for all pointers
    pub fn clear_all(&self) {
        let mut inner = self.inner.lock();
        inner.handlers.clear();
    }

    /// Returns the number of handlers registered for a pointer
    pub fn handler_count(&self, pointer_id: PointerId) -> usize {
        self.inner
            .lock()
            .handlers
            .get(&pointer_id)
            .map(|h| h.len())
            .unwrap_or(0)
    }
}

impl Default for PointerSignalResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::Offset;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn test_resolver_creation() {
        let resolver = PointerSignalResolver::new();
        assert_eq!(resolver.handler_count(0), 0);
    }

    #[test]
    fn test_register_handler() {
        let resolver = PointerSignalResolver::new();

        let handler_id = resolver.register(0, SignalPriority::Normal, |_| {});

        assert_eq!(resolver.handler_count(0), 1);
        assert!(handler_id > 0);
    }

    #[test]
    fn test_unregister_handler() {
        let resolver = PointerSignalResolver::new();

        let handler_id = resolver.register(0, SignalPriority::Normal, |_| {});
        assert_eq!(resolver.handler_count(0), 1);

        resolver.unregister(0, handler_id);
        assert_eq!(resolver.handler_count(0), 0);
    }

    #[test]
    fn test_resolve_single_handler() {
        let resolver = PointerSignalResolver::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        resolver.register(0, SignalPriority::Normal, move |_| {
            called_clone.store(true, Ordering::Relaxed);
        });

        let event = PointerEvent::Scroll {
            device: 0,
            position: Offset::ZERO,
            scroll_delta: Offset::new(0.0, 10.0),
        };

        resolver.resolve(0, event);

        assert!(called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_priority_resolution() {
        let resolver = PointerSignalResolver::new();
        let low_called = Arc::new(AtomicBool::new(false));
        let high_called = Arc::new(AtomicBool::new(false));

        let low_clone = low_called.clone();
        let high_clone = high_called.clone();

        resolver.register(0, SignalPriority::Low, move |_| {
            low_clone.store(true, Ordering::Relaxed);
        });

        resolver.register(0, SignalPriority::High, move |_| {
            high_clone.store(true, Ordering::Relaxed);
        });

        let event = PointerEvent::Scroll {
            device: 0,
            position: Offset::ZERO,
            scroll_delta: Offset::new(0.0, 10.0),
        };

        resolver.resolve(0, event);

        assert!(!low_called.load(Ordering::Relaxed));
        assert!(high_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_same_priority_last_wins() {
        let resolver = PointerSignalResolver::new();
        let first_called = Arc::new(AtomicUsize::new(0));
        let second_called = Arc::new(AtomicUsize::new(0));

        let first_clone = first_called.clone();
        let second_clone = second_called.clone();

        resolver.register(0, SignalPriority::Normal, move |_| {
            first_clone.fetch_add(1, Ordering::Relaxed);
        });

        resolver.register(0, SignalPriority::Normal, move |_| {
            second_clone.fetch_add(1, Ordering::Relaxed);
        });

        let event = PointerEvent::Scroll {
            device: 0,
            position: Offset::ZERO,
            scroll_delta: Offset::new(0.0, 10.0),
        };

        resolver.resolve(0, event);

        // Last registered (second) should win
        assert_eq!(first_called.load(Ordering::Relaxed), 0);
        assert_eq!(second_called.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_clear() {
        let resolver = PointerSignalResolver::new();

        resolver.register(0, SignalPriority::Normal, |_| {});
        resolver.register(0, SignalPriority::Normal, |_| {});

        assert_eq!(resolver.handler_count(0), 2);

        resolver.clear(0);

        assert_eq!(resolver.handler_count(0), 0);
    }

    #[test]
    fn test_clear_all() {
        let resolver = PointerSignalResolver::new();

        resolver.register(0, SignalPriority::Normal, |_| {});
        resolver.register(1, SignalPriority::Normal, |_| {});

        resolver.clear_all();

        assert_eq!(resolver.handler_count(0), 0);
        assert_eq!(resolver.handler_count(1), 0);
    }

    #[test]
    fn test_resolve_and_accept() {
        let resolver = PointerSignalResolver::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        resolver.register(0, SignalPriority::Normal, move |_| {
            called_clone.store(true, Ordering::Relaxed);
        });

        let event = PointerEvent::Scroll {
            device: 0,
            position: Offset::ZERO,
            scroll_delta: Offset::new(0.0, 10.0),
        };

        let accepted = resolver.resolve_and_accept(0, event);

        assert!(accepted);
        assert!(called.load(Ordering::Relaxed));
    }
}
