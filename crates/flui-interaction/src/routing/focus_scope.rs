//! Focus tree nodes for keyboard navigation (Flutter-compatible architecture)
//!
//! This module provides Flutter-compatible focus management using a tree
//! structure:
//!
//! - [`FocusNode`] - A node in the focus tree that can receive keyboard focus
//! - [`FocusScopeNode`] - A special FocusNode that groups descendants and
//!   tracks focus history
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
//! - `hasFocus` = any descendant has focus, `hasPrimaryFocus` = this node has
//!   focus
//!
//! # Singleton manager (I-4 closure)
//!
//! Prior incarnations of this module held a `manager:
//! RwLock<Option<Weak<FocusManagerInner>>>` reference on each
//! [`FocusNode`], plus a private `FocusManagerInner` Arc-based dual
//! state living alongside the public [`crate::FocusManager`] singleton.
//! The current design collapses that into a single singleton: focus nodes reach the
//! manager via [`crate::FocusManager::global`] without any weak-ref
//! dance — the singleton is always live and globally reachable.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::routing::{FocusNode, FocusScopeNode, FocusManager};
//!
//! // Create a focusable node
//! let node = FocusNode::new_with_debug_label("my_button");
//! node.set_on_key_event(Arc::new(|event| { /* handle key */ false }));
//!
//! // Attach to root scope
//! FocusManager::global().root_scope().attach_node(&node);
//!
//! // Request focus
//! node.request_focus();
//! ```
//!
//! # References
//!
//! - [Flutter FocusNode](https://api.flutter.dev/flutter/widgets/FocusNode-class.html)
//! - [Flutter FocusScopeNode](https://api.flutter.dev/flutter/widgets/FocusScopeNode-class.html)
//! - [Understanding Flutter's keyboard focus system](https://docs.flutter.dev/ui/interactivity/focus)

use std::{
    cmp::Ordering,
    collections::VecDeque,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    },
};

use flui_types::geometry::{Pixels, Rect};
use parking_lot::{Mutex, RwLock};

use crate::events::KeyEvent;
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
pub type KeyEventHandler = Arc<dyn Fn(&KeyEvent) -> KeyEventResult + Send + Sync>;

/// Computes a node's bounding rectangle on demand, in root coordinates.
///
/// Installed by the widget layer, which owns the render geometry the
/// reading-order traversal sorts by (ADR-0022 §4's traversal-geometry gap):
/// the node itself cannot reach a render tree. `None` while the widget has no
/// committed layout to measure.
pub type RectProvider = Arc<dyn Fn() -> Option<Rect<Pixels>> + Send + Sync>;

/// Result of key event processing — Flutter's `KeyEventResult`
/// (`focus_manager.dart:73-88`), consumed by the leaf→root dispatch walk
/// (ADR-0023 U1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventResult {
    /// Event was handled; the walk stops and the event counts as consumed.
    Handled,
    /// Event was ignored; the walk continues to the parent node.
    Ignored,
    /// The walk stops, but the event does **not** count as consumed.
    SkipRemainingHandlers,
}

impl KeyEventResult {
    /// Flutter's `combineKeyEventResults` (`focus_manager.dart:98-110`), for
    /// one node's several handler channels: any `Handled` wins; else any
    /// `SkipRemainingHandlers`; else `Ignored`.
    #[must_use]
    pub fn combine(self, other: KeyEventResult) -> KeyEventResult {
        use KeyEventResult::{Handled, Ignored, SkipRemainingHandlers};
        match (self, other) {
            (Handled, _) | (_, Handled) => Handled,
            (SkipRemainingHandlers, _) | (_, SkipRemainingHandlers) => SkipRemainingHandlers,
            (Ignored, Ignored) => Ignored,
        }
    }
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
/// # Manager access
///
/// Focus nodes no longer hold a `Weak<FocusManagerInner>` — they reach
/// the [`crate::FocusManager`] singleton via [`crate::FocusManager::global`]
/// directly. The [`Self::is_attached`] flag still gates focus operations so
/// nodes that haven't been mounted into the tree behave as no-ops.
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

    /// Weak owner when this node is the backing node for a
    /// [`FocusScopeNode`].
    scope_owner: RwLock<Option<Weak<FocusScopeNode>>>,

