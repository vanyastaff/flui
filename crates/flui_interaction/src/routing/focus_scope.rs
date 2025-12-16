//! Focus tree nodes for keyboard navigation (Flutter-compatible architecture)
//!
//! This module provides Flutter-compatible focus management using a tree structure:
//!
//! - [`FocusNode`] - A node in the focus tree that can receive keyboard focus
//! - [`FocusScopeNode`] - A special FocusNode that groups descendants and tracks focus history
//! - [`FocusAttachment`] - RAII handle for attaching FocusNode to the tree
//! - [`FocusTraversalPolicy`] - Determines Tab/Shift+Tab navigation order
//!
//! # Flutter Architecture
//!
//! The focus system mirrors Flutter's design:
//!
//! ```text
//! FocusManager (singleton)
//!     └── rootScope: FocusScopeNode
//!             ├── FocusNode (button)
//!             ├── FocusScopeNode (dialog)
//!             │       ├── FocusNode (text_field)
//!             │       └── FocusNode (ok_button)
//!             └── FocusNode (menu)
//! ```
//!
//! Key concepts from Flutter:
//! - Focus nodes form a **tree** parallel to the widget tree
//! - `FocusScopeNode` restricts traversal and remembers focus history
//! - `hasFocus` = any descendant has focus, `hasPrimaryFocus` = this node has focus
//! - Nodes must be attached via `FocusAttachment` lifecycle
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::routing::{FocusNode, FocusScopeNode, FocusManager};
//!
//! // Create a focusable node
//! let mut node = FocusNode::new()
//!     .with_debug_label("my_button")
//!     .on_key(|event| { /* handle key */ false });
//!
//! // Attach to tree (typically in widget's mount)
//! let attachment = node.attach(parent_scope);
//!
//! // Request focus
//! node.request_focus();
//!
//! // Detach when widget unmounts
//! drop(attachment);
//! ```
//!
//! # References
//!
//! - [Flutter FocusNode](https://api.flutter.dev/flutter/widgets/FocusNode-class.html)
//! - [Flutter FocusScopeNode](https://api.flutter.dev/flutter/widgets/FocusScopeNode-class.html)
//! - [Understanding Flutter's keyboard focus system](https://docs.flutter.dev/ui/interactivity/focus)

use parking_lot::{Mutex, RwLock};
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Weak};

use crate::events::KeyEvent;
use flui_types::geometry::{Offset, Rect};

// Re-export FocusNodeId from ids module
pub use crate::ids::FocusNodeId;

// Global ID counter for focus nodes
static NEXT_FOCUS_NODE_ID: AtomicU64 = AtomicU64::new(1);

fn allocate_focus_node_id() -> FocusNodeId {
    let id = NEXT_FOCUS_NODE_ID.fetch_add(1, AtomicOrdering::Relaxed);
    FocusNodeId::new(id)
}

// ============================================================================
// Key Event Handler
// ============================================================================

/// Callback for handling key events.
///
/// Returns `true` if the event was handled (stops propagation).
pub type KeyEventHandler = Arc<dyn Fn(&KeyEvent) -> bool + Send + Sync>;

/// Result of key event processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventResult {
    /// Event was handled, stop propagation.
    Handled,
    /// Event was ignored, continue to parent.
    Ignored,
    /// Skip to next focus node (for traversal).
    SkipRemainingHandlers,
}

// ============================================================================
// FocusNode
// ============================================================================

/// A node in the focus tree that can receive keyboard focus.
///
/// `FocusNode` is the core building block of the focus system. Each focusable
/// widget should have its own `FocusNode` that manages:
/// - Whether the widget can receive focus
/// - Key event handling
/// - Focus state (hasFocus, hasPrimaryFocus)
///
/// # Flutter Compliance
///
/// | Flutter | FLUI | Notes |
/// |---------|------|-------|
/// | `hasFocus` | `has_focus()` | Any descendant has focus |
/// | `hasPrimaryFocus` | `has_primary_focus()` | This node specifically has focus |
/// | `canRequestFocus` | `can_request_focus` | Whether focus can be requested |
/// | `skipTraversal` | `skip_traversal` | Skip during Tab navigation |
/// | `onKeyEvent` | `on_key_event` | Key event callback |
/// | `requestFocus()` | `request_focus()` | Request primary focus |
/// | `unfocus()` | `unfocus()` | Remove focus |
/// | `nextFocus()` | `next_focus()` | Move to next focusable |
/// | `previousFocus()` | `previous_focus()` | Move to previous focusable |
///
/// # Example
///
/// ```rust,ignore
/// let node = FocusNode::new()
///     .with_debug_label("my_button")
///     .can_request_focus(true)
///     .on_key(|event| {
///         if event.logical_key == Key::Enter {
///             activate_button();
///             return true;
///         }
///         false
///     });
/// ```
pub struct FocusNode {
    /// Unique identifier for this node.
    id: FocusNodeId,

