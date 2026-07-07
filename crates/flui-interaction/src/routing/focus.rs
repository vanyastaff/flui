//! Keyboard focus management
//!
//! [`FocusManager`] is a global singleton that fronts the entire focus
//! tree machinery — it owns the [`FocusScopeNode`] root, tracks the
//! primary-focused node, notifies focus-change listeners, and routes
//! key events through the registered handlers.
//!
//! Audit Finding I-4 closure: prior dual structure (`FocusManager`
//! singleton + private `FocusManagerInner` `Arc<inner>` co-existing with
//! independent `primary_focus` + listener state) collapsed into a single
//! singleton owning every focus invariant. [`FocusNode`] / [`FocusScopeNode`]
//! reach the manager via [`FocusManager::global`] instead of holding a
//! `Weak<FocusManagerInner>`.
//!
//! # Type System Features
//!
//! - **Newtype pattern**: [`FocusNodeId`] uses `NonZeroU64` for niche
//!   optimization (so `Option<FocusNodeId>` is the same 8 bytes).
//! - **Singleton pattern**: Global focus manager via `OnceLock`.
//! - **parking_lot**: High-performance read-write locks.
//! - **TOCTOU-safe**: Primary-focus updates take a single write lock
//!   so concurrent `request_focus` callers cannot interleave a stale
//!   read with a competing write.
//!
//! # Flutter parity
//!
//! Mirrors [`widgets/focus_manager.dart`](https://api.flutter.dev/flutter/widgets/FocusManager-class.html)
//! `FocusManager` — singular `_primaryFocus` + `rootScope` + listener
//! `ChangeNotifier` semantics. FLUI's singleton replaces Flutter's
//! `WidgetsBinding.focusManager` accessor.
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
//! // Tab navigation (uses the root scope's traversal policy)
//! FocusManager::global().focus_next();
//!
//! // Release focus
//! FocusManager::global().unfocus();
//! ```

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;

use crate::{
    events::KeyEvent,
    ids::FocusNodeId,
    routing::focus_scope::{FocusNode, FocusScopeNode},
};

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
/// Tracks which UI element currently has keyboard focus, owns the root
/// [`FocusScopeNode`] of the focus tree, dispatches key events through
/// per-node + global handlers, and notifies registered listeners on
/// focus changes. Only one element can have focus at a time.
///
/// # Singleton ownership
///
/// `FocusManager::global()` returns a `&'static FocusManager` initialized
/// once via [`OnceLock`](std::sync::OnceLock). On first access, [`Default`] eagerly creates the
/// root [`FocusScopeNode`] so consumers can always reach
/// [`FocusManager::root_scope`] without re-initialization.
///
/// # Thread Safety
///
/// `FocusManager` uses `parking_lot::RwLock` for efficient concurrent
/// reads. Primary-focus mutations take a single write lock to avoid
/// read-then-write TOCTOU races against competing `request_focus`
/// callers.
///
/// # Niche Optimization
///
/// `FocusNodeId` uses `NonZeroU64`, so `Option<FocusNodeId>` is the same
/// size as `FocusNodeId` (8 bytes).
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
    /// Root scope of the focus tree.
    ///
    /// Owned directly by the singleton (Flutter parity:
    /// `FocusManager.rootScope`). Constructed eagerly in [`Default`] so
    /// the root scope is always present — no Option dance, no
    /// lazy-construction race.
    root_scope: Arc<FocusScopeNode>,

    /// Currently focused element (if any) — Flutter's `_primaryFocus`.
    primary_focus: RwLock<Option<FocusNodeId>>,

    /// Listeners for focus changes.
    listeners: RwLock<Vec<FocusChangeCallback>>,

    /// Key event handlers registered per node.
    key_handlers: RwLock<HashMap<FocusNodeId, KeyEventCallback>>,

    /// Global key event handlers (called for all key events).
    global_key_handlers: RwLock<Vec<KeyEventCallback>>,

    /// Override scope used for Tab navigation traversal. When `None`
    /// (the default), [`FocusManager::focus_next`] / [`FocusManager::focus_previous`] use
    /// [`FocusManager::root_scope`]. App code can set this to a sub-scope to scope
    /// traversal (matches Flutter modal-route scope semantics).
    active_scope: RwLock<Option<Arc<FocusScopeNode>>>,
}

