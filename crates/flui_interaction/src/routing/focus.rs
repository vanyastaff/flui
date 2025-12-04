//! Keyboard focus management
//!
//! FocusManager is a global singleton that tracks which UI element has keyboard focus.
//! Only one element can have focus at a time.
//!
//! # Type System Features
//!
//! - **Newtype pattern**: `FocusNodeId` uses `NonZeroU64` for niche optimization
//! - **Singleton pattern**: Global focus manager via `OnceLock`
//! - **parking_lot**: High-performance read-write locks
//! - **FocusScope integration**: Tab/Shift+Tab navigation via `FocusScopeManager`
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::{FocusManager, FocusNodeId};
//!
//! let my_node = FocusNodeId::new(42);
//!
//! // Request focus
//! FocusManager::global().request_focus(my_node);
//!
//! // Check focus
//! if FocusManager::global().has_focus(my_node) {
//!     println!("We have focus!");
//! }
//!
//! // Tab navigation (uses FocusScopeManager)
//! FocusManager::global().focus_next();
//!
//! // Release focus
//! FocusManager::global().unfocus();
//! ```

use crate::ids::FocusNodeId;
use flui_types::events::KeyEvent;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

// Re-export FocusNodeId for convenience

// ============================================================================
// FocusChangeCallback
// ============================================================================

/// Callback invoked when focus changes.
///
/// Receives the previous and new focus node IDs.
pub type FocusChangeCallback = Arc<dyn Fn(Option<FocusNodeId>, Option<FocusNodeId>) + Send + Sync>;

/// Callback for handling key events.
///
/// Returns `true` if the event was handled (stops propagation).
pub type KeyEventCallback = Arc<dyn Fn(&KeyEvent) -> bool + Send + Sync>;

// ============================================================================
// FocusManager
// ============================================================================

/// Global focus manager (singleton).
///
/// Tracks which UI element currently has keyboard focus.
/// Only one element can have focus at a time.
///
/// # Thread Safety
///
/// FocusManager uses `parking_lot::RwLock` for efficient concurrent access.
/// Reads (checking focus) are very fast and don't block each other.
///
/// # Niche Optimization
///
/// `FocusNodeId` uses `NonZeroU64`, so `Option<FocusNodeId>` is the same size
/// as `FocusNodeId` (8 bytes).
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::{FocusManager, FocusNodeId};
///
/// let my_node = FocusNodeId::new(42);
///
/// // Request focus
/// FocusManager::global().request_focus(my_node);
///
/// // Check focus
/// if FocusManager::global().has_focus(my_node) {
///     println!("We have focus!");
/// }
///
/// // Release focus
/// FocusManager::global().unfocus();
/// ```
pub struct FocusManager {
    /// Currently focused element (if any).
    focused: RwLock<Option<FocusNodeId>>,

    /// Listeners for focus changes.
    listeners: RwLock<Vec<FocusChangeCallback>>,

    /// Key event handlers registered per node.
    key_handlers: RwLock<HashMap<FocusNodeId, KeyEventCallback>>,

    /// Global key event handlers (called for all key events).
    global_key_handlers: RwLock<Vec<KeyEventCallback>>,
}

impl std::fmt::Debug for FocusManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusManager")
            .field("focused", &*self.focused.read())
            .field("listener_count", &self.listeners.read().len())
            .finish()
    }
}

impl Default for FocusManager {
    fn default() -> Self {
        Self {
            focused: RwLock::new(None),
            listeners: RwLock::new(Vec::new()),
            key_handlers: RwLock::new(HashMap::new()),
            global_key_handlers: RwLock::new(Vec::new()),
        }
    }
}