    /// Debug label for diagnostics.
    debug_label: Option<String>,

    /// Parent node (weak reference to avoid cycles).
    parent: RwLock<Option<Weak<FocusNode>>>,

    /// Child nodes.
    children: RwLock<Vec<Arc<FocusNode>>>,

    /// Whether this node can request focus.
    can_request_focus: AtomicBool,

    /// Whether to skip this node during traversal.
    skip_traversal: AtomicBool,

    /// Whether descendants can be focused.
    descendants_are_focusable: AtomicBool,

    /// Key event handler.
    on_key_event: RwLock<Option<KeyEventHandler>>,

    /// Bounding rectangle (for spatial navigation).
    rect: RwLock<Rect>,

    /// Whether this node is attached to the focus tree.
    attached: AtomicBool,

    /// The manager this node belongs to (set on attach).
    manager: RwLock<Option<Weak<FocusManagerInner>>>,
}

impl FocusNode {
    /// Creates a new focus node.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            id: allocate_focus_node_id(),
            debug_label: None,
            parent: RwLock::new(None),
            children: RwLock::new(Vec::new()),
            can_request_focus: AtomicBool::new(true),
            skip_traversal: AtomicBool::new(false),
            descendants_are_focusable: AtomicBool::new(true),
            on_key_event: RwLock::new(None),
            rect: RwLock::new(Rect::ZERO),
            attached: AtomicBool::new(false),
            manager: RwLock::new(None),
        })
    }

    /// Creates a new focus node with a debug label.
    pub fn with_debug_label(label: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            id: allocate_focus_node_id(),
            debug_label: Some(label.into()),
            parent: RwLock::new(None),
            children: RwLock::new(Vec::new()),
            can_request_focus: AtomicBool::new(true),
            skip_traversal: AtomicBool::new(false),
            descendants_are_focusable: AtomicBool::new(true),
            on_key_event: RwLock::new(None),
            rect: RwLock::new(Rect::ZERO),
            attached: AtomicBool::new(false),
            manager: RwLock::new(None),
        })
    }

    /// Returns the unique identifier.
    #[inline]
    pub fn id(&self) -> FocusNodeId {
        self.id
    }

    /// Returns the debug label.
    #[inline]
    pub fn debug_label(&self) -> Option<&str> {
        self.debug_label.as_deref()
    }

    /// Returns whether this node can request focus.
    #[inline]
    pub fn can_request_focus(&self) -> bool {
        self.can_request_focus.load(AtomicOrdering::Acquire)
    }

    /// Sets whether this node can request focus.
    pub fn set_can_request_focus(&self, can: bool) {
        self.can_request_focus.store(can, AtomicOrdering::Release);
    }

    /// Returns whether to skip this node during traversal.
    #[inline]
    pub fn skip_traversal(&self) -> bool {
        self.skip_traversal.load(AtomicOrdering::Acquire)
    }

    /// Sets whether to skip this node during traversal.
    pub fn set_skip_traversal(&self, skip: bool) {
        self.skip_traversal.store(skip, AtomicOrdering::Release);
    }

    /// Returns whether descendants are focusable.
    #[inline]
    pub fn descendants_are_focusable(&self) -> bool {
        self.descendants_are_focusable.load(AtomicOrdering::Acquire)
    }

    /// Sets whether descendants are focusable.
    pub fn set_descendants_are_focusable(&self, focusable: bool) {
        self.descendants_are_focusable
            .store(focusable, AtomicOrdering::Release);
    }

    /// Returns whether this node is attached to the focus tree.
    #[inline]
    pub fn is_attached(&self) -> bool {
        self.attached.load(AtomicOrdering::Acquire)
    }

    /// Returns the parent node.
    pub fn parent(&self) -> Option<Arc<FocusNode>> {
        self.parent.read().as_ref().and_then(|w| w.upgrade())
    }

    /// Returns the children.
    pub fn children(&self) -> Vec<Arc<FocusNode>> {
        self.children.read().clone()
    }

    /// Returns the bounding rectangle.
    pub fn rect(&self) -> Rect {
        *self.rect.read()
    }

    /// Sets the bounding rectangle.
    pub fn set_rect(&self, rect: Rect) {
        *self.rect.write() = rect;
    }

    /// Sets the key event handler.
    pub fn set_on_key_event(&self, handler: KeyEventHandler) {
        *self.on_key_event.write() = Some(handler);
    }

    /// Clears the key event handler.
    pub fn clear_on_key_event(&self) {
        *self.on_key_event.write() = None;
    }

    /// Returns whether this node has focus (this node or any descendant).
    ///
    /// This is `true` if any node in this subtree has primary focus.
    pub fn has_focus(&self) -> bool {
        if let Some(manager) = self.manager.read().as_ref().and_then(|w| w.upgrade()) {
            if let Some(focused_id) = manager.primary_focus() {
                // Check if focused node is this node or a descendant
                return self.id == focused_id || self.has_descendant(focused_id);
            }
        }
        false
    }

    /// Returns whether this specific node has primary focus.
    pub fn has_primary_focus(&self) -> bool {
        if let Some(manager) = self.manager.read().as_ref().and_then(|w| w.upgrade()) {
            return manager.primary_focus() == Some(self.id);
        }
        false
    }

    /// Checks if a node with the given ID is a descendant of this node.
    fn has_descendant(&self, id: FocusNodeId) -> bool {
        for child in self.children.read().iter() {
            if child.id == id || child.has_descendant(id) {
                return true;
            }
        }
        false
    }

    /// Finds the nearest enclosing scope.
    pub fn enclosing_scope(&self) -> Option<Arc<FocusScopeNode>> {
        let mut current = self.parent();
        while let Some(node) = current {
            if let Some(scope) = node.as_scope() {
                return Some(scope);
            }
            current = node.parent();
        }
        None
    }

    /// Returns this node as a FocusScopeNode if it is one.
    ///
    /// Override in FocusScopeNode.
    pub fn as_scope(&self) -> Option<Arc<FocusScopeNode>> {
        None
    }

    /// Returns true if this is a FocusScopeNode.
    pub fn is_scope(&self) -> bool {
        false
    }

    /// Requests primary focus for this node.
    pub fn request_focus(self: &Arc<Self>) {
        if !self.can_request_focus() || !self.is_attached() {
            return;
        }

        if let Some(manager) = self.manager.read().as_ref().and_then(|w| w.upgrade()) {
            manager.set_primary_focus(Some(self.id));
        }
    }

    /// Removes focus from this node.
    pub fn unfocus(&self) {
        if !self.has_primary_focus() {
            return;
        }

        if let Some(manager) = self.manager.read().as_ref().and_then(|w| w.upgrade()) {
            manager.set_primary_focus(None);
        }
    }

    /// Moves focus to the next focusable node.
    pub fn next_focus(&self) -> bool {
        if let Some(scope) = self.enclosing_scope() {
            return scope.focus_next_in_scope(self.id);
        }
        false
    }

    /// Moves focus to the previous focusable node.
    pub fn previous_focus(&self) -> bool {
        if let Some(scope) = self.enclosing_scope() {
            return scope.focus_previous_in_scope(self.id);
        }
        false
    }

    /// Handles a key event.
    ///
    /// Returns the result indicating whether the event was handled.
    pub fn handle_key_event(&self, event: &KeyEvent) -> KeyEventResult {
        if let Some(handler) = self.on_key_event.read().as_ref() {
            if handler(event) {
                return KeyEventResult::Handled;
            }
        }
        KeyEventResult::Ignored
    }

    /// Iterates over all ancestors (parent, grandparent, etc.).
    pub fn ancestors(&self) -> impl Iterator<Item = Arc<FocusNode>> {
        AncestorIterator {
            current: self.parent(),
        }
    }

    /// Iterates over all descendants in depth-first order.
    pub fn descendants(&self) -> impl Iterator<Item = Arc<FocusNode>> {
        DescendantIterator {
            stack: self.children(),
        }
    }

    /// Returns the depth of this node in the tree (0 = root).
    pub fn depth(&self) -> usize {
        self.ancestors().count()
    }

    // ========================================================================
    // Internal methods
    // ========================================================================

    fn attach_child(self: &Arc<Self>, child: &Arc<FocusNode>) {
        // Set parent
        *child.parent.write() = Some(Arc::downgrade(self));

        // Copy manager reference
        *child.manager.write() = self.manager.read().clone();

        // Mark as attached
        child.attached.store(true, AtomicOrdering::Release);

        // Add to children
        self.children.write().push(child.clone());
    }

    fn detach_child(&self, child_id: FocusNodeId) {
        let mut children = self.children.write();
        if let Some(pos) = children.iter().position(|c| c.id == child_id) {
            let child = children.remove(pos);

            // Clear parent
            *child.parent.write() = None;

            // Clear manager
            *child.manager.write() = None;

            // Mark as detached
            child.attached.store(false, AtomicOrdering::Release);
        }
    }
}

