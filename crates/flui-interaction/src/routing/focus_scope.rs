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
//! FocusManager (owner-thread TLS)
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
//! # Owner-thread focus manager
//!
//! Focus nodes reach the current owner thread's TLS manager through
//! [`crate::FocusManager::global`]. They do not store a per-node manager
//! reference or consult a parallel registry.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::routing::{FocusNode, FocusScopeNode, FocusManager};
//!
//! // Create a focusable node
//! let node = FocusNode::new_with_debug_label("my_button");
//! node.set_on_key_event(Rc::new(|event| { /* handle key */ false }));
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
    rc::Rc,
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

/// Owner-local callback for handling key events.
///
/// Returns the key event result used by the leaf-to-root focus dispatch walk.
/// This is intentionally not `Send + Sync`: focus callbacks run on the UI
/// owner thread and may capture owner-local widget state.
pub type KeyEventHandler = Rc<dyn Fn(&KeyEvent) -> KeyEventResult>;

/// Computes a node's bounding rectangle on demand, in root coordinates.
///
/// Installed by the widget layer, which owns the render geometry the
/// reading-order traversal sorts by (the traversal-geometry gap ADR-0022 records):
/// the node itself cannot reach a render tree. `None` while the widget has no
/// committed layout to measure. This is owner-local under ADR-0027, matching
/// [`KeyEventHandler`].
pub type RectProvider = Rc<dyn Fn() -> Option<Rect<Pixels>>>;