impl std::fmt::Debug for FocusManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusManager")
            .field("primary_focus", &*self.primary_focus.read())
            .field("root_scope_id", &self.root_scope.id())
            .field("listener_count", &self.listeners.read().len())
            .finish_non_exhaustive()
    }
}

impl Default for FocusManager {
    fn default() -> Self {
        let root_scope = FocusScopeNode::with_debug_label("Root Focus Scope");
        // Root scope is attached by definition — it has no parent
        // because it IS the tree root. Mark attached so child-attach
        // recursion treats it as live (audit parity with Flutter's
        // root-scope-always-attached invariant).
        FocusNode::mark_root_attached(root_scope.as_focus_node());
        Self {
            root_scope,
            primary_focus: RwLock::new(None),
            listeners: RwLock::new(Vec::new()),
            key_handlers: RwLock::new(HashMap::new()),
            global_key_handlers: RwLock::new(Vec::new()),
            active_scope: RwLock::new(None),
        }
    }
}

impl FocusManager {
    /// Get the global focus manager instance.
    ///
    /// This is a singleton — the same instance is returned every time.
    /// On first access the root [`FocusScopeNode`] is constructed.
    pub fn global() -> &'static FocusManager {
        static INSTANCE: std::sync::OnceLock<FocusManager> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(FocusManager::default)
    }

    /// Returns the root focus scope.
    ///
    /// Flutter parity: `FocusManager.rootScope`. Always present —
    /// constructed eagerly in [`Default`]. Use this as the parent
    /// when attaching focus nodes to the tree.
    #[inline]
    pub fn root_scope(&self) -> &Arc<FocusScopeNode> {
        &self.root_scope
    }

    /// Override the active scope for Tab navigation. Pass `None` to
    /// fall back to the [`Self::root_scope`].
    ///
    /// Useful for modal dialogs that need traversal scoped to dialog
    /// descendants. Flutter equivalent: pushing a scope onto the
    /// modal-route history.
    pub fn set_active_scope(&self, scope: Option<Arc<FocusScopeNode>>) {
        *self.active_scope.write() = scope;
    }

    /// Returns the active focus scope used for traversal. Defaults to
    /// [`Self::root_scope`] when no override is set.
    pub fn active_scope(&self) -> Arc<FocusScopeNode> {
        self.active_scope
            .read()
            .clone()
            .unwrap_or_else(|| self.root_scope.clone())
    }

    /// Create a new focus manager (for testing).
    ///
    /// Normally you should use `global()` instead.
    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self::default()
    }

    /// Request focus for a node. If another node had focus, it loses
    /// focus. Focus-change listeners are notified.
    ///
    /// Uses a single-write-lock TOCTOU-safe update so concurrent
    /// callers cannot interleave a stale read with a competing write
    /// — earlier dual-state design read with a separate read lock
    /// before writing, allowing races.
    ///
    /// Records the new focus in the node's enclosing scope's history
    /// (Flutter parity: `FocusScopeNode._focusedChild` history).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let text_field = FocusNodeId::new(1);
    /// FocusManager::global().request_focus(text_field);
    /// ```
    pub fn request_focus(&self, node_id: FocusNodeId) {
        self.set_primary_focus(Some(node_id));
    }

    /// Internal: single-write-lock primary-focus update + scope
    /// history record + listener notification. Carries the
    /// TOCTOU-safe pattern migrated from the prior
    /// `FocusManagerInner::set_primary_focus`.
    fn set_primary_focus(&self, node_id: Option<FocusNodeId>) {
        // Single write lock: atomically read previous and write new
        // (avoids TOCTOU race against concurrent request_focus calls).
        let previous = {
            let mut focus = self.primary_focus.write();
            let previous = *focus;
            if previous == node_id {
                return;
            }
            *focus = node_id;
            previous
        };

        tracing::trace!(
            previous = ?previous.map(super::super::ids::FocusNodeId::get),
            new = ?node_id.map(super::super::ids::FocusNodeId::get),
            "Focus changed"
        );

        // Record in enclosing scope's history when focusing a node
        // (mirrors Flutter `_setAsFocusedChildForScope`).
        if let Some(id) = node_id
            && let Some(node) = self.find_node(id)
            && let Some(scope) = node.enclosing_scope()
        {
            scope.record_focus(id);
        }

        self.notify_listeners(previous, node_id);
    }

    /// Get the currently focused node (if any).
    #[inline]
    pub fn focused(&self) -> Option<FocusNodeId> {
        *self.primary_focus.read()
    }

    /// Alias for [`Self::focused`] — matches Flutter's `primaryFocus` getter.
    #[inline]
    pub fn primary_focus(&self) -> Option<FocusNodeId> {
        *self.primary_focus.read()
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
        *self.primary_focus.read() == Some(node_id)
    }

    /// Clear focus (no element focused).
    ///
    /// Call this when:
    /// - User clicks on background
    /// - Window loses focus
    /// - Focused element is removed
    pub fn unfocus(&self) {
        self.set_primary_focus(None);
    }

    /// Add a listener for focus changes.
    ///
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

    /// Remove all listeners. Useful for cleanup during shutdown.
    pub fn clear_listeners(&self) {
        self.listeners.write().clear();
    }

    /// Notify all listeners of a focus change.
    fn notify_listeners(&self, previous: Option<FocusNodeId>, new: Option<FocusNodeId>) {
        // Clone the listener Vec so listener invocations can call
        // `add_listener` / `clear_listeners` without deadlocking on
        // our read lock (Flutter ChangeNotifier reentrancy semantics).
        let listeners = self.listeners.read().clone();
        for listener in &listeners {
            listener(previous, new);
        }
    }

    /// Locate a focus node by ID by descending from the root scope.
    ///
    /// Returns `None` if the node is not attached to the tree.
    /// O(N) over tree nodes — used during focus-change history
    /// recording (not on the per-event hot path).
    pub(crate) fn find_node(&self, id: FocusNodeId) -> Option<Arc<FocusNode>> {
        let root = self.root_scope.as_focus_node();
        if root.id() == id {
            return Some(root.clone());
        }
        root.descendants().find(|node| node.id() == id)
    }

    /// Transfer focus to the next focusable element via the active
    /// scope's traversal policy (Tab key). The active scope defaults
    /// to [`Self::root_scope`] when no override is set via
    /// [`Self::set_active_scope`].
    ///
    /// Returns `true` if focus advanced, `false` if no element is
    /// currently focused or the traversal policy returned `None`.
    pub fn focus_next(&self) -> bool {
        let Some(current) = *self.primary_focus.read() else {
            tracing::trace!("focus_next: no element currently focused");
            return false;
        };
        let scope = self.active_scope();
        let Some(next_id) = scope.next_focusable_id(current) else {
            return false;
        };
        self.set_primary_focus(Some(next_id));
        true
    }

    /// Transfer focus to the previous focusable element via the
    /// active scope's traversal policy (Shift+Tab).
    ///
    /// See [`Self::focus_next`] for behavior contract.
    pub fn focus_previous(&self) -> bool {
        let Some(current) = *self.primary_focus.read() else {
            tracing::trace!("focus_previous: no element currently focused");
            return false;
        };
        let scope = self.active_scope();
        let Some(prev_id) = scope.previous_focusable_id(current) else {
            return false;
        };
        self.set_primary_focus(Some(prev_id));
        true
    }

    /// Check if any element has focus.
    #[inline]
    pub fn is_focused(&self) -> bool {
        self.primary_focus.read().is_some()
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
    /// 1. Global handlers (in order added) — stop if any returns `true`
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
        // First, try global handlers — clone so the handler can mutate
        // the global handler list without deadlocking.
        let global_handlers = self.global_key_handlers.read().clone();
        for handler in &global_handlers {
            if handler(event) {
                tracing::trace!("Key event handled by global handler");
                return true;
            }
        }

        // Then, try focused node's handler
        let focused = *self.primary_focus.read();
        if let Some(node_id) = focused
            && let Some(handler) = self.key_handlers.read().get(&node_id).cloned()
            && handler(event)
        {
            tracing::trace!(node = node_id.get(), "Key event handled by focused node");
            return true;
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
    fn test_focus_manager_has_root_scope() {
        let manager = FocusManager::new_for_test();
        // Root scope is always present and attached.
        assert!(manager.root_scope().as_focus_node().is_attached());
        assert!(
            manager.root_scope().as_focus_node().is_scope(),
            "root backing node must identify as a FocusScopeNode"
        );
        // Active scope falls back to root scope.
        assert_eq!(
            manager.active_scope().id(),
            manager.root_scope().id(),
            "active_scope should default to root_scope"
        );
    }

    #[test]
    fn test_request_focus() {
        let manager = FocusManager::new_for_test();
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
        let manager = FocusManager::new_for_test();
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
        let manager = FocusManager::new_for_test();
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

        let manager = FocusManager::new_for_test();
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
    fn focus_next_and_previous_walk_active_scope_and_record_history() {
        let manager = FocusManager::new_for_test();
        let root = manager.root_scope().clone();

        let first = FocusNode::with_debug_label("first");
        first.set_rect(flui_types::geometry::Rect::from_xywh(
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(10.0),
            flui_types::geometry::Pixels(10.0),
        ));
        let second = FocusNode::with_debug_label("second");
        second.set_rect(flui_types::geometry::Rect::from_xywh(
            flui_types::geometry::Pixels(20.0),
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(10.0),
            flui_types::geometry::Pixels(10.0),
        ));
        let third = FocusNode::with_debug_label("third");
        third.set_rect(flui_types::geometry::Rect::from_xywh(
            flui_types::geometry::Pixels(40.0),
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(10.0),
            flui_types::geometry::Pixels(10.0),
        ));

        root.attach_node(&first);
        root.attach_node(&second);
        root.attach_node(&third);

        manager.request_focus(first.id());
        assert_eq!(manager.focused(), Some(first.id()));
        assert_eq!(root.focused_child(), Some(first.id()));

        assert!(manager.focus_next());
        assert_eq!(manager.focused(), Some(second.id()));
        assert_eq!(root.focused_child(), Some(second.id()));
        assert!(!manager.has_focus(first.id()));
        assert!(manager.has_focus(second.id()));

        assert!(manager.focus_previous());
        assert_eq!(manager.focused(), Some(first.id()));
        assert_eq!(root.focused_child(), Some(first.id()));

        manager.request_focus(third.id());
        assert!(manager.focus_next(), "reading-order traversal wraps");
        assert_eq!(manager.focused(), Some(first.id()));
    }

    #[test]
    fn focus_traversal_respects_active_scope_and_skips_unfocusable_nodes() {
        let manager = FocusManager::new_for_test();
        let root = manager.root_scope().clone();
        let outside = FocusNode::with_debug_label("outside");
        outside.set_rect(flui_types::geometry::Rect::from_xywh(
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(10.0),
            flui_types::geometry::Pixels(10.0),
        ));
        root.attach_node(&outside);

        let dialog = FocusScopeNode::with_debug_label("dialog");
        dialog
            .as_focus_node()
            .set_rect(flui_types::geometry::Rect::from_xywh(
                flui_types::geometry::Pixels(10.0),
                flui_types::geometry::Pixels(0.0),
                flui_types::geometry::Pixels(10.0),
                flui_types::geometry::Pixels(10.0),
            ));
        root.attach_node(dialog.as_focus_node());

        let first = FocusNode::with_debug_label("first");
        first.set_rect(flui_types::geometry::Rect::from_xywh(
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(10.0),
            flui_types::geometry::Pixels(10.0),
        ));
        let skipped = FocusNode::with_debug_label("skipped");
        skipped.set_skip_traversal(true);
        skipped.set_rect(flui_types::geometry::Rect::from_xywh(
            flui_types::geometry::Pixels(10.0),
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(10.0),
            flui_types::geometry::Pixels(10.0),
        ));
        let disabled = FocusNode::with_debug_label("disabled");
        disabled.set_can_request_focus(false);
        disabled.set_rect(flui_types::geometry::Rect::from_xywh(
            flui_types::geometry::Pixels(20.0),
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(10.0),
            flui_types::geometry::Pixels(10.0),
        ));
        let second = FocusNode::with_debug_label("second");
        second.set_rect(flui_types::geometry::Rect::from_xywh(
            flui_types::geometry::Pixels(30.0),
            flui_types::geometry::Pixels(0.0),
            flui_types::geometry::Pixels(10.0),
            flui_types::geometry::Pixels(10.0),
        ));

        dialog.attach_node(&first);
        dialog.attach_node(&skipped);
        dialog.attach_node(&disabled);
        dialog.attach_node(&second);

        manager.set_active_scope(Some(dialog.clone()));
        manager.request_focus(first.id());

        assert!(manager.focus_next());
        assert_eq!(
            manager.focused(),
            Some(second.id()),
            "active-scope traversal skips skipTraversal/canRequestFocus=false nodes"
        );

        assert!(manager.focus_next(), "active-scope traversal wraps");
        assert_eq!(
            manager.focused(),
            Some(first.id()),
            "outside root sibling must not participate while dialog is active"
        );
        assert_eq!(dialog.focused_child(), Some(first.id()));
    }

    #[test]
    fn test_request_same_focus_no_change() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let manager = FocusManager::new_for_test();
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

        let manager = FocusManager::new_for_test();
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

        let manager = FocusManager::new_for_test();
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
        use std::sync::atomic::{AtomicBool, Ordering};

        use crate::{events::keyboard::Code, testing::input::KeyEventBuilder};

        let manager = FocusManager::new_for_test();
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
        let event = KeyEventBuilder::new(Code::KeyA).build();

        // Dispatch - should be handled
        let result = manager.dispatch_key_event(&event);
        assert!(result);
        assert!(handled.load(Ordering::Relaxed));
    }

    #[test]
    fn test_dispatch_key_event_no_focus() {
        use std::sync::atomic::{AtomicBool, Ordering};

        use crate::{events::keyboard::Code, testing::input::KeyEventBuilder};

        let manager = FocusManager::new_for_test();
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
        let event = KeyEventBuilder::new(Code::KeyA).build();

        // Dispatch - should NOT be handled (no focus)
        let result = manager.dispatch_key_event(&event);
        assert!(!result);
        assert!(!handled.load(Ordering::Relaxed));
    }

    #[test]
    fn test_global_key_handler() {
        use std::sync::atomic::{AtomicBool, Ordering};

        use crate::{events::keyboard::Code, testing::input::KeyEventBuilder};

        let manager = FocusManager::new_for_test();

        let global_handled = Arc::new(AtomicBool::new(false));
        let global_clone = global_handled.clone();

        manager.add_global_key_handler(Arc::new(move |_event| {
            global_clone.store(true, Ordering::Relaxed);
            true
        }));

        let event = KeyEventBuilder::new(Code::Escape).build();

        // Global handler should be called even without focus
        let result = manager.dispatch_key_event(&event);
        assert!(result);
        assert!(global_handled.load(Ordering::Relaxed));
    }

    #[test]
    fn test_global_handler_priority() {
        use std::sync::atomic::{AtomicBool, Ordering};

        use crate::{events::keyboard::Code, testing::input::KeyEventBuilder};

        let manager = FocusManager::new_for_test();
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

        let event = KeyEventBuilder::new(Code::Enter).build();
        manager.dispatch_key_event(&event);

        // Global should be called first and handle
        assert!(global_called.load(Ordering::Relaxed));
        // Node handler should NOT be called (global handled it)
        assert!(!node_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_global_handler_passthrough() {
        use std::sync::atomic::{AtomicBool, Ordering};

        use crate::{events::keyboard::Code, testing::input::KeyEventBuilder};

        let manager = FocusManager::new_for_test();
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

        let event = KeyEventBuilder::new(Code::KeyX).build();
        manager.dispatch_key_event(&event);

        // Both should be called
        assert!(global_called.load(Ordering::Relaxed));
        assert!(node_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_clear_global_key_handlers() {
        use std::sync::atomic::{AtomicBool, Ordering};

        use crate::{events::keyboard::Code, testing::input::KeyEventBuilder};

        let manager = FocusManager::new_for_test();

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        manager.add_global_key_handler(Arc::new(move |_event| {
            called_clone.store(true, Ordering::Relaxed);
            true
        }));

        manager.clear_global_key_handlers();

        let event = KeyEventBuilder::new(Code::Tab).build();
        manager.dispatch_key_event(&event);

        // Handler should not be called (was cleared)
        assert!(!called.load(Ordering::Relaxed));
    }
}