impl Default for FocusNode {
    fn default() -> Self {
        Self {
            id: allocate_focus_node_id(),
            debug_label: None,
            parent: RwLock::new(None),
            children: RwLock::new(Vec::new()),
            can_request_focus: AtomicBool::new(true),
            skip_traversal: AtomicBool::new(false),
            descendants_are_focusable: AtomicBool::new(true),
            on_key_event: RwLock::new(None),
            rect: RwLock::new(Rect::ZERO),
            attached: AtomicBool::new(false),
            manager: RwLock::new(None),
        }
    }
}

impl std::fmt::Debug for FocusNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusNode")
            .field("id", &self.id)
            .field("debug_label", &self.debug_label)
            .field("can_request_focus", &self.can_request_focus())
            .field("skip_traversal", &self.skip_traversal())
            .field("attached", &self.is_attached())
            .field("children_count", &self.children.read().len())
            .finish()
    }
}

// ============================================================================
// Ancestor/Descendant Iterators
// ============================================================================

struct AncestorIterator {
    current: Option<Arc<FocusNode>>,
}

impl Iterator for AncestorIterator {
    type Item = Arc<FocusNode>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.current.take()?;
        self.current = node.parent();
        Some(node)
    }
}

struct DescendantIterator {
    stack: Vec<Arc<FocusNode>>,
}