impl FocusManager {
    /// Get the global focus manager instance.
    ///
    /// This is a singleton - the same instance is returned every time.
    pub fn global() -> &'static FocusManager {
        static INSTANCE: std::sync::OnceLock<FocusManager> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(|| FocusManager {
            focused: RwLock::new(None),
            listeners: RwLock::new(Vec::new()),
            key_handlers: RwLock::new(HashMap::new()),
            global_key_handlers: RwLock::new(Vec::new()),
        })
    }

    /// Create a new focus manager (for testing).
    ///
    /// Normally you should use `global()` instead.
    #[cfg(test)]
    fn new() -> Self {
        Self {
            focused: RwLock::new(None),
            listeners: RwLock::new(Vec::new()),
            key_handlers: RwLock::new(HashMap::new()),
            global_key_handlers: RwLock::new(Vec::new()),
        }
    }

    /// Request focus for a node.
    ///
    /// If another node had focus, it loses focus.
    /// Focus change listeners are notified.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let text_field = FocusNodeId::new(1);
    /// FocusManager::global().request_focus(text_field);
    /// ```
    pub fn request_focus(&self, node_id: FocusNodeId) {
        let previous = *self.focused.read();

        if previous == Some(node_id) {
            return; // Already focused
        }

        *self.focused.write() = Some(node_id);

        tracing::debug!(
            previous = ?previous.map(|id| id.get()),
            new = node_id.get(),
            "Focus changed"
        );

        // Notify listeners
        self.notify_listeners(previous, Some(node_id));
    }

    /// Get the currently focused node (if any).
    ///
    /// Returns `None` if no element has focus.
    #[inline]
    pub fn focused(&self) -> Option<FocusNodeId> {
        *self.focused.read()
    }

    /// Check if a specific node has focus.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if FocusManager::global().has_focus(my_node) {
    ///     // Draw focus indicator
    /// }
    /// ```
    #[inline]
    pub fn has_focus(&self, node_id: FocusNodeId) -> bool {
        *self.focused.read() == Some(node_id)
    }

    /// Clear focus (no element focused).
    ///
    /// Call this when:
    /// - User clicks on background
    /// - Window loses focus
    /// - Focused element is removed
    pub fn unfocus(&self) {
        let previous = *self.focused.read();

        if previous.is_none() {
            return; // Already unfocused
        }

        *self.focused.write() = None;
        tracing::debug!("Focus cleared");

        // Notify listeners
        self.notify_listeners(previous, None);
    }

    /// Add a listener for focus changes.
    ///
    /// Returns a callback that can be stored and used to check the current focus.
    /// The listener is called whenever focus changes.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// FocusManager::global().add_listener(Arc::new(|prev, new| {
    ///     println!("Focus changed from {:?} to {:?}", prev, new);
    /// }));
    /// ```
    pub fn add_listener(&self, callback: FocusChangeCallback) {
        self.listeners.write().push(callback);
    }

    /// Remove all listeners.
    ///
    /// Useful for cleanup during shutdown.
    pub fn clear_listeners(&self) {
        self.listeners.write().clear();
    }

    /// Notify all listeners of a focus change.
    fn notify_listeners(&self, previous: Option<FocusNodeId>, new: Option<FocusNodeId>) {
        let listeners = self.listeners.read();
        for listener in listeners.iter() {
            listener(previous, new);
        }
    }

    /// Transfer focus to the next focusable element.
    ///
    /// This is called when user presses Tab.
    /// Implementation requires traversing the UI tree to find the next focusable element.
    ///
    /// # Note
    ///
    /// This requires focus scope and traversal policy support, which is not yet implemented.
    pub fn focus_next(&self) {
        tracing::warn!("focus_next() not yet implemented - needs focus scope support");
    }

    /// Transfer focus to the previous focusable element.
    ///
    /// This is called when user presses Shift+Tab.
    ///
    /// # Note
    ///
    /// This requires focus scope and traversal policy support, which is not yet implemented.
    pub fn focus_previous(&self) {
        tracing::warn!("focus_previous() not yet implemented - needs focus scope support");
    }

    /// Check if any element has focus.
    #[inline]
    pub fn is_focused(&self) -> bool {
        self.focused.read().is_some()
    }

    // ========================================================================
    // Key Event Handling
    // ========================================================================

    /// Register a key event handler for a specific node.
    ///
    /// The handler is called when the node has focus and receives a key event.
    /// Returns `true` from the handler to indicate the event was handled.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let text_field = FocusNodeId::new(1);
    /// FocusManager::global().register_key_handler(text_field, Arc::new(|event| {
    ///     println!("Key pressed: {:?}", event);
    ///     true // Event handled
    /// }));
    /// ```
    pub fn register_key_handler(&self, node_id: FocusNodeId, handler: KeyEventCallback) {
        self.key_handlers.write().insert(node_id, handler);
    }

    /// Unregister a key event handler for a node.
    pub fn unregister_key_handler(&self, node_id: FocusNodeId) {
        self.key_handlers.write().remove(&node_id);
    }

    /// Register a global key event handler.
    ///
    /// Global handlers are called for all key events, regardless of focus.
    /// They are called before the focused node's handler.
    /// Useful for keyboard shortcuts.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// FocusManager::global().add_global_key_handler(Arc::new(|event| {
    ///     if event.key == Key::Escape {
    ///         close_modal();
    ///         return true;
    ///     }
    ///     false // Not handled, continue to focused element
    /// }));
    /// ```
    pub fn add_global_key_handler(&self, handler: KeyEventCallback) {
        self.global_key_handlers.write().push(handler);
    }

    /// Clear all global key handlers.
    pub fn clear_global_key_handlers(&self) {
        self.global_key_handlers.write().clear();
    }

    /// Dispatch a key event to the appropriate handler(s).
    ///
    /// Event routing order:
    /// 1. Global handlers (in order added) - stop if any returns `true`
    /// 2. Focused node's handler (if any)
    ///
    /// Returns `true` if the event was handled.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let handled = FocusManager::global().dispatch_key_event(&key_event);
    /// if !handled {
    ///     // Default handling
    /// }
    /// ```
    pub fn dispatch_key_event(&self, event: &KeyEvent) -> bool {
        // First, try global handlers
        {
            let global_handlers = self.global_key_handlers.read();
            for handler in global_handlers.iter() {
                if handler(event) {
                    tracing::trace!("Key event handled by global handler");
                    return true;
                }
            }
        }

        // Then, try focused node's handler
        let focused = *self.focused.read();
        if let Some(node_id) = focused {
            let handlers = self.key_handlers.read();
            if let Some(handler) = handlers.get(&node_id) {
                if handler(event) {
                    tracing::trace!(node = node_id.get(), "Key event handled by focused node");
                    return true;
                }
            }
        }

        tracing::trace!("Key event not handled");
        false
    }

    /// Check if a node has a registered key handler.
    pub fn has_key_handler(&self, node_id: FocusNodeId) -> bool {
        self.key_handlers.read().contains_key(&node_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_manager_singleton() {
        let manager1 = FocusManager::global();
        let manager2 = FocusManager::global();

        // Should be the same instance
        assert!(std::ptr::eq(manager1, manager2));
    }

    #[test]
    fn test_request_focus() {
        let manager = FocusManager::new();
        let node1 = FocusNodeId::new(1);
        let node2 = FocusNodeId::new(2);

        // Initially no focus
        assert_eq!(manager.focused(), None);
        assert!(!manager.is_focused());

        // Request focus for node1
        manager.request_focus(node1);
        assert_eq!(manager.focused(), Some(node1));
        assert!(manager.has_focus(node1));
        assert!(!manager.has_focus(node2));
        assert!(manager.is_focused());

        // Request focus for node2 (node1 loses focus)
        manager.request_focus(node2);
        assert_eq!(manager.focused(), Some(node2));
        assert!(!manager.has_focus(node1));
        assert!(manager.has_focus(node2));
    }

    #[test]
    fn test_unfocus() {
        let manager = FocusManager::new();
        let node = FocusNodeId::new(42);

        // Give focus
        manager.request_focus(node);
        assert!(manager.has_focus(node));

        // Clear focus
        manager.unfocus();
        assert_eq!(manager.focused(), None);
        assert!(!manager.has_focus(node));
        assert!(!manager.is_focused());
    }

    #[test]
    fn test_has_focus() {
        let manager = FocusManager::new();
        let node1 = FocusNodeId::new(1);
        let node2 = FocusNodeId::new(2);

        manager.request_focus(node1);

        assert!(manager.has_focus(node1));
        assert!(!manager.has_focus(node2));
    }

    #[test]
    fn test_focus_node_id() {
        let id1 = FocusNodeId::new(123);
        let id2 = FocusNodeId::new(123);
        let id3 = FocusNodeId::new(456);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.get(), 123);
    }

    #[test]
    fn test_focus_node_id_niche_optimization() {
        // Option<FocusNodeId> should be same size as FocusNodeId
        assert_eq!(
            std::mem::size_of::<Option<FocusNodeId>>(),
            std::mem::size_of::<FocusNodeId>()
        );
    }

    #[test]
    fn test_focus_listener() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let manager = FocusManager::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        manager.add_listener(Arc::new(move |_prev, _new| {
            called_clone.store(true, Ordering::Relaxed);
        }));

        let node = FocusNodeId::new(1);
        manager.request_focus(node);

        assert!(called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_request_same_focus_no_change() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let manager = FocusManager::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = call_count.clone();

        manager.add_listener(Arc::new(move |_prev, _new| {
            count_clone.fetch_add(1, Ordering::Relaxed);
        }));

        let node = FocusNodeId::new(1);

        // First request triggers listener
        manager.request_focus(node);
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        // Same node again - should not trigger
        manager.request_focus(node);
        assert_eq!(call_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_clear_listeners() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let manager = FocusManager::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        manager.add_listener(Arc::new(move |_prev, _new| {
            called_clone.store(true, Ordering::Relaxed);
        }));

        manager.clear_listeners();

        let node = FocusNodeId::new(1);
        manager.request_focus(node);

        // Listener should not have been called
        assert!(!called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_register_key_handler() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let manager = FocusManager::new();
        let node = FocusNodeId::new(1);

        assert!(!manager.has_key_handler(node));

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        manager.register_key_handler(
            node,
            Arc::new(move |_event| {
                called_clone.store(true, Ordering::Relaxed);
                true
            }),
        );

        assert!(manager.has_key_handler(node));

        // Unregister
        manager.unregister_key_handler(node);
        assert!(!manager.has_key_handler(node));
    }

    #[test]
    fn test_dispatch_key_event_to_focused() {
        use crate::testing::input::KeyEventBuilder;
        use flui_types::events::PhysicalKey;
        use std::sync::atomic::{AtomicBool, Ordering};

        let manager = FocusManager::new();
        let node = FocusNodeId::new(1);

        let handled = Arc::new(AtomicBool::new(false));
        let handled_clone = handled.clone();

        manager.register_key_handler(
            node,
            Arc::new(move |_event| {
                handled_clone.store(true, Ordering::Relaxed);
                true
            }),
        );

        // Focus the node
        manager.request_focus(node);

        // Create a key event
        let event = KeyEventBuilder::new(PhysicalKey::KeyA).build();

        // Dispatch - should be handled
        let result = manager.dispatch_key_event(&event);
        assert!(result);
        assert!(handled.load(Ordering::Relaxed));
    }

    #[test]
    fn test_dispatch_key_event_no_focus() {
        use crate::testing::input::KeyEventBuilder;
        use flui_types::events::PhysicalKey;
        use std::sync::atomic::{AtomicBool, Ordering};

        let manager = FocusManager::new();
        let node = FocusNodeId::new(1);

        let handled = Arc::new(AtomicBool::new(false));
        let handled_clone = handled.clone();

        manager.register_key_handler(
            node,
            Arc::new(move |_event| {
                handled_clone.store(true, Ordering::Relaxed);
                true
            }),
        );

        // Don't focus the node
        let event = KeyEventBuilder::new(PhysicalKey::KeyA).build();

        // Dispatch - should NOT be handled (no focus)
        let result = manager.dispatch_key_event(&event);
        assert!(!result);
        assert!(!handled.load(Ordering::Relaxed));
    }

    #[test]
    fn test_global_key_handler() {
        use crate::testing::input::KeyEventBuilder;
        use flui_types::events::PhysicalKey;
        use std::sync::atomic::{AtomicBool, Ordering};

        let manager = FocusManager::new();

        let global_handled = Arc::new(AtomicBool::new(false));
        let global_clone = global_handled.clone();

        manager.add_global_key_handler(Arc::new(move |_event| {
            global_clone.store(true, Ordering::Relaxed);
            true
        }));

        let event = KeyEventBuilder::new(PhysicalKey::Escape).build();

        // Global handler should be called even without focus
        let result = manager.dispatch_key_event(&event);
        assert!(result);
        assert!(global_handled.load(Ordering::Relaxed));
    }

    #[test]
    fn test_global_handler_priority() {
        use crate::testing::input::KeyEventBuilder;
        use flui_types::events::PhysicalKey;
        use std::sync::atomic::{AtomicBool, Ordering};

        let manager = FocusManager::new();
        let node = FocusNodeId::new(1);

        let global_called = Arc::new(AtomicBool::new(false));
        let global_clone = global_called.clone();
        let node_called = Arc::new(AtomicBool::new(false));
        let node_clone = node_called.clone();

        // Global handler that handles the event
        manager.add_global_key_handler(Arc::new(move |_event| {
            global_clone.store(true, Ordering::Relaxed);
            true // Handled - stop propagation
        }));

        // Node handler
        manager.register_key_handler(
            node,
            Arc::new(move |_event| {
                node_clone.store(true, Ordering::Relaxed);
                true
            }),
        );

        manager.request_focus(node);

        let event = KeyEventBuilder::new(PhysicalKey::Enter).build();
        manager.dispatch_key_event(&event);

        // Global should be called first and handle
        assert!(global_called.load(Ordering::Relaxed));
        // Node handler should NOT be called (global handled it)
        assert!(!node_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_global_handler_passthrough() {
        use crate::testing::input::KeyEventBuilder;
        use flui_types::events::PhysicalKey;
        use std::sync::atomic::{AtomicBool, Ordering};

        let manager = FocusManager::new();
        let node = FocusNodeId::new(1);

        let global_called = Arc::new(AtomicBool::new(false));
        let global_clone = global_called.clone();
        let node_called = Arc::new(AtomicBool::new(false));
        let node_clone = node_called.clone();

        // Global handler that does NOT handle the event
        manager.add_global_key_handler(Arc::new(move |_event| {
            global_clone.store(true, Ordering::Relaxed);
            false // Not handled - continue to focused node
        }));

        // Node handler
        manager.register_key_handler(
            node,
            Arc::new(move |_event| {
                node_clone.store(true, Ordering::Relaxed);
                true
            }),
        );

        manager.request_focus(node);

        let event = KeyEventBuilder::new(PhysicalKey::KeyX).build();
        manager.dispatch_key_event(&event);

        // Both should be called
        assert!(global_called.load(Ordering::Relaxed));
        assert!(node_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_clear_global_key_handlers() {
        use crate::testing::input::KeyEventBuilder;
        use flui_types::events::PhysicalKey;
        use std::sync::atomic::{AtomicBool, Ordering};

        let manager = FocusManager::new();

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        manager.add_global_key_handler(Arc::new(move |_event| {
            called_clone.store(true, Ordering::Relaxed);
            true
        }));

        manager.clear_global_key_handlers();

        let event = KeyEventBuilder::new(PhysicalKey::Tab).build();
        manager.dispatch_key_event(&event);

        // Handler should not be called (was cleared)
        assert!(!called.load(Ordering::Relaxed));
    }
}