/// Result of key event processing — Flutter's `KeyEventResult`
/// (`focus_manager.dart:73-88`), consumed by the leaf→root dispatch walk
/// (ADR-0023).
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
/// Focus nodes do not store a manager reference; they reach the current owner
/// thread's TLS [`crate::FocusManager`] through
/// [`crate::FocusManager::global`]. The [`Self::is_attached`] flag still gates
/// focus operations for nodes outside that owner's focus tree.
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

    /// A `request_focus()` call made while unattached, fulfilled the moment
    /// [`Self::attach_child`] attaches this node — Flutter's requestFocus-
    /// before-attach queuing (`FocusNode._requestFocus`/`_hasKeyboardToken`
    /// resolve their pending grant on `FocusAttachment.reparent`).  Consumed
    /// (cleared) on attach whether or not the retried request actually
    /// succeeds, so a request dropped for another reason (e.g.
    /// `can_request_focus` turned false in the meantime) does not linger and
    /// fire on a later, unrelated attach.
    pending_focus_request: AtomicBool,
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
            pending_focus_request: AtomicBool::new(false),
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
            pending_focus_request: AtomicBool::new(false),
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
            pending_focus_request: AtomicBool::new(false),
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
    ///
    /// On a `true` to `false` change, primary focus **held by this node
    /// itself** is released (Flutter: `FocusNode.canRequestFocus` setter,
    /// `focus_scope_test.dart`'s "Focus is lost when set to not focusable.",
    /// tag 3.44.0). Unlike [`Self::set_descendants_are_focusable`], this does
    /// not evict a focused *descendant* — `can_request_focus` gates only
    /// whether this node itself may hold focus.
    pub fn set_can_request_focus(&self, can: bool) {
        let previous = self.can_request_focus.swap(can, AtomicOrdering::AcqRel);
        if previous == can {
            return;
        }
        if !can && self.has_primary_focus() {
            crate::FocusManager::global().unfocus();
        }
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
    ///
    /// On a `true` to `false` change, focus held by this node or any descendant
    /// is cleared; the node's own future eligibility is unchanged. FLUI
    /// currently clears primary focus to `None` and does not yet apply
    /// Flutter's previously-focused-child fallback in the enclosing scope.
    pub fn set_descendants_are_focusable(&self, focusable: bool) {
        let previous = self
            .descendants_are_focusable
            .swap(focusable, AtomicOrdering::AcqRel);
        if previous == focusable {
            return;
        }
        if !focusable && self.has_focus() {
            crate::FocusManager::global().unfocus();
        }
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
    /// Queries the current owner thread's manager directly; its TLS instance
    /// is initialized by [`crate::FocusManager::global`].
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
    /// No-op when the node cannot request focus. A request made while the
    /// node is not yet attached to the focus tree is deferred rather than
    /// dropped: [`Self::attach_child`] retries it the moment this node joins
    /// the tree — Flutter's requestFocus-before-attach queuing
    /// (`FocusNode._requestFocus`/`_hasKeyboardToken`, fulfilled on
    /// `FocusAttachment.reparent`).
    pub fn request_focus(self: &Arc<Self>) {
        if !self.can_request_focus() {
            return;
        }
        if !self.is_attached() {
            self.pending_focus_request
                .store(true, AtomicOrdering::Release);
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
    /// the leaf→root key-dispatch walk follows (ADR-0023): a non-scope
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
    /// focus** (ADR-0022, generalized to plain-node parents for
    /// ADR-0023). A moved subtree that holds the primary focus keeps it, the
    /// old enclosing scope's history forgets the moved ids, and the *nearest*
    /// scope at the new location records the focused id. A node already under
    /// this parent is a no-op.
    pub fn adopt_node(self: &Arc<Self>, node: &Arc<FocusNode>) {
        // A node cannot become its own ancestor: the parent pointers would form
        // a cycle, and the very next `enclosing_scope()` walk would spin
        // forever. Reachable from a widget's `did_change_dependencies`, so
        // refuse it rather than hang.
        if self.id == node.id || node.has_descendant(self.id) {
            tracing::error!(
                "BUG: a focus node cannot be adopted into its own subtree; the move is refused"
            );
            return;
        }

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
        // Lift the node out of any previous parent first. Without this it sits
        // in **two** child lists: `descendants()` yields it twice — so it
        // appears twice in the traversal order — and the abandoned parent's
        // later `detach_child` unfocuses a node it no longer owns.
        if let Some(old_parent) = child.parent()
            && old_parent.id != self.id
        {
            old_parent
                .children
                .write()
                .retain(|held| held.id != child.id);
        }

        // Set parent
        *child.parent.write() = Some(Arc::downgrade(self));

        // Mark as attached. Focus operations later resolve the current owner
        // thread's TLS manager, so no manager reference is propagated.
        child.attached.store(true, AtomicOrdering::Release);

        // Add to children
        self.children.write().push(child.clone());

        // Fulfill a `request_focus()` call made before this node was
        // attached. Consumed unconditionally: a request that no longer
        // applies (`can_request_focus` turned false in the meantime) must not
        // linger and fire on a later, unrelated attach.
        if child
            .pending_focus_request
            .swap(false, AtomicOrdering::AcqRel)
        {
            child.request_focus();
        }
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
            pending_focus_request: AtomicBool::new(false),
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

    /// What happens when traversal runs off the end of this scope's order —
    /// Flutter's `TraversalEdgeBehavior` (`focus_traversal.dart:113-156`).
    traversal_edge_behavior: RwLock<TraversalEdgeBehavior>,
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
            traversal_edge_behavior: RwLock::new(TraversalEdgeBehavior::default()),
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
            traversal_edge_behavior: RwLock::new(TraversalEdgeBehavior::default()),
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
    /// The reparent primitive (ADR-0022). [`detach_node`](Self::detach_node)
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

    /// This scope's edge behavior — see [`TraversalEdgeBehavior`].
    pub fn traversal_edge_behavior(&self) -> TraversalEdgeBehavior {
        *self.traversal_edge_behavior.read()
    }

    /// Set what happens when traversal runs off the end of this scope's order.
    pub fn set_traversal_edge_behavior(&self, behavior: TraversalEdgeBehavior) {
        *self.traversal_edge_behavior.write() = behavior;
    }

    /// Sets focus to the first focusable child through the current owner
    /// thread's TLS manager, in **policy order**, not attach order.
    pub fn set_first_focus(self: &Arc<Self>) {
        if let ResolvedStep::Focus(id) = self.resolve_traversal(None, true) {
            crate::FocusManager::global().request_focus(id);
        }
    }

    /// This scope's traversal candidates in policy order, with `cursor`
    /// **force-included** even when it is not itself traversable
    /// (`skip_traversal`, `can_request_focus(false)`) — Flutter force-includes
    /// the current node (`focus_traversal.dart:487-489`) precisely so a step
    /// *from* a non-traversable node still knows where it stands. The sorted
    /// list is the traversal primitive (ADR-0026); next/previous/edge are
    /// positional facts about it.
    pub fn sorted_traversal_order(&self, cursor: Option<FocusNodeId>) -> Vec<Arc<FocusNode>> {
        let mut nodes = self.collect_focusable_nodes();
        if let Some(cursor) = cursor
            && !nodes.iter().any(|node| node.id() == cursor)
            && let Some(node) = self.inner.descendants().find(|node| node.id() == cursor)
        {
            nodes.push(node);
        }
        let policy = self.traversal_policy.read().clone();
        policy.sort_descendants(&nodes)
    }

    /// One traversal step from `current` (or the first-focus fallback when
    /// nothing is focused), with this scope's edge behavior applied — **the**
    /// edge switch, shared by every caller (ADR-0026): the manager's
    /// `focus_next`/`focus_previous`, this scope's in-scope variants, and
    /// `set_first_focus`.
    ///
    /// It computes, it does not focus: the caller performs the returned intent
    /// against whichever [`crate::FocusManager`] it owns. (This scope's own
    /// stepping methods perform it against the owner-thread manager, because
    /// that is what a `FocusScopeNode` can reach; [`crate::FocusManager::focus_next`]
    /// performs it against its caller-owned receiver, so a test manager
    /// traverses without mutating the owner thread's TLS manager.)
    ///
    /// Three-state by construction (ADR-0026): a cursor **outside** this
    /// scope resolves to [`ResolvedStep::None`] without consulting the edge
    /// behavior — previously both cases collapsed into one `None`, and a
    /// `skip_traversal` cursor silently fell out of the candidate set.
    pub fn resolve_traversal(&self, current: Option<FocusNodeId>, forward: bool) -> ResolvedStep {
        let order = self.sorted_traversal_order(current);

        let Some(current) = current else {
            // Nothing focused: fall back to policy-ordered first/last —
            // Flutter's `findFirstFocus` fallback (`focus_traversal.dart:594-608`).
            let target = if forward {
                order.iter().find(|node| is_traversable(node))
            } else {
                order.iter().rev().find(|node| is_traversable(node))
            };
            return match target {
                Some(node) => ResolvedStep::Focus(node.id()),
                None => ResolvedStep::None,
            };
        };

        let Some(position) = order.iter().position(|node| node.id() == current) else {
            // Not a descendant of this scope: nothing to decide here.
            return ResolvedStep::None;
        };

        // Step positionally, skipping the force-included non-traversable
        // cursor as a *target* (it only matters as the starting position).
        let step_target = if forward {
            order[position + 1..]
                .iter()
                .find(|node| is_traversable(node))
        } else {
            order[..position]
                .iter()
                .rev()
                .find(|node| is_traversable(node))
        };
        if let Some(node) = step_target {
            return ResolvedStep::Focus(node.id());
        }

        // The true edge (`focus_traversal.dart:590-666`).
        match self.traversal_edge_behavior() {
            // Continue in the enclosing scope, keeping the cursor
            // (`focus_traversal.dart:141-149`). The enclosing scope's candidate
            // set includes this scope's nodes — the walk crosses scope
            // boundaries — so re-resolving there from the same cursor steps to
            // the first node *outside* this scope, which is what "leave the
            // scope" means. No landing on the scope's own (invisible) backing
            // node, and no geometry to invent.
            TraversalEdgeBehavior::ParentScope if self.inner.enclosing_scope().is_some() => {
                ResolvedStep::RetryInParent
            }
            // With no enclosing scope, Flutter itself falls back to a wrap
            // (`focus_traversal.dart:148-149`): parity, not a gap.
            TraversalEdgeBehavior::ClosedLoop | TraversalEdgeBehavior::ParentScope => {
                let wrap = if forward {
                    order.iter().find(|node| is_traversable(node))
                } else {
                    order.iter().rev().find(|node| is_traversable(node))
                };
                match wrap {
                    Some(node) => ResolvedStep::Focus(node.id()),
                    None => ResolvedStep::None,
                }
            }
            TraversalEdgeBehavior::Stop => ResolvedStep::None,
            TraversalEdgeBehavior::LeaveFlutterView => ResolvedStep::Unfocus,
        }
    }

    /// Focuses the next node in this scope. Returns `true` when focus
    /// advanced.
    pub fn focus_next_in_scope(&self, current: FocusNodeId) -> bool {
        Self::perform(self.step(Some(current), true))
    }

    /// Focuses the previous node in this scope.
    pub fn focus_previous_in_scope(&self, current: FocusNodeId) -> bool {
        Self::perform(self.step(Some(current), false))
    }

    /// [`resolve_traversal`](Self::resolve_traversal), following
    /// [`ResolvedStep::RetryInParent`] up the scope chain until a scope decides
    /// — the loop Flutter's `_moveFocus` performs by recursing into
    /// `parentScope.nextFocus()`.
    ///
    /// The walk is bounded by the scope chain, which is finite and acyclic
    /// (a scope's parent is a strict ancestor), so it terminates.
    pub fn step(&self, current: Option<FocusNodeId>, forward: bool) -> ResolvedStep {
        let mut scope: Option<Arc<FocusScopeNode>> = None;
        loop {
            let step = match &scope {
                Some(scope) => scope.resolve_traversal(current, forward),
                None => self.resolve_traversal(current, forward),
            };
            if step != ResolvedStep::RetryInParent {
                return step;
            }
            let node = match &scope {
                Some(scope) => Arc::clone(scope.as_focus_node()),
                None => Arc::clone(&self.inner),
            };
            let Some(parent) = node.enclosing_scope() else {
                // No parent to continue in: Flutter falls back to a wrap
                // (`focus_traversal.dart:148-149`), which is what a
                // `ClosedLoop` scope answers.
                return ResolvedStep::None;
            };
            scope = Some(parent);
        }
    }

    /// Carry out a resolved step against the current owner thread's TLS manager.
    fn perform(step: ResolvedStep) -> bool {
        match step {
            ResolvedStep::Focus(id) => {
                crate::FocusManager::global().request_focus(id);
                true
            }
            ResolvedStep::Unfocus => {
                crate::FocusManager::global().unfocus();
                false
            }
            // `step` follows the parent chain itself, so a retry never reaches
            // a performer.
            ResolvedStep::None | ResolvedStep::RetryInParent => false,
        }
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
        self.inner.descendants().filter(is_traversable).collect()
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

/// What happens when Tab traversal runs off the end of a scope's sorted
/// order — Flutter's `TraversalEdgeBehavior` (`focus_traversal.dart:113-156`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraversalEdgeBehavior {
    /// Wrap to the other end (`:120-127`). The default, and Flutter's own
    /// fallback for a scope with no parent (`:148-149`).
    #[default]
    ClosedLoop,
    /// Unfocus and report the event unconsumed, so a host can move focus out
    /// of the view (`:129-139`). FLUI has no embedder channel yet: the
    /// unfocus-and-report half is the whole implementable contract
    /// (ADR-0026).
    LeaveFlutterView,
    /// Continue traversal into the enclosing scope, keeping the cursor
    /// (`:141-149`): a step at this scope's edge crosses the boundary to the first
    /// node *outside* it (resolving to [`ResolvedStep::RetryInParent`]). With no
    /// enclosing scope it wraps within this one, like
    /// [`ClosedLoop`](Self::ClosedLoop) — Flutter's own no-parent fallback
    /// (`focus_traversal.dart:148-149`), parity not a gap.
    ParentScope,
    /// Stay put (`:151-155`).
    Stop,
}

/// The intent one traversal step resolved to. The resolver is pure; the
/// caller performs the intent against whichever [`crate::FocusManager`] it
/// owns. Scope convenience methods use the current owner thread's TLS manager;
/// manager methods use their caller-owned receiver, including in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedStep {
    /// Move the primary focus here.
    Focus(FocusNodeId),
    /// Release the primary focus and report the traversal unconsumed
    /// ([`TraversalEdgeBehavior::LeaveFlutterView`]).
    Unfocus,
    /// No focus change: `Stop` at the edge, a cursor outside the scope, or an
    /// empty scope.
    None,
    /// The step ran off this scope's edge and the scope asked to continue in
    /// the enclosing one ([`TraversalEdgeBehavior::ParentScope`]). The caller
    /// re-resolves against that scope, keeping the same cursor.
    RetryInParent,
}

/// Whether Tab may land on `node` — the candidate filter
/// (`can_request_focus && !skip_traversal`), applied per *target*: a
/// force-included cursor may sit in the order without being a landing spot.
fn is_traversable(node: &Arc<FocusNode>) -> bool {
    // A scope's backing node is **not** a landing spot. It is focusable and
    // un-skipped by construction, and it carries no geometry of its own — so it
    // sorts at the origin and would be exactly where the first Tab goes.
    // Flutter never gives a scope the primary focus.
    !node.is_scope() && node.can_request_focus() && !node.skip_traversal()
}

/// Determines the order for Tab/Shift+Tab navigation.
///
/// **The sorted list is the primitive** (ADR-0026, Flutter's
/// `_sortAllDescendants` architecture): a policy only orders candidates;
/// next/previous/first/last/edge are positional facts the scope computes.
/// (The old trait's `find_next`-as-oracle shape baked wraparound into the
/// policy and conflated "at the edge" with "cursor not in candidates".)
pub trait FocusTraversalPolicy: Send + Sync + std::fmt::Debug {
    /// `nodes`, reordered into this policy's traversal order — Flutter's
    /// `sortDescendants` (`focus_traversal.dart:503-573`'s per-policy point).
    fn sort_descendants(&self, nodes: &[Arc<FocusNode>]) -> Vec<Arc<FocusNode>>;
}

// ============================================================================
// ReadingOrderPolicy
// ============================================================================

/// Traversal policy using reading order (top-to-bottom, left-to-right).
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadingOrderPolicy;

impl FocusTraversalPolicy for ReadingOrderPolicy {
    fn sort_descendants(&self, nodes: &[Arc<FocusNode>]) -> Vec<Arc<FocusNode>> {
        Self::sorted_indices(nodes)
            .into_iter()
            .map(|index| Arc::clone(&nodes[index]))
            .collect()
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
            // `total_cmp`, not `partial_cmp(..).unwrap_or(Equal)`: the rects come
            // from a render transform now, and a single NaN made the comparator
            // non-transitive — which `sort_by` detects and panics on. A total
            // order cannot be inconsistent, whatever the input.
            let y_cmp = rect_a.top().0.total_cmp(&rect_b.top().0);
            if y_cmp != Ordering::Equal {
                return y_cmp;
            }
            rect_a.left().0.total_cmp(&rect_b.left().0)
        });
        indices
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    /// Tests that mutate or observe primary focus retain conservative fixture
    /// serialization across test owners. Each owner thread has independent TLS
    /// focus state; this prevents overlapping fixtures rather than cross-owner
    /// state clobbering.
    static GLOBAL_FOCUS_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    #[test]
    fn rect_provider_accepts_owner_local_rc_state() {
        let node = FocusNode::new();
        let calls = Rc::new(Cell::new(0));
        let calls_for_provider = Rc::clone(&calls);
        node.set_rect_provider(Rc::new(move || {
            calls_for_provider.set(calls_for_provider.get() + 1);
            Some(Rect::from_xywh(
                Pixels(1.0),
                Pixels(2.0),
                Pixels(3.0),
                Pixels(4.0),
            ))
        }));

        assert_eq!(
            node.rect(),
            Rect::from_xywh(Pixels(1.0), Pixels(2.0), Pixels(3.0), Pixels(4.0))
        );
        assert_eq!(calls.get(), 1);
    }

    /// ADR-0022: `adopt_node` is a **move**, not a remove-plus-insert —
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

        // Under the current owner thread's root, as widget-owned scopes are:
        // focus-history
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

        // Leave this owner thread's focus root the way this test found it.
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

    /// `request_focus()` called before a node is attached is deferred, not
    /// dropped: `attach_child` retries it the moment the node joins the tree.
    /// Flutter parity: `FocusManager.instance` queues a pre-attach
    /// `requestFocus()` behind the node's `FocusAttachment` and grants it on
    /// `reparent` (`focus_manager_test.dart`'s "Requesting focus before
    /// adding to tree results in a request after adding", tag 3.44.0).
    ///
    /// Red-check (verified): reverting `request_focus`/`attach_child` to the
    /// prior unconditional `!is_attached()` no-op leaves `child` never
    /// focused — the final assertion fails.
    #[test]
    fn request_focus_before_attach_is_granted_on_attach() {
        let _guard = GLOBAL_FOCUS_LOCK.lock();
        let manager = crate::FocusManager::global();
        manager.unfocus();

        let scope = FocusScopeNode::with_debug_label("pending-request-scope");
        manager.root_scope().attach_node(scope.as_focus_node());
        let child = FocusNode::with_debug_label("pending-request-child");

        // Requested while unattached: neither attached nor focused yet.
        child.request_focus();
        assert!(!child.is_attached());
        assert!(!child.has_primary_focus());
        assert_eq!(manager.primary_focus(), None);

        scope.attach_node(&child);

        assert!(
            child.has_primary_focus(),
            "the deferred request is granted the moment the node attaches"
        );
        assert_eq!(scope.focused_child(), Some(child.id()));

        manager.unfocus();
        manager.root_scope().detach_node(scope.as_focus_node().id());
    }

    /// A pending request is consumed on attach even when it can no longer
    /// succeed: `can_request_focus(false)` set between the request and the
    /// attach must not leave a stale grant that fires on some *later*,
    /// unrelated attach.
    #[test]
    fn a_pending_request_dropped_by_can_request_focus_does_not_linger() {
        let _guard = GLOBAL_FOCUS_LOCK.lock();
        let manager = crate::FocusManager::global();
        manager.unfocus();

        let scope = FocusScopeNode::with_debug_label("stale-pending-scope");
        manager.root_scope().attach_node(scope.as_focus_node());
        let child = FocusNode::with_debug_label("stale-pending-child");
        child.request_focus();
        child.set_can_request_focus(false);

        scope.attach_node(&child);
        assert!(
            !child.has_primary_focus(),
            "can_request_focus(false) defeats the retried request"
        );

        // Detach, re-enable, and reattach: the earlier request must not have
        // lingered to fire here.
        scope.detach_node(child.id());
        child.set_can_request_focus(true);
        scope.attach_node(&child);
        assert!(
            !child.has_primary_focus(),
            "a defeated pending request does not resurrect on a later attach"
        );

        manager.unfocus();
        manager.root_scope().detach_node(scope.as_focus_node().id());
    }

    #[test]
    fn test_focus_manager_owns_root_scope() {
        // A caller-owned manager constructs and attaches its root eagerly.
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

        // First of the sorted order is the top-left node — a positional fact
        // over `sort_descendants`, the trait's one primitive (ADR-0026).
        let order = policy.sort_descendants(&nodes);
        assert_eq!(order.first().map(|n| n.id()), Some(nodes[1].id())); // left one
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

    #[test]
    fn disabling_descendant_focus_evicts_self_and_allows_later_self_request() {
        let _guard = GLOBAL_FOCUS_LOCK.lock();
        let manager = crate::FocusManager::global();
        manager.unfocus();

        let policy = FocusNode::with_debug_label("policy-self");
        manager.root_scope().attach_node(&policy);
        policy.request_focus();
        assert!(policy.has_primary_focus());

        policy.set_descendants_are_focusable(false);
        assert_eq!(manager.primary_focus(), None);
        assert!(
            policy.can_request_focus(),
            "the policy does not disable itself"
        );

        policy.request_focus();
        assert!(
            policy.has_primary_focus(),
            "a later explicit request may focus the policy node itself"
        );

        manager.root_scope().detach_node(policy.id());
        manager.unfocus();
    }

    #[test]
    fn disabling_descendant_focus_evicts_deep_primary_once_and_reenable_does_not_refocus() {
        let _guard = GLOBAL_FOCUS_LOCK.lock();
        let manager = crate::FocusManager::global();
        manager.unfocus();

        let policy = FocusNode::with_debug_label("policy");
        let middle = FocusNode::with_debug_label("middle");
        let leaf = FocusNode::with_debug_label("leaf");
        manager.root_scope().attach_node(&policy);
        policy.attach_node(&middle);
        middle.attach_node(&leaf);

        let edges = Rc::new(RefCell::new(Vec::new()));
        let captured = Rc::clone(&edges);
        let listener = manager.add_listener(Rc::new(move |previous, current| {
            captured.borrow_mut().push((previous, current));
        }));

        leaf.request_focus();
        edges.borrow_mut().clear();
        policy.set_descendants_are_focusable(false);
        assert_eq!(manager.primary_focus(), None);
        assert_eq!(
            edges.borrow().as_slice(),
            &[(Some(leaf.id()), None)],
            "one policy transition emits exactly one eviction edge"
        );

        policy.set_descendants_are_focusable(false);
        policy.set_descendants_are_focusable(true);
        assert_eq!(
            manager.primary_focus(),
            None,
            "re-enabling does not refocus"
        );
        assert_eq!(edges.borrow().len(), 1, "idempotent writes emit no edge");

        leaf.request_focus();
        assert!(
            leaf.has_primary_focus(),
            "a later descendant request succeeds"
        );

        manager.remove_listener(listener);
        manager.root_scope().detach_node(policy.id());
        manager.unfocus();
    }

    #[test]
    fn disabling_descendant_focus_preserves_unrelated_sibling_primary() {
        let _guard = GLOBAL_FOCUS_LOCK.lock();
        let manager = crate::FocusManager::global();
        manager.unfocus();

        let policy = FocusNode::with_debug_label("policy");
        let policy_child = FocusNode::with_debug_label("policy-child");
        let sibling = FocusNode::with_debug_label("sibling");
        manager.root_scope().attach_node(&policy);
        manager.root_scope().attach_node(&sibling);
        policy.attach_node(&policy_child);
        sibling.request_focus();

        policy.set_descendants_are_focusable(false);
        assert!(
            sibling.has_primary_focus(),
            "disabling one subtree must not clear an unrelated sibling"
        );

        manager.root_scope().detach_node(policy.id());
        manager.root_scope().detach_node(sibling.id());
        manager.unfocus();
    }
}