impl Iterator for DescendantIterator {
    type Item = Arc<FocusNode>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        // Add children in reverse order so we visit them in order
        let children = node.children();
        for child in children.into_iter().rev() {
            self.stack.push(child);
        }
        Some(node)
    }
}

// ============================================================================
// FocusScopeNode
// ============================================================================

/// A special FocusNode that groups descendants and manages focus history.
///
/// `FocusScopeNode` serves two main purposes:
/// 1. **Scope traversal**: Tab navigation stays within the scope
/// 2. **Focus history**: Remembers which node was last focused
///
/// # Flutter Compliance
///
/// | Flutter | FLUI | Notes |
/// |---------|------|-------|
/// | `focusedChild` | `focused_child()` | Currently focused descendant |
/// | `autofocus` | `autofocus` | Auto-focus first child on attach |
/// | `setFirstFocus()` | `set_first_focus()` | Focus first focusable child |
///
/// # Example
///
/// ```rust,ignore
/// // Create a dialog scope
/// let dialog_scope = FocusScopeNode::new()
///     .with_debug_label("dialog")
///     .autofocus(true);
///
/// // Add children
/// dialog_scope.attach_node(&text_field);
/// dialog_scope.attach_node(&ok_button);
/// dialog_scope.attach_node(&cancel_button);
///
/// // Later: focus returns to last focused child
/// dialog_scope.request_focus();
/// ```
pub struct FocusScopeNode {
    /// The underlying focus node.
    inner: Arc<FocusNode>,

