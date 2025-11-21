//! Keyboard focus management
//!
//! FocusManager is a global singleton that tracks which UI element has keyboard focus.
//! Only one element can have focus at a time.

use parking_lot::RwLock;
use std::sync::Arc;

/// Unique identifier for a focusable UI element
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusNodeId(u64);

impl FocusNodeId {
    /// Create a new focus node ID
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the raw ID value
    pub fn get(&self) -> u64 {
        self.0
    }
}

/// Global focus manager (singleton)
///
/// Tracks which UI element currently has keyboard focus.
/// Only one element can have focus at a time.
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
    /// Currently focused element (if any)
    focused: Arc<RwLock<Option<FocusNodeId>>>,
}

impl FocusManager {
    /// Get the global focus manager instance
    ///
    /// This is a singleton - the same instance is returned every time.
    pub fn global() -> &'static FocusManager {
        static INSTANCE: std::sync::OnceLock<FocusManager> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(|| FocusManager {
            focused: Arc::new(RwLock::new(None)),
        })
    }

    /// Request focus for a node
    ///
    /// If another node had focus, it loses focus.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let text_field = FocusNodeId::new(1);
    /// FocusManager::global().request_focus(text_field);
    /// ```
    pub fn request_focus(&self, node_id: FocusNodeId) {
        let previous = *self.focused.read();

        *self.focused.write() = Some(node_id);

        if previous != Some(node_id) {
            tracing::debug!(
                "Focus changed: {:?} → {:?}",
                previous.map(|id| id.get()),
                node_id.get()
            );
        }
    }

    /// Get the currently focused node (if any)
    ///
    /// Returns `None` if no element has focus.
    pub fn focused(&self) -> Option<FocusNodeId> {
        *self.focused.read()
    }

    /// Check if a specific node has focus
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if FocusManager::global().has_focus(my_node) {
    ///     // Draw focus indicator
    /// }
    /// ```
    pub fn has_focus(&self, node_id: FocusNodeId) -> bool {
        *self.focused.read() == Some(node_id)
    }

    /// Clear focus (no element focused)
    ///
    /// Call this when:
    /// - User clicks on background
    /// - Window loses focus
    /// - Focused element is removed
    pub fn unfocus(&self) {
        if self.focused.read().is_some() {
            tracing::debug!("Focus cleared");
            *self.focused.write() = None;
        }
    }

    /// Transfer focus to the next focusable element
    ///
    /// This is called when user presses Tab.
    /// Implementation requires traversing the UI tree to find the next focusable element.
    ///
    /// TODO: This needs focus scope and traversal policy support.
    pub fn focus_next(&self) {
        tracing::warn!("focus_next() not yet implemented - needs focus scope support");
    }

    /// Transfer focus to the previous focusable element
    ///
    /// This is called when user presses Shift+Tab.
    ///
    /// TODO: This needs focus scope and traversal policy support.
    pub fn focus_previous(&self) {
        tracing::warn!("focus_previous() not yet implemented - needs focus scope support");
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
        let manager = FocusManager::global();
        let node1 = FocusNodeId::new(1);
        let node2 = FocusNodeId::new(2);

        // Clear any previous focus from other tests
        manager.unfocus();

        // Initially no focus
        assert_eq!(manager.focused(), None);

        // Request focus for node1
        manager.request_focus(node1);
        assert_eq!(manager.focused(), Some(node1));
        assert!(manager.has_focus(node1));
        assert!(!manager.has_focus(node2));

        // Request focus for node2 (node1 loses focus)
        manager.request_focus(node2);
        assert_eq!(manager.focused(), Some(node2));
        assert!(!manager.has_focus(node1));
        assert!(manager.has_focus(node2));
    }

    #[test]
    fn test_unfocus() {
        let manager = FocusManager::global();
        let node = FocusNodeId::new(42);

        // Give focus
        manager.request_focus(node);
        assert!(manager.has_focus(node));

        // Clear focus
        manager.unfocus();
        assert_eq!(manager.focused(), None);
        assert!(!manager.has_focus(node));
    }

    #[test]
    fn test_has_focus() {
        let manager = FocusManager::global();
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
}