    /// Key event handler.
    on_key_event: RwLock<Option<KeyEventHandler>>,

    /// Bounding rectangle (for spatial navigation).
    rect: RwLock<Rect<Pixels>>,
    /// The live geometry source, when a widget installed one; wins over
    /// [`rect`](Self::rect)'s stored value.
    rect_provider: RwLock<Option<RectProvider>>,

    /// Whether this node is attached to the focus tree.
    attached: AtomicBool,
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
            scope_owner: RwLock::new(None),
            on_key_event: RwLock::new(None),
            rect: RwLock::new(Rect::ZERO),
            rect_provider: RwLock::new(None),
            attached: AtomicBool::new(false),
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
            scope_owner: RwLock::new(None),
            on_key_event: RwLock::new(None),
            rect: RwLock::new(Rect::ZERO),
            rect_provider: RwLock::new(None),
            attached: AtomicBool::new(false),
        })
    }

    fn new_scope_backing_node(
        label: Option<String>,
        scope_owner: Weak<FocusScopeNode>,
    ) -> Arc<Self> {
        Arc::new(Self {
            id: allocate_focus_node_id(),
            debug_label: label,
            parent: RwLock::new(None),
            children: RwLock::new(Vec::new()),
            can_request_focus: AtomicBool::new(true),
            skip_traversal: AtomicBool::new(false),
            descendants_are_focusable: AtomicBool::new(true),
            scope_owner: RwLock::new(Some(scope_owner)),
            on_key_event: RwLock::new(None),
            rect: RwLock::new(Rect::ZERO),
            rect_provider: RwLock::new(None),
            attached: AtomicBool::new(false),
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
        self.own_can_request_focus()
            && self
                .ancestors()
                .all(|ancestor| ancestor.allows_descendant_focus())
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
        self.parent
            .read()
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
    }

    /// Returns the children.
    pub fn children(&self) -> Vec<Arc<FocusNode>> {
        self.children.read().clone()
    }

    /// Returns the bounding rectangle: a live [`RectProvider`]'s answer when
    /// one is installed (widget-mounted nodes measure their render anchor
    /// lazily), else the stored value.
    pub fn rect(&self) -> Rect<Pixels> {
        if let Some(provider) = self.rect_provider.read().as_ref()
            && let Some(rect) = provider()
        {
            return rect;
        }
        *self.rect.read()
    }

    /// Sets the bounding rectangle.
    pub fn set_rect(&self, rect: Rect<Pixels>) {
        *self.rect.write() = rect;
    }

    /// Install a live geometry source — see [`RectProvider`].
    pub fn set_rect_provider(&self, provider: RectProvider) {
        *self.rect_provider.write() = Some(provider);
    }

    /// Remove the live geometry source; [`rect`](Self::rect) falls back to the
    /// stored value. The installing widget clears on dispose so an external
    /// node never measures a dead anchor.
    pub fn clear_rect_provider(&self) {
        *self.rect_provider.write() = None;
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
    /// Reaches the [`crate::FocusManager`] singleton directly (no Weak
    /// upgrade dance) — singleton always live.
    ///
    /// Gated on `is_attached()` so detached nodes
    /// (still holding a stale FocusManager.primary_focus ID после
    /// `detach_child`) don't lie about having focus.
    pub fn has_focus(&self) -> bool {
        if !self.is_attached() {
            return false;
        }
        let Some(focused_id) = crate::FocusManager::global().primary_focus() else {
            return false;
        };
        self.id == focused_id || self.has_descendant(focused_id)
    }

    /// Returns whether this specific node has primary focus.
    ///
    /// Same is_attached() gate as `has_focus` to
    /// avoid stale-focus reports for detached nodes.
    pub fn has_primary_focus(&self) -> bool {
        if !self.is_attached() {
            return false;
        }
        crate::FocusManager::global().primary_focus() == Some(self.id)
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
        self.scope_owner.read().as_ref().and_then(Weak::upgrade)
    }

    /// Returns true if this is a FocusScopeNode.
    pub fn is_scope(&self) -> bool {
        self.scope_owner
            .read()
            .as_ref()
            .is_some_and(|w| w.strong_count() > 0)
    }

    /// Requests primary focus for this node.
    ///
    /// No-op when the node cannot request focus or is not attached to
    /// the focus tree.
    pub fn request_focus(self: &Arc<Self>) {
        if !self.can_request_focus() || !self.is_attached() {
            return;
        }
        crate::FocusManager::global().request_focus(self.id);
    }

    /// Removes focus from this node.
    pub fn unfocus(&self) {
        if !self.has_primary_focus() {
            return;
        }
        crate::FocusManager::global().unfocus();
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

    /// Handles a key event through this node's [`on_key_event`] handler.
    ///
    /// The handler's [`KeyEventResult`] passes through verbatim —
    /// `SkipRemainingHandlers` included — so the dispatch walk can honor it
    /// (`focus_manager.dart:2288-2301`). `Ignored` when no handler is set.
    ///
    /// [`on_key_event`]: Self::set_on_key_event
    pub fn handle_key_event(&self, event: &KeyEvent) -> KeyEventResult {
        match self.on_key_event.read().as_ref() {
            Some(handler) => handler(event),
            None => KeyEventResult::Ignored,
        }
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

    /// Mark the root node as attached. Used by [`crate::FocusManager`]
    /// during default construction — the root scope has no parent so
    /// the normal `attach_child` path doesn't run for it.
    pub(crate) fn mark_root_attached(node: &Arc<FocusNode>) {
        node.attached.store(true, AtomicOrdering::Release);
    }

    /// Attach `child` under **this node** — scope or not. The node-tree edge
    /// the leaf→root key-dispatch walk follows (ADR-0023 U2): a non-scope
    /// `Focus` in the widget tree nests here so keys the focused descendant
    /// ignores bubble through it, Flutter's node-tree shape.
    pub fn attach_node(self: &Arc<Self>, child: &Arc<FocusNode>) {
        self.attach_child(child);
    }

    /// Detach `child_id` from this node — **removal** semantics: the subtree
    /// is marked detached and loses the primary focus if it held it. For a
    /// scope parent, prefer [`FocusScopeNode::detach_node`], which also cleans
    /// the focused-child history.
    pub fn detach_node(&self, child_id: FocusNodeId) {
        self.detach_child(child_id);
    }

    /// Move `node` — with its whole subtree — under this node, **preserving
    /// focus** (ADR-0022 U1.3, generalized to plain-node parents for
    /// ADR-0023). A moved subtree that holds the primary focus keeps it, the
    /// old enclosing scope's history forgets the moved ids, and the *nearest*
    /// scope at the new location records the focused id. A node already under
    /// this parent is a no-op.
    pub fn adopt_node(self: &Arc<Self>, node: &Arc<FocusNode>) {
        let old_parent = node.parent();
        if old_parent
            .as_ref()
            .is_some_and(|parent| parent.id == self.id)
        {
            return;
        }

        if let Some(old_parent) = old_parent {
            // Lift the node out of the old parent's child list *without*
            // `detach_child`'s removal semantics (recursive detached-marking
            // and primary-focus clearing).
            old_parent
                .children
                .write()
                .retain(|child| child.id != node.id);
            // The old scope's focused-child history forgets every id that
            // moved away with the subtree.
            if let Some(old_scope) = old_parent
                .as_scope()
                .or_else(|| old_parent.enclosing_scope())
            {
                old_scope
                    .focus_history
                    .lock()
                    .retain(|&id| id != node.id && !node.has_descendant(id));
            }
        }

        self.attach_child(node);

        // A moved subtree that holds the primary focus keeps it — and the
        // nearest scope's history learns about it, exactly as
        // `set_primary_focus` would have recorded had the focus been
        // requested here.
        if let Some(focused) = crate::FocusManager::global().primary_focus()
            && (node.id == focused || node.has_descendant(focused))
            && let Some(scope) = self.as_scope().or_else(|| self.enclosing_scope())
        {
            scope.record_focus(focused);
        }
    }

    fn attach_child(self: &Arc<Self>, child: &Arc<FocusNode>) {
        // Set parent
        *child.parent.write() = Some(Arc::downgrade(self));

        // Mark as attached — the singleton manager is always reachable
        // via FocusManager::global, so no per-node manager reference
        // needs to be propagated.
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

            // Detach recursively so the entire removed
            // subtree is marked detached, not just the direct child.
            // Without this, descendants kept `attached == true` despite
            // being unreachable from the root scope — making
            // `request_focus` on those descendants succeed AND
            // `has_focus`/`has_primary_focus` (with the new is_attached
            // gate) return inconsistent results across the subtree.
            Self::detach_subtree(&child);

            // Clear FocusManager.primary_focus if the detached subtree
            // owned the current focus — prevents stale focus state from
            // outliving the tree node.
            let focus_mgr = crate::FocusManager::global();
            if let Some(focused_id) = focus_mgr.primary_focus()
                && (child.id == focused_id || child.has_descendant(focused_id))
            {
                focus_mgr.unfocus();
            }
        }
    }

    /// Recursively mark a FocusNode and all its descendants as detached.
    ///
    /// Used by [`Self::detach_child`] to ensure the entire removed subtree's
    /// attached state matches reality после parent-link clearing.
    fn detach_subtree(node: &Arc<FocusNode>) {
        node.attached.store(false, AtomicOrdering::Release);
        for grandchild in node.children.read().iter() {
            Self::detach_subtree(grandchild);
        }
    }

    fn own_can_request_focus(&self) -> bool {
        self.can_request_focus.load(AtomicOrdering::Acquire)
    }

    fn allows_descendant_focus(&self) -> bool {
        self.descendants_are_focusable() && (!self.is_scope() || self.own_can_request_focus())
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
            scope_owner: RwLock::new(None),
            on_key_event: RwLock::new(None),
            rect: RwLock::new(Rect::ZERO),
            rect_provider: RwLock::new(None),
            attached: AtomicBool::new(false),
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
            .finish_non_exhaustive()
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
/// let dialog_scope = FocusScopeNode::with_debug_label("dialog");
/// dialog_scope.set_autofocus(true);
///
/// // Add children
/// dialog_scope.attach_node(&text_field);
/// dialog_scope.attach_node(&ok_button);
/// dialog_scope.attach_node(&cancel_button);
///
/// // Later: focus returns to last focused child
/// dialog_scope.set_first_focus();
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
        Arc::new_cyclic(|weak_self| Self {
            inner: FocusNode::new_scope_backing_node(None, weak_self.clone()),
            focus_history: Mutex::new(VecDeque::new()),
            autofocus: AtomicBool::new(false),
            traps_focus: AtomicBool::new(false),
            traversal_policy: RwLock::new(Arc::new(ReadingOrderPolicy)),
        })
    }

    /// Creates a new focus scope with a debug label.
    pub fn with_debug_label(label: impl Into<String>) -> Arc<Self> {
        let label = label.into();
        Arc::new_cyclic(|weak_self| Self {
            inner: FocusNode::new_scope_backing_node(Some(label.clone()), weak_self.clone()),
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

    /// Move `node` — with its whole subtree — under this scope, **preserving
    /// focus**.
    ///
    /// The reparent primitive (ADR-0022 U1.3). [`detach_node`](Self::detach_node)
    /// followed by [`attach_node`](Self::attach_node) is a *removal* and an
    /// insertion: the detach marks the subtree detached and clears the manager's
    /// primary focus if the subtree held it — right for an unmount, wrong for a
    /// move. Flutter's `FocusAttachment.reparent` preserves focus across the
    /// move; this is that operation for a scope-parented node.
    ///
    /// A node with no current parent is simply attached; adopting a node already
    /// under this scope is a no-op.
    pub fn adopt_node(self: &Arc<Self>, node: &Arc<FocusNode>) {
        self.inner.adopt_node(node);
    }

    /// Sets focus to the first focusable child via the singleton.
    pub fn set_first_focus(self: &Arc<Self>) {
        let nodes = self.collect_focusable_nodes();
        if let Some(first) = nodes.first() {
            crate::FocusManager::global().request_focus(first.id());
        }
    }

    /// Compute the next focusable node ID per the scope's traversal
    /// policy without mutating any focus state.
    ///
    /// Returns `None` if no next focusable exists. Use this when the
    /// caller (e.g. the [`crate::FocusManager`] singleton's
    /// `focus_next`) needs to update its own focused state.
    pub fn next_focusable_id(&self, current: FocusNodeId) -> Option<FocusNodeId> {
        let nodes = self.collect_focusable_nodes();
        let policy = self.traversal_policy.read().clone();
        policy.find_next(current, &nodes)
    }

    /// Compute the previous focusable node ID per the scope's traversal
    /// policy without mutating any focus state.
    pub fn previous_focusable_id(&self, current: FocusNodeId) -> Option<FocusNodeId> {
        let nodes = self.collect_focusable_nodes();
        let policy = self.traversal_policy.read().clone();
        policy.find_previous(current, &nodes)
    }

    /// Focuses the next node in this scope. Returns `true` when focus
    /// advanced.
    pub fn focus_next_in_scope(&self, current: FocusNodeId) -> bool {
        let Some(next_id) = self.next_focusable_id(current) else {
            return false;
        };
        crate::FocusManager::global().request_focus(next_id);
        true
    }

    /// Focuses the previous node in this scope.
    pub fn focus_previous_in_scope(&self, current: FocusNodeId) -> bool {
        let Some(prev_id) = self.previous_focusable_id(current) else {
            return false;
        };
        crate::FocusManager::global().request_focus(prev_id);
        true
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
            .filter(|node| node.can_request_focus() && !node.skip_traversal())
            .collect()
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
            .finish_non_exhaustive()
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
        let sorted = Self::sorted_indices(nodes);
        let current_idx = sorted.iter().position(|&i| nodes[i].id() == current)?;
        let next_idx = (current_idx + 1) % sorted.len();
        Some(nodes[sorted[next_idx]].id())
    }

    fn find_previous(&self, current: FocusNodeId, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        let sorted = Self::sorted_indices(nodes);
        let current_idx = sorted.iter().position(|&i| nodes[i].id() == current)?;
        let prev_idx = if current_idx == 0 {
            sorted.len() - 1
        } else {
            current_idx - 1
        };
        Some(nodes[sorted[prev_idx]].id())
    }

    fn find_first(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        let sorted = Self::sorted_indices(nodes);
        sorted.first().map(|&i| nodes[i].id())
    }

    fn find_last(&self, nodes: &[Arc<FocusNode>]) -> Option<FocusNodeId> {
        let sorted = Self::sorted_indices(nodes);
        sorted.last().map(|&i| nodes[i].id())
    }
}

impl ReadingOrderPolicy {
    /// Returns indices into `nodes` sorted by reading order (top-to-bottom,
    /// left-to-right). Avoids cloning `Arc<FocusNode>` — only sorts
    /// lightweight indices.
    fn sorted_indices(nodes: &[Arc<FocusNode>]) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..nodes.len()).collect();
        indices.sort_by(|&a, &b| {
            let rect_a = nodes[a].rect();
            let rect_b = nodes[b].rect();

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
        indices
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that drive the process-global [`FocusManager`] serialize here —
    /// nextest isolates test *binaries*, not the threads inside one, and a
    /// concurrent test would see (or clobber) this one's primary focus.
    static GLOBAL_FOCUS_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    /// ADR-0022 U1.3: `adopt_node` is a **move**, not a remove-plus-insert —
    /// the primary focus survives the reparent, the new scope's history learns
    /// the focused id, and the old parent forgets the subtree. `detach_node` +
    /// `attach_node` would clear the focus (`detach_child`'s removal
    /// semantics): that is the reparent-drops-focus hazard this pins.
    ///
    /// Red-check (verified): implementing `adopt_node` as `detach_node` then
    /// `attach_node` makes the moved node lose primary focus, failing the
    /// keeps-focus assertion after the move.
    #[test]
    fn adopt_preserves_primary_focus_across_a_reparent() {
        let _guard = GLOBAL_FOCUS_LOCK.lock();
        let manager = crate::FocusManager::global();
        manager.unfocus();

        // Under the global root, as widget-owned scopes are: focus-history
        // recording resolves the focused node by descending from the root.
        let scope_a = FocusScopeNode::with_debug_label("adopt-from");
        let scope_b = FocusScopeNode::with_debug_label("adopt-to");
        manager.root_scope().attach_node(scope_a.as_focus_node());
        manager.root_scope().attach_node(scope_b.as_focus_node());
        let moved = FocusNode::with_debug_label("moved");
        scope_a.attach_node(&moved);
        moved.request_focus();
        assert!(moved.has_primary_focus(), "sanity: focused before the move");
        assert_eq!(scope_a.focused_child(), Some(moved.id()));

        scope_b.adopt_node(&moved);

        assert!(
            moved.has_primary_focus(),
            "a moved node keeps the primary focus"
        );
        assert_eq!(
            moved.parent().map(|parent| parent.id()),
            Some(scope_b.as_focus_node().id()),
            "the node now hangs under the adopting scope"
        );
        assert!(
            scope_a.as_focus_node().children().is_empty(),
            "the old parent no longer lists the moved node"
        );
        assert_eq!(
            scope_a.focused_child(),
            None,
            "the old scope's history forgot the moved subtree"
        );
        assert_eq!(
            scope_b.focused_child(),
            Some(moved.id()),
            "the new scope's history records the moved focus"
        );

        // Adopting again is a no-op.
        scope_b.adopt_node(&moved);
        assert_eq!(scope_b.as_focus_node().children().len(), 1);

        // The removal semantics still live where they belong: a genuine
        // detach clears the focus.
        scope_b.detach_node(moved.id());
        assert!(!moved.has_primary_focus());
        assert_eq!(manager.primary_focus(), None, "detach still unfocuses");

        // Leave the process-global root the way this test found it.
        manager
            .root_scope()
            .detach_node(scope_a.as_focus_node().id());
        manager
            .root_scope()
            .detach_node(scope_b.as_focus_node().id());
    }

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
        assert!(
            scope.as_focus_node().is_scope(),
            "scope backing node should identify as its FocusScopeNode"
        );
    }

    #[test]
    fn test_attach_child() {
        let scope = FocusScopeNode::new();
        let node = FocusNode::new();

        scope.attach_node(&node);

        assert_eq!(scope.as_focus_node().children().len(), 1);
        assert!(node.parent().is_some());
        // After attach the child is marked attached.
        assert!(node.is_attached());
    }

    #[test]
    fn test_focus_manager_owns_root_scope() {
        // The singleton's root scope is constructed eagerly and attached.
        let manager = crate::FocusManager::new_for_test();
        assert!(manager.root_scope().as_focus_node().is_attached());
        assert!(manager.primary_focus().is_none());
    }

    #[test]
    fn test_reading_order_policy() {
        let nodes = vec![
            {
                let n = FocusNode::new();
                n.set_rect(Rect::from_xywh(
                    Pixels(100.0),
                    Pixels(0.0),
                    Pixels(50.0),
                    Pixels(30.0),
                )); // right
                n
            },
            {
                let n = FocusNode::new();
                n.set_rect(Rect::from_xywh(
                    Pixels(0.0),
                    Pixels(0.0),
                    Pixels(50.0),
                    Pixels(30.0),
                )); // left
                n
            },
            {
                let n = FocusNode::new();
                n.set_rect(Rect::from_xywh(
                    Pixels(0.0),
                    Pixels(50.0),
                    Pixels(50.0),
                    Pixels(30.0),
                )); // bottom
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

    #[test]
    fn descendants_are_focusable_blocks_descendants_not_self() {
        let outer = FocusScopeNode::new();
        let inner_scope = FocusScopeNode::with_debug_label("inner");
        let child = FocusNode::with_debug_label("child");

        outer.attach_node(inner_scope.as_focus_node());
        inner_scope.attach_node(&child);

        assert!(inner_scope.as_focus_node().can_request_focus());
        assert!(child.can_request_focus());

        inner_scope
            .as_focus_node()
            .set_descendants_are_focusable(false);

        assert!(
            inner_scope.as_focus_node().can_request_focus(),
            "descendants_are_focusable=false must not disable the node itself"
        );
        assert!(
            !child.can_request_focus(),
            "descendants_are_focusable=false must prevent descendant focus"
        );
    }
}