    /// Focus history stack (most recent first).
    focus_history: Mutex<VecDeque<FocusNodeId>>,

    /// Whether to auto-focus first child.
    autofocus: AtomicBool,

    /// Whether focus is trapped within this scope (modal behavior).
    traps_focus: AtomicBool,

    /// Traversal policy for this scope.
    traversal_policy: RwLock<Arc<dyn FocusTraversalPolicy>>,
}

impl FocusScopeNode {
    /// Creates a new focus scope node.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: FocusNode::new(),
            focus_history: Mutex::new(VecDeque::new()),
            autofocus: AtomicBool::new(false),
            traps_focus: AtomicBool::new(false),
            traversal_policy: RwLock::new(Arc::new(ReadingOrderPolicy)),
        })
    }

    /// Creates a new focus scope with a debug label.
    pub fn with_debug_label(label: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            inner: FocusNode::with_debug_label(label),
            focus_history: Mutex::new(VecDeque::new()),
            autofocus: AtomicBool::new(false),
            traps_focus: AtomicBool::new(false),
            traversal_policy: RwLock::new(Arc::new(ReadingOrderPolicy)),
        })
    }

    /// Returns the underlying FocusNode.
    #[inline]
    pub fn as_focus_node(&self) -> &Arc<FocusNode> {
        &self.inner
    }

    /// Returns the unique identifier.
    #[inline]
    pub fn id(&self) -> FocusNodeId {
        self.inner.id()
    }

    /// Returns whether autofocus is enabled.
    #[inline]
    pub fn autofocus(&self) -> bool {
        self.autofocus.load(AtomicOrdering::Acquire)
    }

    /// Sets autofocus.
    pub fn set_autofocus(&self, autofocus: bool) {
        self.autofocus.store(autofocus, AtomicOrdering::Release);
    }

    /// Returns whether focus is trapped.
    #[inline]
    pub fn traps_focus(&self) -> bool {
        self.traps_focus.load(AtomicOrdering::Acquire)
    }

    /// Sets whether focus is trapped (modal behavior).
    pub fn set_traps_focus(&self, traps: bool) {
        self.traps_focus.store(traps, AtomicOrdering::Release);
    }

    /// Sets the traversal policy.
    pub fn set_traversal_policy(&self, policy: Arc<dyn FocusTraversalPolicy>) {
        *self.traversal_policy.write() = policy;
    }

    /// Returns the currently focused child in this scope.
    pub fn focused_child(&self) -> Option<FocusNodeId> {
        self.focus_history.lock().front().copied()
    }

    /// Attaches a child node to this scope.
    pub fn attach_node(self: &Arc<Self>, node: &Arc<FocusNode>) {
        self.inner.attach_child(node);
    }

    /// Detaches a child node from this scope.
    pub fn detach_node(&self, node_id: FocusNodeId) {
        self.inner.detach_child(node_id);

        // Remove from focus history
        self.focus_history.lock().retain(|id| *id != node_id);
    }

    /// Sets focus to the first focusable child.
    pub fn set_first_focus(self: &Arc<Self>) {
        let nodes = self.collect_focusable_nodes();
        if let Some(first) = nodes.first() {
            if let Some(manager) = self.inner.manager.read().as_ref().and_then(|w| w.upgrade()) {
                manager.set_primary_focus(Some(first.id()));
            }
        }
    }

    /// Focuses the next node in this scope.
    pub fn focus_next_in_scope(&self, current: FocusNodeId) -> bool {
        let nodes = self.collect_focusable_nodes();
        let policy = self.traversal_policy.read().clone();

        if let Some(next_id) = policy.find_next(current, &nodes) {
            if let Some(manager) = self.inner.manager.read().as_ref().and_then(|w| w.upgrade()) {
                manager.set_primary_focus(Some(next_id));
                return true;
            }
        }
        false
    }

    /// Focuses the previous node in this scope.
    pub fn focus_previous_in_scope(&self, current: FocusNodeId) -> bool {
        let nodes = self.collect_focusable_nodes();
        let policy = self.traversal_policy.read().clone();

        if let Some(prev_id) = policy.find_previous(current, &nodes) {
            if let Some(manager) = self.inner.manager.read().as_ref().and_then(|w| w.upgrade()) {
                manager.set_primary_focus(Some(prev_id));
                return true;
            }
        }
        false
    }

    /// Records that a node received focus.
    pub fn record_focus(&self, node_id: FocusNodeId) {
        let mut history = self.focus_history.lock();

        // Remove if already in history
        history.retain(|id| *id != node_id);

        // Add to front
        history.push_front(node_id);

        // Limit history size
        while history.len() > 10 {
            history.pop_back();
        }
    }

    /// Collects all focusable nodes in this scope.
    fn collect_focusable_nodes(&self) -> Vec<Arc<FocusNode>> {
        self.inner
            .descendants()
            .filter(|node| {
                node.can_request_focus()
                    && !node.skip_traversal()
                    && node.descendants_are_focusable()
            })
            .collect()
    }
}

impl Default for FocusScopeNode {
    fn default() -> Self {
        Self {
            inner: FocusNode::new(),
            focus_history: Mutex::new(VecDeque::new()),
            autofocus: AtomicBool::new(false),
            traps_focus: AtomicBool::new(false),
            traversal_policy: RwLock::new(Arc::new(ReadingOrderPolicy)),
        }
    }
}

impl std::fmt::Debug for FocusScopeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusScopeNode")
            .field("id", &self.inner.id())
            .field("debug_label", &self.inner.debug_label())
            .field("autofocus", &self.autofocus())
            .field("traps_focus", &self.traps_focus())
            .field("focused_child", &self.focused_child())
            .finish()
    }
}

// ============================================================================
// FocusTraversalPolicy
// ============================================================================

/// Determines the order for Tab/Shift+Tab navigation.
///
/// Implement this trait for custom traversal behavior.
pub trait FocusTraversalPolicy: Send + Sync + std::fmt::Debug {
    /// Finds the next node to focus from current.
    fn find_next(&self, current: FocusNodeId, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId>;

    /// Finds the previous node to focus from current.
    fn find_previous(&self, current: FocusNodeId, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId>;

    /// Finds the first focusable node.
    fn find_first(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId>;

    /// Finds the last focusable node.
    fn find_last(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId>;
}

// ============================================================================
// ReadingOrderPolicy
// ============================================================================

/// Traversal policy using reading order (top-to-bottom, left-to-right).
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadingOrderPolicy;

impl FocusTraversalPolicy for ReadingOrderPolicy {
    fn find_next(&self, current: FocusNodeId, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        let mut sorted = nodes.to_vec();
        self.sort_nodes(&mut sorted);

        let current_idx = sorted.iter().position(|n| n.id() == current)?;
        let next_idx = (current_idx + 1) % sorted.len();
        Some(sorted[next_idx].id())
    }

    fn find_previous(&self, current: FocusNodeId, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        let mut sorted = nodes.to_vec();
        self.sort_nodes(&mut sorted);

        let current_idx = sorted.iter().position(|n| n.id() == current)?;
        let prev_idx = if current_idx == 0 {
            sorted.len() - 1
        } else {
            current_idx - 1
        };
        Some(sorted[prev_idx].id())
    }

    fn find_first(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        let mut sorted = nodes.to_vec();
        self.sort_nodes(&mut sorted);
        sorted.first().map(|n| n.id())
    }

    fn find_last(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        let mut sorted = nodes.to_vec();
        self.sort_nodes(&mut sorted);
        sorted.last().map(|n| n.id())
    }
}

impl ReadingOrderPolicy {
    fn sort_nodes(&self, nodes: &mut [Arc<FocusNode>]) {
        nodes.sort_by(|a, b| {
            let rect_a = a.rect();
            let rect_b = b.rect();

            // Primary: top-to-bottom
            let y_cmp = rect_a
                .top()
                .partial_cmp(&rect_b.top())
                .unwrap_or(Ordering::Equal);
            if y_cmp != Ordering::Equal {
                return y_cmp;
            }

            // Secondary: left-to-right
            rect_a
                .left()
                .partial_cmp(&rect_b.left())
                .unwrap_or(Ordering::Equal)
        });
    }
}

// ============================================================================
// OrderedTraversalPolicy
// ============================================================================

/// Traversal policy using explicit order.
///
/// Nodes are sorted by their depth in the tree, then by their position
/// in the children list.
#[derive(Debug, Clone, Copy, Default)]
pub struct OrderedTraversalPolicy;

impl FocusTraversalPolicy for OrderedTraversalPolicy {
    fn find_next(&self, current: FocusNodeId, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        // Use tree order (depth-first)
        let current_idx = nodes.iter().position(|n| n.id() == current)?;
        let next_idx = (current_idx + 1) % nodes.len();
        Some(nodes[next_idx].id())
    }

    fn find_previous(&self, current: FocusNodeId, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        let current_idx = nodes.iter().position(|n| n.id() == current)?;
        let prev_idx = if current_idx == 0 {
            nodes.len() - 1
        } else {
            current_idx - 1
        };
        Some(nodes[prev_idx].id())
    }

    fn find_first(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        nodes.first().map(|n| n.id())
    }

    fn find_last(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        nodes.last().map(|n| n.id())
    }
}

// ============================================================================
// DirectionalFocusPolicy
// ============================================================================

/// Direction for directional focus navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Traversal policy for arrow key navigation.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirectionalFocusPolicy;

impl DirectionalFocusPolicy {
    /// Finds the nearest node in the given direction.
    pub fn find_in_direction(
        &self,
        current: &FocusNode,
        direction: TraversalDirection,
        nodes: &[Arc<FocusNode>],
    ) -> Option<FocusNodeId> {
        let current_rect = current.rect();
        let current_center = Offset::new(
            (current_rect.left() + current_rect.right()) / 2.0,
            (current_rect.top() + current_rect.bottom()) / 2.0,
        );

        let candidates: Vec<&Arc<FocusNode>> = nodes
            .iter()
            .filter(|n| {
                if n.id() == current.id() {
                    return false;
                }

                let rect = n.rect();
                let center = Offset::new(
                    (rect.left() + rect.right()) / 2.0,
                    (rect.top() + rect.bottom()) / 2.0,
                );

                match direction {
                    TraversalDirection::Up => center.dy < current_center.dy,
                    TraversalDirection::Down => center.dy > current_center.dy,
                    TraversalDirection::Left => center.dx < current_center.dx,
                    TraversalDirection::Right => center.dx > current_center.dx,
                }
            })
            .collect();

        candidates
            .into_iter()
            .min_by(|a, b| {
                let rect_a = a.rect();
                let rect_b = b.rect();
                let center_a = Offset::new(
                    (rect_a.left() + rect_a.right()) / 2.0,
                    (rect_a.top() + rect_a.bottom()) / 2.0,
                );
                let center_b = Offset::new(
                    (rect_b.left() + rect_b.right()) / 2.0,
                    (rect_b.top() + rect_b.bottom()) / 2.0,
                );

                let dist_a = distance_squared(current_center, center_a);
                let dist_b = distance_squared(current_center, center_b);
                dist_a.partial_cmp(&dist_b).unwrap_or(Ordering::Equal)
            })
            .map(|n| n.id())
    }
}

impl FocusTraversalPolicy for DirectionalFocusPolicy {
    fn find_next(&self, current: FocusNodeId, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        // Default to reading order for Tab
        ReadingOrderPolicy.find_next(current, nodes)
    }

    fn find_previous(&self, current: FocusNodeId, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        ReadingOrderPolicy.find_previous(current, nodes)
    }

    fn find_first(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        ReadingOrderPolicy.find_first(nodes)
    }

    fn find_last(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        ReadingOrderPolicy.find_last(nodes)
    }
}

#[inline]
fn distance_squared(a: Offset, b: Offset) -> f32 {
    let dx = b.dx - a.dx;
    let dy = b.dy - a.dy;
    dx * dx + dy * dy
}

// ============================================================================
// FocusManagerInner (internal state)
// ============================================================================

/// Internal state for focus management.
pub(crate) struct FocusManagerInner {
    /// Root scope.
    root_scope: Arc<FocusScopeNode>,

    /// Currently focused node ID.
    primary_focus: RwLock<Option<FocusNodeId>>,

    /// Focus change listeners.
    #[allow(clippy::type_complexity)]
    listeners: RwLock<Vec<Arc<dyn Fn(Option<FocusNodeId>, Option<FocusNodeId>) + Send + Sync>>>,
}

#[allow(dead_code)] // Future public API for focus management
impl FocusManagerInner {
    pub fn new() -> Arc<Self> {
        let root_scope = FocusScopeNode::with_debug_label("Root Focus Scope");

        let manager = Arc::new(Self {
            root_scope: root_scope.clone(),
            primary_focus: RwLock::new(None),
            listeners: RwLock::new(Vec::new()),
        });

        // Set manager reference in root scope
        *root_scope.inner.manager.write() = Some(Arc::downgrade(&manager));
        root_scope
            .inner
            .attached
            .store(true, AtomicOrdering::Release);

        manager
    }

    pub fn root_scope(&self) -> &Arc<FocusScopeNode> {
        &self.root_scope
    }

    pub fn primary_focus(&self) -> Option<FocusNodeId> {
        *self.primary_focus.read()
    }

    pub fn set_primary_focus(&self, node_id: Option<FocusNodeId>) {
        let previous = *self.primary_focus.read();
        if previous == node_id {
            return;
        }

        *self.primary_focus.write() = node_id;

        // Record in enclosing scope's history
        if let Some(id) = node_id {
            if let Some(node) = self.find_node(id) {
                if let Some(scope) = node.enclosing_scope() {
                    scope.record_focus(id);
                }
            }
        }

        // Notify listeners
        let listeners = self.listeners.read().clone();
        for listener in listeners {
            listener(previous, node_id);
        }
    }

    pub fn add_listener(
        &self,
        callback: Arc<dyn Fn(Option<FocusNodeId>, Option<FocusNodeId>) + Send + Sync>,
    ) {
        self.listeners.write().push(callback);
    }

    fn find_node(&self, id: FocusNodeId) -> Option<Arc<FocusNode>> {
        // Search from root
        if self.root_scope.inner.id() == id {
            return Some(self.root_scope.inner.clone());
        }

        self.root_scope
            .inner
            .descendants()
            .find(|node| node.id() == id)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_node_creation() {
        let node = FocusNode::new();
        assert!(node.can_request_focus());
        assert!(!node.skip_traversal());
        assert!(!node.is_attached());
    }

    #[test]
    fn test_focus_node_id_niche() {
        assert_eq!(
            std::mem::size_of::<Option<FocusNodeId>>(),
            std::mem::size_of::<FocusNodeId>()
        );
    }

    #[test]
    fn test_focus_scope_creation() {
        let scope = FocusScopeNode::new();
        assert!(!scope.autofocus());
        assert!(!scope.traps_focus());
        assert!(scope.focused_child().is_none());
    }

    #[test]
    fn test_attach_child() {
        let scope = FocusScopeNode::new();
        let node = FocusNode::new();

        scope.attach_node(&node);

        assert_eq!(scope.as_focus_node().children().len(), 1);
        assert!(node.parent().is_some());
    }

    #[test]
    fn test_focus_manager_inner() {
        let manager = FocusManagerInner::new();

        assert!(manager.primary_focus().is_none());
        assert!(manager.root_scope().as_focus_node().is_attached());
    }

    #[test]
    fn test_reading_order_policy() {
        let nodes = vec![
            {
                let n = FocusNode::new();
                n.set_rect(Rect::from_xywh(100.0, 0.0, 50.0, 30.0)); // right
                n
            },
            {
                let n = FocusNode::new();
                n.set_rect(Rect::from_xywh(0.0, 0.0, 50.0, 30.0)); // left
                n
            },
            {
                let n = FocusNode::new();
                n.set_rect(Rect::from_xywh(0.0, 50.0, 50.0, 30.0)); // bottom
                n
            },
        ];

        let policy = ReadingOrderPolicy;

        // First should be top-left
        let first = policy.find_first(&nodes);
        assert_eq!(first, Some(nodes[1].id())); // left one
    }

    #[test]
    fn test_focus_history() {
        let scope = FocusScopeNode::new();

        let id1 = FocusNodeId::new(100);
        let id2 = FocusNodeId::new(200);

        scope.record_focus(id1);
        assert_eq!(scope.focused_child(), Some(id1));

        scope.record_focus(id2);
        assert_eq!(scope.focused_child(), Some(id2));
    }
}
