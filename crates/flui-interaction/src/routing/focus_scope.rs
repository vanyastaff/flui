//! Owner-affine focus tree and traversal primitives.
//!
//! Focus nodes form a tree parallel to the view tree. A node is created
//! unbound, then acquires the exact [`crate::FocusManager`] owner when its
//! subtree is attached below that manager's root scope. There is no ambient
//! focus manager and no registry keyed by node IDs.

use std::{
    cell::{Cell, RefCell},
    cmp::Ordering,
    collections::VecDeque,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
};

use flui_foundation::ListenerId;
use flui_types::geometry::{Pixels, Rect};
use thiserror::Error;

use crate::{FocusManager, events::KeyEvent};

pub use crate::ids::FocusNodeId;

static NEXT_FOCUS_NODE_ID: AtomicU64 = AtomicU64::new(1);

fn allocate_focus_node_id() -> FocusNodeId {
    FocusNodeId::new(NEXT_FOCUS_NODE_ID.fetch_add(1, AtomicOrdering::Relaxed))
}

/// Owner-local callback for handling key events.
pub type KeyEventHandler = Rc<dyn Fn(&KeyEvent) -> KeyEventResult>;

/// Computes a node's bounding rectangle on demand, in root coordinates.
pub type RectProvider = Rc<dyn Fn() -> Option<Rect<Pixels>>>;

/// ChangeNotifier-style callback for one focus node.
pub type FocusNodeChangeCallback = Rc<dyn Fn()>;

/// Result of one focus-node key handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventResult {
    /// Stop propagation and consume the event.
    Handled,
    /// Continue to the parent focus node.
    Ignored,
    /// Stop propagation without consuming the event.
    SkipRemainingHandlers,
}

impl KeyEventResult {
    /// Combine several handler channels on one node using Flutter semantics.
    #[must_use]
    pub fn combine(self, other: Self) -> Self {
        use KeyEventResult::{Handled, Ignored, SkipRemainingHandlers};
        match (self, other) {
            (Handled, _) | (_, Handled) => Handled,
            (SkipRemainingHandlers, _) | (_, SkipRemainingHandlers) => SkipRemainingHandlers,
            (Ignored, Ignored) => Ignored,
        }
    }
}

/// A structural focus-tree mutation failed.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FocusTreeError {
    /// The mutation would create a parent cycle.
    #[error(
        "focus node {child:?} cannot be attached below {parent:?}: the edge would create a cycle"
    )]
    Cycle {
        /// Proposed parent.
        parent: FocusNodeId,
        /// Proposed child.
        child: FocusNodeId,
    },
    /// A live node is already attached and must be reparented explicitly.
    #[error("focus node {node:?} is already attached below {parent:?}")]
    AlreadyAttached {
        /// Attached node.
        node: FocusNodeId,
        /// Current parent.
        parent: FocusNodeId,
    },
    /// Parent and child belong to different focus managers.
    #[error("focus subtree rooted at {node:?} belongs to a different FocusManager")]
    ManagerMismatch {
        /// Root of the mismatched subtree.
        node: FocusNodeId,
    },
    /// The manager was closed and its nodes are permanently retired.
    #[error("focus node {node:?} belongs to a closed FocusManager")]
    OwnerClosed {
        /// Retired node.
        node: FocusNodeId,
    },
    /// An attachment was superseded by a later attach or reparent operation.
    #[error("focus attachment for node {node:?} is stale")]
    StaleAttachment {
        /// Node whose generation no longer matches.
        node: FocusNodeId,
    },
    /// A replacement node is already bound or attached to a focus tree.
    #[error("replacement focus node {replacement:?} is already attached or manager-bound")]
    ReplacementAttached {
        /// Replacement that is unavailable for a fresh attachment.
        replacement: FocusNodeId,
    },
    /// A replacement node already owns children.
    #[error("replacement focus node {replacement:?} must not have children")]
    ReplacementNotEmpty {
        /// Replacement whose existing subtree would make ownership ambiguous.
        replacement: FocusNodeId,
    },
    /// Ordinary nodes and scope backing nodes cannot replace each other.
    #[error(
        "focus node {current:?} and replacement {replacement:?} have incompatible concrete kinds"
    )]
    ReplacementKindMismatch {
        /// Currently attached node.
        current: FocusNodeId,
        /// Proposed replacement.
        replacement: FocusNodeId,
    },
}

/// Result of a node-level focus request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusRequestOutcome {
    /// The node became primary focus.
    Focused,
    /// The node is detached; the request will be fulfilled when it attaches.
    Queued,
    /// The node or one of its ancestors currently refuses focus.
    Rejected,
    /// The node belongs to a manager that has been closed.
    OwnerClosed,
}

/// Result of detaching through a [`FocusAttachment`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDetachOutcome {
    /// The live attachment was detached.
    Detached,
    /// The handle no longer describes the node's current attachment.
    Stale,
    /// The owner closed and retired the node.
    OwnerClosed,
}

#[derive(Debug)]
enum ManagerBinding {
    Unbound,
    Bound(Weak<FocusManager>),
    Closed,
}

/// Generation-checked ownership of one focus-tree attachment.
///
/// The handle is intentionally not `Clone`: a widget owns one current
/// attachment. Reparenting updates this handle's generation; any older handle
/// is unable to detach the new mount.
pub struct FocusAttachment {
    node: Weak<FocusNode>,
    node_id: FocusNodeId,
    generation: Cell<u64>,
}

impl std::fmt::Debug for FocusAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusAttachment")
            .field("node", &self.node.upgrade().map(|node| node.id()))
            .field("node_id", &self.node_id)
            .field("generation", &self.generation.get())
            .finish()
    }
}

impl FocusAttachment {
    fn current(node: &Rc<FocusNode>) -> Self {
        Self {
            node: Rc::downgrade(node),
            node_id: node.id(),
            generation: Cell::new(node.attachment_generation.get()),
        }
    }

    /// Whether this handle still describes a live attachment.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        let Some(node) = self.node.upgrade() else {
            return false;
        };
        node.is_attached() && node.attachment_generation.get() == self.generation.get()
    }

    /// Move the attached subtree below `parent` without dropping focus.
    pub fn reparent(&self, parent: &Rc<FocusNode>) -> Result<(), FocusTreeError> {
        let Some(node) = self.node.upgrade() else {
            return Err(FocusTreeError::StaleAttachment { node: self.node_id });
        };
        self.ensure_current(&node)?;
        parent.adopt_node_internal(&node)?;
        self.generation.set(node.attachment_generation.get());
        Ok(())
    }

    /// Atomically replace the node owned by this attachment.
    ///
    /// The replacement takes the current node's exact parent slot and child
    /// subtree. Descendant attachments remain current because their nodes are
    /// neither detached nor assigned a new attachment generation. The current
    /// node is retired from this tree and this handle becomes stale; the
    /// returned handle exclusively owns the replacement attachment.
    ///
    /// `replacement` must be an empty, unbound node of the same concrete kind
    /// (ordinary node or focus-scope backing node) as the current node.
    pub fn replace_node(
        &self,
        replacement: &Rc<FocusNode>,
    ) -> Result<FocusAttachment, FocusTreeError> {
        let Some(current) = self.node.upgrade() else {
            return Err(FocusTreeError::StaleAttachment { node: self.node_id });
        };
        self.ensure_current(&current)?;
        FocusNode::replace_attached_node(&current, replacement)
    }

    /// Detach the current subtree. Repeated or superseded calls are inert.
    pub fn detach(&self) -> FocusDetachOutcome {
        let Some(node) = self.node.upgrade() else {
            return FocusDetachOutcome::Stale;
        };
        if matches!(*node.manager_binding.borrow(), ManagerBinding::Closed) {
            return FocusDetachOutcome::OwnerClosed;
        }
        if node.manager().is_some_and(|manager| manager.is_closed()) {
            return FocusDetachOutcome::OwnerClosed;
        }
        if node.attachment_generation.get() != self.generation.get() || !node.is_attached() {
            return FocusDetachOutcome::Stale;
        }
        let Some(parent) = node.parent() else {
            return FocusDetachOutcome::Stale;
        };
        parent.remove_child(&node);
        FocusDetachOutcome::Detached
    }

    fn ensure_current(&self, node: &FocusNode) -> Result<(), FocusTreeError> {
        if matches!(*node.manager_binding.borrow(), ManagerBinding::Closed) {
            return Err(FocusTreeError::OwnerClosed { node: node.id() });
        }
        if node.manager().is_some_and(|manager| manager.is_closed()) {
            return Err(FocusTreeError::OwnerClosed { node: node.id() });
        }
        if node.attachment_generation.get() != self.generation.get() || !node.is_attached() {
            return Err(FocusTreeError::StaleAttachment { node: node.id() });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusNodeRegistrationKind {
    KeyHandler,
    RectProvider,
}

/// Generation-checked ownership of one replaceable [`FocusNode`] property.
///
/// A registration is returned by
/// [`FocusNode::register_on_key_event`] or
/// [`FocusNode::register_rect_provider`]. Dropping it clears the installed
/// value only when no later writer has replaced that property. This lets a
/// widget clean up the callback it installed without erasing newer
/// caller-owned state on a hosted external node.
///
/// Registrations are intentionally neither cloneable nor reusable: exactly
/// one token owns each installed generation.
#[must_use = "dropping the registration immediately removes the installed focus-node property"]
pub struct FocusNodeRegistration {
    node: Weak<FocusNode>,
    node_id: FocusNodeId,
    generation: u64,
    kind: FocusNodeRegistrationKind,
    armed: bool,
}

impl FocusNodeRegistration {
    fn new(node: &Rc<FocusNode>, generation: u64, kind: FocusNodeRegistrationKind) -> Self {
        Self {
            node: Rc::downgrade(node),
            node_id: node.id(),
            generation,
            kind,
            armed: true,
        }
    }

    /// Whether this token still owns the currently installed property value.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.armed
            && self.node.upgrade().is_some_and(|node| match self.kind {
                FocusNodeRegistrationKind::KeyHandler => {
                    node.on_key_event_generation.get() == self.generation
                }
                FocusNodeRegistrationKind::RectProvider => {
                    node.rect_provider_generation.get() == self.generation
                }
            })
    }

    /// Transfer the installed value to the node's external owner.
    ///
    /// The property remains installed, but dropping this token will no longer
    /// clear it. This is used when a hosted node changes from widget-managed
    /// configuration to source-of-truth external configuration.
    pub fn relinquish(mut self) {
        self.armed = false;
    }

    fn clear_if_current(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        let Some(node) = self.node.upgrade() else {
            return;
        };
        match self.kind {
            FocusNodeRegistrationKind::KeyHandler => {
                node.clear_on_key_event_generation(self.generation);
            }
            FocusNodeRegistrationKind::RectProvider => {
                node.clear_rect_provider_generation(self.generation);
            }
        }
    }
}

impl Drop for FocusNodeRegistration {
    fn drop(&mut self) {
        self.clear_if_current();
    }
}

impl std::fmt::Debug for FocusNodeRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusNodeRegistration")
            .field("node_id", &self.node_id)
            .field("generation", &self.generation)
            .field("kind", &self.kind)
            .field("current", &self.is_current())
            .finish_non_exhaustive()
    }
}

/// A node in the owner-local focus tree.
pub struct FocusNode {
    id: FocusNodeId,
    debug_label: Option<String>,
    parent: RefCell<Option<Weak<FocusNode>>>,
    children: RefCell<Vec<Rc<FocusNode>>>,
    can_request_focus: Cell<bool>,
    skip_traversal: Cell<bool>,
    descendants_are_focusable: Cell<bool>,
    scope_owner: RefCell<Option<Weak<FocusScopeNode>>>,
    on_key_event: RefCell<Option<KeyEventHandler>>,
    listeners: RefCell<Vec<(ListenerId, FocusNodeChangeCallback)>>,
    next_listener_id: Cell<usize>,
    rect: Cell<Rect<Pixels>>,
    rect_provider: RefCell<Option<RectProvider>>,
    rect_provider_generation: Cell<u64>,
    manager_binding: RefCell<ManagerBinding>,
    attached: Cell<bool>,
    pending_focus_request: Cell<bool>,
    attachment_generation: Cell<u64>,
    on_key_event_generation: Cell<u64>,
}

impl FocusNode {
    /// Create an unattached focus node.
    #[must_use]
    pub fn new() -> Rc<Self> {
        Self::create(None, None)
    }

    /// Create an unattached focus node with a diagnostics label.
    #[must_use]
    pub fn with_debug_label(label: impl Into<String>) -> Rc<Self> {
        Self::create(Some(label.into()), None)
    }

    fn create(label: Option<String>, scope_owner: Option<Weak<FocusScopeNode>>) -> Rc<Self> {
        Rc::new(Self {
            id: allocate_focus_node_id(),
            debug_label: label,
            parent: RefCell::new(None),
            children: RefCell::new(Vec::new()),
            can_request_focus: Cell::new(true),
            skip_traversal: Cell::new(false),
            descendants_are_focusable: Cell::new(true),
            scope_owner: RefCell::new(scope_owner),
            on_key_event: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            next_listener_id: Cell::new(1),
            rect: Cell::new(Rect::ZERO),
            rect_provider: RefCell::new(None),
            rect_provider_generation: Cell::new(0),
            manager_binding: RefCell::new(ManagerBinding::Unbound),
            attached: Cell::new(false),
            pending_focus_request: Cell::new(false),
            attachment_generation: Cell::new(1),
            on_key_event_generation: Cell::new(0),
        })
    }

    fn new_scope_backing_node(
        label: Option<String>,
        scope_owner: Weak<FocusScopeNode>,
    ) -> Rc<Self> {
        Self::create(label, Some(scope_owner))
    }

    /// Stable diagnostics identity for this node.
    #[inline]
    pub fn id(&self) -> FocusNodeId {
        self.id
    }

    /// Optional diagnostics label.
    #[inline]
    pub fn debug_label(&self) -> Option<&str> {
        self.debug_label.as_deref()
    }

    /// Whether this node and its ancestors currently allow a focus request.
    #[inline]
    pub fn can_request_focus(&self) -> bool {
        self.own_can_request_focus()
            && self
                .ancestors()
                .all(|ancestor| ancestor.allows_descendant_focus())
    }

    /// Change this node's focus eligibility.
    pub fn set_can_request_focus(&self, can_request_focus: bool) {
        if self.can_request_focus.replace(can_request_focus) == can_request_focus {
            return;
        }
        if !can_request_focus
            && (self.has_primary_focus() || (self.is_scope() && self.has_focus()))
            && let Some(manager) = self.manager()
        {
            manager.unfocus();
        }
        self.notify_listeners();
        if can_request_focus {
            if self.is_scope() {
                for child in self.children() {
                    Self::fulfill_pending_first_focus_subtree(&child);
                }
            }
            self.fulfill_pending_first_focus_ancestors();
        }
    }

    /// Whether traversal skips this node.
    #[inline]
    pub fn skip_traversal(&self) -> bool {
        self.skip_traversal.get()
    }

    /// Change whether traversal skips this node.
    pub fn set_skip_traversal(&self, skip: bool) {
        if self.skip_traversal.replace(skip) != skip {
            self.notify_listeners();
            if !skip {
                self.fulfill_pending_first_focus_ancestors();
            }
        }
    }

    /// Whether descendants may receive focus.
    #[inline]
    pub fn descendants_are_focusable(&self) -> bool {
        self.descendants_are_focusable.get()
    }

    /// Change descendant focus eligibility.
    pub fn set_descendants_are_focusable(&self, focusable: bool) {
        if self.descendants_are_focusable.replace(focusable) == focusable {
            return;
        }
        if !focusable
            && self.has_focus()
            && let Some(manager) = self.manager()
        {
            manager.unfocus();
        }
        self.notify_listeners();
        if focusable {
            for child in self.children() {
                Self::fulfill_pending_first_focus_subtree(&child);
            }
            self.fulfill_pending_first_focus_ancestors();
        }
    }

    /// Whether the node currently belongs to a live manager tree.
    #[inline]
    pub fn is_attached(&self) -> bool {
        self.attached.get()
    }

    /// Current parent, if any.
    pub fn parent(&self) -> Option<Rc<FocusNode>> {
        self.parent.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// Snapshot the child list.
    pub fn children(&self) -> Vec<Rc<FocusNode>> {
        self.children.borrow().clone()
    }

    /// Current traversal geometry.
    pub fn rect(&self) -> Rect<Pixels> {
        let provider = self.rect_provider.borrow().clone();
        if let Some(provider) = provider
            && let Some(rect) = provider()
        {
            return rect;
        }
        self.rect.get()
    }

    /// Store fallback traversal geometry.
    pub fn set_rect(&self, rect: Rect<Pixels>) {
        self.rect.set(rect);
    }

    /// Install a live traversal-geometry source.
    pub fn set_rect_provider(&self, provider: RectProvider) {
        self.replace_rect_provider(Some(provider));
    }

    /// Remove the live traversal-geometry source.
    pub fn clear_rect_provider(&self) {
        self.replace_rect_provider(None);
    }

    /// Install a live traversal-geometry source with generation-checked
    /// cleanup ownership.
    ///
    /// Dropping the returned registration clears `provider` only if no later
    /// writer has replaced the node's geometry source.
    pub fn register_rect_provider(
        self: &Rc<Self>,
        provider: RectProvider,
    ) -> FocusNodeRegistration {
        let generation = self.replace_rect_provider(Some(provider));
        FocusNodeRegistration::new(self, generation, FocusNodeRegistrationKind::RectProvider)
    }

    /// Install this node's key handler.
    pub fn set_on_key_event(&self, handler: KeyEventHandler) {
        self.replace_on_key_event(Some(handler));
    }

    /// Clear this node's key handler.
    pub fn clear_on_key_event(&self) {
        self.replace_on_key_event(None);
    }

    /// Install this node's key handler with generation-checked cleanup
    /// ownership.
    ///
    /// Dropping the returned registration clears `handler` only if no later
    /// writer has replaced the node's key-handler slot.
    pub fn register_on_key_event(
        self: &Rc<Self>,
        handler: KeyEventHandler,
    ) -> FocusNodeRegistration {
        let generation = self.replace_on_key_event(Some(handler));
        FocusNodeRegistration::new(self, generation, FocusNodeRegistrationKind::KeyHandler)
    }

    fn replace_rect_provider(&self, provider: Option<RectProvider>) -> u64 {
        let generation = Self::next_property_generation(&self.rect_provider_generation);
        *self.rect_provider.borrow_mut() = provider;
        generation
    }

    fn clear_rect_provider_generation(&self, generation: u64) {
        if self.rect_provider_generation.get() == generation {
            self.replace_rect_provider(None);
        }
    }

    fn replace_on_key_event(&self, handler: Option<KeyEventHandler>) -> u64 {
        let generation = Self::next_property_generation(&self.on_key_event_generation);
        *self.on_key_event.borrow_mut() = handler;
        generation
    }

    fn clear_on_key_event_generation(&self, generation: u64) {
        if self.on_key_event_generation.get() == generation {
            self.replace_on_key_event(None);
        }
    }

    fn next_property_generation(generation: &Cell<u64>) -> u64 {
        let next = generation
            .get()
            .checked_add(1)
            .expect("BUG: focus-node property generation exhausted");
        generation.set(next);
        next
    }

    /// Register a listener for focus or focusability changes on this node.
    pub fn add_listener(&self, callback: FocusNodeChangeCallback) -> ListenerId {
        let id = ListenerId::new(self.next_listener_id.get());
        let next = self
            .next_listener_id
            .get()
            .checked_add(1)
            .expect("BUG: focus-node listener ID space exhausted");
        self.next_listener_id.set(next);
        self.listeners.borrow_mut().push((id, callback));
        id
    }

    /// Remove one node listener.
    pub fn remove_listener(&self, id: ListenerId) {
        self.listeners.borrow_mut().retain(|(held, _)| *held != id);
    }

    /// Number of node listeners, for deterministic lifecycle tests.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn listener_count(&self) -> usize {
        self.listeners.borrow().len()
    }

    pub(crate) fn notify_listeners(&self) {
        if self.parent.borrow().is_none() {
            return;
        }
        self.notify_listeners_after_tree_change();
    }

    pub(crate) fn notify_listeners_after_tree_change(&self) {
        let listeners = self.listeners.borrow().clone();
        for (_, listener) in listeners {
            listener();
        }
    }

    /// Whether this node or one of its descendants has primary focus.
    pub fn has_focus(&self) -> bool {
        if !self.is_attached() {
            return false;
        }
        let Some(manager) = self.manager() else {
            return false;
        };
        let Some(primary) = manager.primary_focus() else {
            return false;
        };
        self.id == primary.id() || self.has_descendant_node(&primary)
    }

    /// Whether this exact node has primary focus.
    pub fn has_primary_focus(&self) -> bool {
        if !self.is_attached() {
            return false;
        }
        self.manager()
            .and_then(|manager| manager.primary_focus())
            .is_some_and(|primary| primary.id() == self.id)
    }

    /// Nearest enclosing focus scope.
    pub fn enclosing_scope(&self) -> Option<Rc<FocusScopeNode>> {
        let mut current = self.parent();
        while let Some(node) = current {
            if let Some(scope) = node.as_scope() {
                return Some(scope);
            }
            current = node.parent();
        }
        None
    }

    /// This node's scope owner when it is a scope backing node.
    pub fn as_scope(&self) -> Option<Rc<FocusScopeNode>> {
        self.scope_owner.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// Whether this node backs a [`FocusScopeNode`].
    pub fn is_scope(&self) -> bool {
        self.scope_owner
            .borrow()
            .as_ref()
            .is_some_and(|owner| owner.strong_count() > 0)
    }

    /// Request focus, queueing the request while detached.
    pub fn request_focus(self: &Rc<Self>) -> FocusRequestOutcome {
        if !self.can_request_focus() {
            return FocusRequestOutcome::Rejected;
        }
        match &*self.manager_binding.borrow() {
            ManagerBinding::Unbound => {
                self.pending_focus_request.set(true);
                FocusRequestOutcome::Queued
            }
            ManagerBinding::Closed => FocusRequestOutcome::OwnerClosed,
            ManagerBinding::Bound(manager) => {
                let Some(manager) = manager.upgrade() else {
                    return FocusRequestOutcome::OwnerClosed;
                };
                if manager.is_closed() {
                    return FocusRequestOutcome::OwnerClosed;
                }
                if manager.request_focus(self) {
                    FocusRequestOutcome::Focused
                } else {
                    FocusRequestOutcome::Rejected
                }
            }
        }
    }

    /// Release focus when this node is primary.
    pub fn unfocus(&self) {
        if self.has_primary_focus()
            && let Some(manager) = self.manager()
        {
            manager.unfocus();
        }
    }

    /// Move to the next focusable node in the enclosing scope.
    pub fn next_focus(self: &Rc<Self>) -> bool {
        self.enclosing_scope()
            .is_some_and(|scope| scope.focus_next_in_scope(self))
    }

    /// Move to the previous focusable node in the enclosing scope.
    pub fn previous_focus(self: &Rc<Self>) -> bool {
        self.enclosing_scope()
            .is_some_and(|scope| scope.focus_previous_in_scope(self))
    }

    /// Invoke this node's key handler.
    pub fn handle_key_event(&self, event: &KeyEvent) -> KeyEventResult {
        let handler = self.on_key_event.borrow().clone();
        handler.map_or(KeyEventResult::Ignored, |handler| handler(event))
    }

    /// Iterate parent-first over ancestors.
    pub fn ancestors(&self) -> impl Iterator<Item = Rc<FocusNode>> {
        AncestorIterator {
            current: self.parent(),
        }
    }

    /// Iterate depth-first over descendants.
    pub fn descendants(&self) -> impl Iterator<Item = Rc<FocusNode>> {
        DescendantIterator {
            stack: self.children(),
        }
    }

    /// Depth in the focus tree.
    pub fn depth(&self) -> usize {
        self.ancestors().count()
    }

    /// Attach an unbound subtree below this node.
    pub fn attach_node(
        self: &Rc<Self>,
        child: &Rc<FocusNode>,
    ) -> Result<FocusAttachment, FocusTreeError> {
        self.attach_child(child)
    }

    /// Move a live subtree below this node without dropping primary focus.
    pub fn adopt_node(
        self: &Rc<Self>,
        node: &Rc<FocusNode>,
    ) -> Result<FocusAttachment, FocusTreeError> {
        self.adopt_node_internal(node)?;
        Ok(FocusAttachment::current(node))
    }

    pub(crate) fn manager(&self) -> Option<Rc<FocusManager>> {
        match &*self.manager_binding.borrow() {
            ManagerBinding::Bound(manager) => manager.upgrade(),
            ManagerBinding::Unbound | ManagerBinding::Closed => None,
        }
    }

    fn has_descendant_node(&self, needle: &Rc<FocusNode>) -> bool {
        self.children
            .borrow()
            .iter()
            .any(|child| Rc::ptr_eq(child, needle) || child.has_descendant_node(needle))
    }

    fn has_descendant_id(&self, id: FocusNodeId) -> bool {
        self.children
            .borrow()
            .iter()
            .any(|child| child.id == id || child.has_descendant_id(id))
    }

    fn attach_child(
        self: &Rc<Self>,
        child: &Rc<FocusNode>,
    ) -> Result<FocusAttachment, FocusTreeError> {
        self.validate_edge(child)?;
        let expected_manager = self.expected_manager()?;
        Self::validate_subtree_owner(child, expected_manager.as_ref())?;
        if let Some(parent) = child.parent() {
            if Rc::ptr_eq(&parent, self) {
                return Err(FocusTreeError::AlreadyAttached {
                    node: child.id(),
                    parent: self.id(),
                });
            }
            return Err(FocusTreeError::AlreadyAttached {
                node: child.id(),
                parent: parent.id(),
            });
        }

        *child.parent.borrow_mut() = Some(Rc::downgrade(self));
        self.children.borrow_mut().push(Rc::clone(child));

        if let Some(manager) = expected_manager {
            Self::bind_subtree(child, &manager);
            Self::fulfill_pending_subtree(child);
        }
        child.bump_attachment_generation();
        Self::fulfill_pending_first_focus_subtree(child);
        self.fulfill_pending_first_focus_ancestors();
        Ok(FocusAttachment::current(child))
    }

    fn replace_attached_node(
        current: &Rc<FocusNode>,
        replacement: &Rc<FocusNode>,
    ) -> Result<FocusAttachment, FocusTreeError> {
        match &*replacement.manager_binding.borrow() {
            ManagerBinding::Unbound
                if !replacement.is_attached() && replacement.parent().is_none() => {}
            ManagerBinding::Unbound | ManagerBinding::Bound(_) => {
                return Err(FocusTreeError::ReplacementAttached {
                    replacement: replacement.id(),
                });
            }
            ManagerBinding::Closed => {
                return Err(FocusTreeError::OwnerClosed {
                    node: replacement.id(),
                });
            }
        }
        if !replacement.children.borrow().is_empty() {
            return Err(FocusTreeError::ReplacementNotEmpty {
                replacement: replacement.id(),
            });
        }
        if current.scope_owner.borrow().is_some() != replacement.scope_owner.borrow().is_some() {
            return Err(FocusTreeError::ReplacementKindMismatch {
                current: current.id(),
                replacement: replacement.id(),
            });
        }

        let manager = current
            .manager()
            .ok_or(FocusTreeError::OwnerClosed { node: current.id() })?;
        let parent = current
            .parent()
            .ok_or(FocusTreeError::StaleAttachment { node: current.id() })?;
        let sibling_index = parent
            .children
            .borrow()
            .iter()
            .position(|child| Rc::ptr_eq(child, current))
            .ok_or(FocusTreeError::StaleAttachment { node: current.id() })?;

        let focused = manager
            .primary_focus()
            .filter(|primary| Rc::ptr_eq(primary, current) || current.has_descendant_node(primary));
        let previous_focus_path = focused.as_ref().map(|primary| {
            std::iter::once(Rc::clone(primary))
                .chain(primary.ancestors())
                .collect::<Vec<_>>()
        });
        if focused
            .as_ref()
            .is_some_and(|primary| Rc::ptr_eq(primary, current))
        {
            manager.clear_primary_for_node_replacement(current);
        }

        let children = std::mem::take(&mut *current.children.borrow_mut());
        for child in &children {
            *child.parent.borrow_mut() = Some(Rc::downgrade(replacement));
        }
        *replacement.children.borrow_mut() = children;

        parent.children.borrow_mut()[sibling_index] = Rc::clone(replacement);
        *replacement.parent.borrow_mut() = Some(Rc::downgrade(&parent));
        *replacement.manager_binding.borrow_mut() = ManagerBinding::Bound(Rc::downgrade(&manager));
        replacement.attached.set(true);
        replacement.bump_attachment_generation();

        current.parent.borrow_mut().take();
        current.attached.set(false);
        *current.manager_binding.borrow_mut() = ManagerBinding::Unbound;
        current.bump_attachment_generation();
        if let Some(scope) = current.as_scope() {
            scope.clear_replaced_state();
        }

        if let Some(scope) = parent.as_scope().or_else(|| parent.enclosing_scope()) {
            scope.forget_subtree(current);
        }
        if let Some(primary) = focused.as_ref()
            && !primary.can_request_focus()
        {
            manager.clear_primary_for_node_replacement(primary);
        }

        let replacement_attachment = FocusAttachment::current(replacement);
        if let (Some(previous_primary), Some(previous_focus_path)) = (focused, previous_focus_path)
        {
            manager.finish_node_replacement(previous_primary, previous_focus_path);
        }

        if replacement_attachment.is_attached() {
            Self::fulfill_pending_subtree(replacement);
            Self::fulfill_pending_first_focus_subtree(replacement);
            replacement.fulfill_pending_first_focus_ancestors();
        }

        Ok(replacement_attachment)
    }

    fn adopt_node_internal(self: &Rc<Self>, node: &Rc<FocusNode>) -> Result<(), FocusTreeError> {
        self.validate_edge(node)?;
        if let Some(parent) = node.parent() {
            if Rc::ptr_eq(&parent, self) {
                node.bump_attachment_generation();
                Self::fulfill_pending_first_focus_subtree(node);
                self.fulfill_pending_first_focus_ancestors();
                return Ok(());
            }

            let expected_manager = self.expected_manager()?;
            Self::validate_subtree_owner(node, expected_manager.as_ref())?;
            let manager = node.manager();
            let focused = manager
                .as_ref()
                .and_then(|manager| manager.primary_focus())
                .filter(|primary| Rc::ptr_eq(primary, node) || node.has_descendant_node(primary));

            parent
                .children
                .borrow_mut()
                .retain(|held| !Rc::ptr_eq(held, node));
            if let Some(old_scope) = parent.as_scope().or_else(|| parent.enclosing_scope()) {
                old_scope.forget_subtree(node);
            }

            *node.parent.borrow_mut() = Some(Rc::downgrade(self));
            self.children.borrow_mut().push(Rc::clone(node));
            node.bump_attachment_generation();

            if let Some(primary) = focused {
                FocusManager::refresh_focus_history(&primary);
            }
            Self::fulfill_pending_first_focus_subtree(node);
            self.fulfill_pending_first_focus_ancestors();
            return Ok(());
        }

        self.attach_child(node).map(|_| ())
    }

    fn validate_edge(&self, child: &FocusNode) -> Result<(), FocusTreeError> {
        if self.id == child.id || child.has_descendant_id(self.id) {
            return Err(FocusTreeError::Cycle {
                parent: self.id,
                child: child.id,
            });
        }
        Ok(())
    }

    fn expected_manager(&self) -> Result<Option<Rc<FocusManager>>, FocusTreeError> {
        match &*self.manager_binding.borrow() {
            ManagerBinding::Unbound => Ok(None),
            ManagerBinding::Bound(manager) => manager
                .upgrade()
                .filter(|manager| !manager.is_closed())
                .map(Some)
                .ok_or(FocusTreeError::OwnerClosed { node: self.id }),
            ManagerBinding::Closed => Err(FocusTreeError::OwnerClosed { node: self.id }),
        }
    }

    fn validate_subtree_owner(
        node: &Rc<FocusNode>,
        expected: Option<&Rc<FocusManager>>,
    ) -> Result<(), FocusTreeError> {
        match &*node.manager_binding.borrow() {
            ManagerBinding::Unbound => {}
            ManagerBinding::Closed => {
                return Err(FocusTreeError::OwnerClosed { node: node.id() });
            }
            ManagerBinding::Bound(actual) => {
                let Some(actual) = actual.upgrade() else {
                    return Err(FocusTreeError::OwnerClosed { node: node.id() });
                };
                if actual.is_closed() {
                    return Err(FocusTreeError::OwnerClosed { node: node.id() });
                }
                if expected.is_none_or(|expected| !Rc::ptr_eq(&actual, expected)) {
                    return Err(FocusTreeError::ManagerMismatch { node: node.id() });
                }
            }
        }
        for child in node.children() {
            Self::validate_subtree_owner(&child, expected)?;
        }
        Ok(())
    }

    fn bind_subtree(node: &Rc<FocusNode>, manager: &Rc<FocusManager>) {
        *node.manager_binding.borrow_mut() = ManagerBinding::Bound(Rc::downgrade(manager));
        node.attached.set(true);
        for child in node.children() {
            Self::bind_subtree(&child, manager);
        }
    }

    fn fulfill_pending_subtree(node: &Rc<FocusNode>) {
        if node.pending_focus_request.replace(false) {
            node.request_focus();
        }
        for child in node.children() {
            Self::fulfill_pending_subtree(&child);
        }
    }

    /// Retry first-focus intents in scopes rooted inside `node`.
    fn fulfill_pending_first_focus_subtree(node: &Rc<FocusNode>) {
        if let Some(scope) = node.as_scope() {
            scope.fulfill_pending_first_focus();
        }
        for child in node.children() {
            Self::fulfill_pending_first_focus_subtree(&child);
        }
    }

    /// Retry first-focus intents whose scope contains this node.
    fn fulfill_pending_first_focus_ancestors(&self) {
        if let Some(scope) = self.as_scope() {
            scope.fulfill_pending_first_focus();
        }
        for ancestor in self.ancestors() {
            if let Some(scope) = ancestor.as_scope() {
                scope.fulfill_pending_first_focus();
            }
        }
    }

    fn remove_child(&self, child: &Rc<FocusNode>) {
        let was_child = self
            .children
            .borrow()
            .iter()
            .any(|held| Rc::ptr_eq(held, child));
        if !was_child {
            return;
        }

        if let Some(manager) = child.manager()
            && let Some(primary) = manager.primary_focus()
            && (Rc::ptr_eq(&primary, child) || child.has_descendant_node(&primary))
        {
            manager.unfocus();
        }

        self.children
            .borrow_mut()
            .retain(|held| !Rc::ptr_eq(held, child));
        child.parent.borrow_mut().take();
        Self::unbind_subtree(child);

        if let Some(scope) = self.as_scope().or_else(|| self.enclosing_scope()) {
            scope.forget_subtree(child);
        }
    }

    fn unbind_subtree(node: &Rc<FocusNode>) {
        node.attached.set(false);
        *node.manager_binding.borrow_mut() = ManagerBinding::Unbound;
        node.bump_attachment_generation();
        for child in node.children() {
            Self::unbind_subtree(&child);
        }
    }

    pub(crate) fn close_owned_tree(node: &Rc<FocusNode>) {
        let children = std::mem::take(&mut *node.children.borrow_mut());
        for child in children {
            child.parent.borrow_mut().take();
            Self::tombstone_subtree(&child);
        }
        Self::tombstone_node(node);
    }

    fn tombstone_subtree(node: &Rc<FocusNode>) {
        let children = std::mem::take(&mut *node.children.borrow_mut());
        for child in children {
            child.parent.borrow_mut().take();
            Self::tombstone_subtree(&child);
        }
        Self::tombstone_node(node);
    }

    fn tombstone_node(node: &Rc<FocusNode>) {
        node.attached.set(false);
        node.pending_focus_request.set(false);
        *node.manager_binding.borrow_mut() = ManagerBinding::Closed;
        node.bump_attachment_generation();
        node.clear_on_key_event();
        node.listeners.borrow_mut().clear();
        node.clear_rect_provider();
        if let Some(scope) = node.as_scope() {
            scope.pending_first_focus.set(false);
            scope.focus_history.borrow_mut().clear();
        }
    }

    fn bump_attachment_generation(&self) {
        let next = self
            .attachment_generation
            .get()
            .checked_add(1)
            .expect("BUG: focus attachment generation exhausted");
        self.attachment_generation.set(next);
    }

    fn own_can_request_focus(&self) -> bool {
        self.can_request_focus.get()
    }

    fn allows_descendant_focus(&self) -> bool {
        self.descendants_are_focusable() && (!self.is_scope() || self.own_can_request_focus())
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
            .field("children_count", &self.children.borrow().len())
            .finish_non_exhaustive()
    }
}

struct AncestorIterator {
    current: Option<Rc<FocusNode>>,
}

impl Iterator for AncestorIterator {
    type Item = Rc<FocusNode>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.current.take()?;
        self.current = node.parent();
        Some(node)
    }
}

struct DescendantIterator {
    stack: Vec<Rc<FocusNode>>,
}

impl Iterator for DescendantIterator {
    type Item = Rc<FocusNode>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        for child in node.children().into_iter().rev() {
            self.stack.push(child);
        }
        Some(node)
    }
}

/// A scope groups descendants, constrains traversal, and remembers focus.
pub struct FocusScopeNode {
    inner: Rc<FocusNode>,
    focus_history: RefCell<VecDeque<Weak<FocusNode>>>,
    /// An explicit first-focus request made before this scope had an eligible
    /// descendant. The scope parks primary focus on its backing node while
    /// this is set, then consumes the intent exactly once when a descendant
    /// becomes eligible.
    pending_first_focus: Cell<bool>,
    autofocus: Cell<bool>,
    traps_focus: Cell<bool>,
    traversal_policy: RefCell<Rc<dyn FocusTraversalPolicy>>,
    traversal_edge_behavior: Cell<TraversalEdgeBehavior>,
}

impl FocusScopeNode {
    /// Create an unattached focus scope.
    #[must_use]
    pub fn new() -> Rc<Self> {
        Self::create(None)
    }

    /// Create an unattached focus scope with a diagnostics label.
    #[must_use]
    pub fn with_debug_label(label: impl Into<String>) -> Rc<Self> {
        Self::create(Some(label.into()))
    }

    fn create(label: Option<String>) -> Rc<Self> {
        Rc::new_cyclic(|owner| Self {
            inner: FocusNode::new_scope_backing_node(label, owner.clone()),
            focus_history: RefCell::new(VecDeque::new()),
            pending_first_focus: Cell::new(false),
            autofocus: Cell::new(false),
            traps_focus: Cell::new(false),
            traversal_policy: RefCell::new(Rc::new(ReadingOrderPolicy)),
            traversal_edge_behavior: Cell::new(TraversalEdgeBehavior::default()),
        })
    }

    pub(crate) fn new_root(manager: Weak<FocusManager>) -> Rc<Self> {
        let scope = Self::with_debug_label("Root Focus Scope");
        *scope.inner.manager_binding.borrow_mut() = ManagerBinding::Bound(manager);
        scope.inner.attached.set(true);
        scope
    }

    /// The backing focus node used in tree edges.
    #[inline]
    pub fn as_focus_node(&self) -> &Rc<FocusNode> {
        &self.inner
    }

    /// Diagnostics identity of the backing node.
    #[inline]
    pub fn id(&self) -> FocusNodeId {
        self.inner.id()
    }

    /// Whether this scope should focus its first descendant after attach.
    #[inline]
    pub fn autofocus(&self) -> bool {
        self.autofocus.get()
    }

    /// Change autofocus behavior.
    pub fn set_autofocus(&self, autofocus: bool) {
        self.autofocus.set(autofocus);
    }

    /// Whether focus is trapped inside this scope.
    #[inline]
    pub fn traps_focus(&self) -> bool {
        self.traps_focus.get()
    }

    /// Change whether focus is trapped inside this scope.
    pub fn set_traps_focus(&self, traps: bool) {
        self.traps_focus.set(traps);
    }

    /// Replace this scope's owner-local traversal policy.
    pub fn set_traversal_policy(&self, policy: Rc<dyn FocusTraversalPolicy>) {
        *self.traversal_policy.borrow_mut() = policy;
    }

    /// Most recently focused structurally live descendant.
    ///
    /// A temporarily detached subtree remains eligible history: requesting
    /// first focus while it is offline queues the remembered node, and
    /// reattachment fulfills that request. Only removal from this scope or
    /// deterministic owner retirement invalidates the entry.
    pub fn focused_child(&self) -> Option<Rc<FocusNode>> {
        let mut history = self.focus_history.borrow_mut();
        while let Some(candidate) = history.front() {
            match candidate.upgrade() {
                Some(node)
                    if !matches!(*node.manager_binding.borrow(), ManagerBinding::Closed)
                        && self.inner.has_descendant_node(&node) =>
                {
                    return Some(node);
                }
                Some(_) | None => {
                    history.pop_front();
                }
            }
        }
        None
    }

    /// Attach an unbound subtree below this scope.
    pub fn attach_node(
        self: &Rc<Self>,
        node: &Rc<FocusNode>,
    ) -> Result<FocusAttachment, FocusTreeError> {
        self.inner.attach_child(node)
    }

    /// Move a live subtree below this scope without dropping focus.
    pub fn adopt_node(
        self: &Rc<Self>,
        node: &Rc<FocusNode>,
    ) -> Result<FocusAttachment, FocusTreeError> {
        self.inner.adopt_node(node)
    }

    /// Current edge behavior.
    pub fn traversal_edge_behavior(&self) -> TraversalEdgeBehavior {
        self.traversal_edge_behavior.get()
    }

    /// Set what traversal does at this scope's edge.
    pub fn set_traversal_edge_behavior(&self, behavior: TraversalEdgeBehavior) {
        self.traversal_edge_behavior.set(behavior);
    }

    /// Restore the remembered descendant, or focus the first policy-ordered
    /// descendant.
    ///
    /// If no eligible descendant exists yet, the request remains pending and
    /// primary focus is parked on this scope's backing node. The first
    /// descendant that later becomes eligible consumes the pending request.
    /// This lets a route request focus before its lazily built subtree mounts
    /// without a second manager-side "active scope" state.
    pub fn set_first_focus(self: &Rc<Self>) -> bool {
        if matches!(*self.inner.manager_binding.borrow(), ManagerBinding::Closed)
            || self
                .inner
                .manager()
                .is_some_and(|manager| manager.is_closed())
        {
            self.pending_first_focus.set(false);
            return false;
        }

        if let Some(target) = self.preferred_first_focus() {
            self.pending_first_focus.set(false);
            return matches!(
                target.request_focus(),
                FocusRequestOutcome::Focused | FocusRequestOutcome::Queued
            );
        }

        self.pending_first_focus.set(true);
        match self.inner.request_focus() {
            FocusRequestOutcome::Focused
            | FocusRequestOutcome::Queued
            | FocusRequestOutcome::Rejected => true,
            FocusRequestOutcome::OwnerClosed => {
                self.pending_first_focus.set(false);
                false
            }
        }
    }

    /// Prefer focus history, recursively following remembered child scopes,
    /// before falling back to this scope's traversal policy.
    fn preferred_first_focus(&self) -> Option<Rc<FocusNode>> {
        if let Some(remembered) = self.focused_child() {
            if let Some(scope) = remembered.as_scope() {
                if let Some(target) = scope.preferred_first_focus() {
                    return Some(target);
                }
            } else if remembered.can_request_focus() {
                // A node focused explicitly may intentionally be skipped by
                // Tab traversal; focus restoration still returns to it.
                return Some(remembered);
            }
        }

        self.sorted_traversal_order(None)
            .into_iter()
            .find(is_traversable)
    }

    /// Attempt an already-queued first-focus intent after a tree or
    /// focusability change.
    fn fulfill_pending_first_focus(self: &Rc<Self>) {
        if !self.pending_first_focus.get() {
            return;
        }
        let Some(target) = self.preferred_first_focus() else {
            return;
        };

        // Clear before the synchronous request: focus listeners may mutate
        // this tree reentrantly. A rejected request restores the intent.
        self.pending_first_focus.set(false);
        if matches!(target.request_focus(), FocusRequestOutcome::Rejected) {
            self.pending_first_focus.set(true);
        }
    }

    fn clear_replaced_state(&self) {
        self.pending_first_focus.set(false);
        self.focus_history.borrow_mut().clear();
    }

    /// Traversal candidates in policy order.
    pub fn sorted_traversal_order(&self, cursor: Option<&Rc<FocusNode>>) -> Vec<Rc<FocusNode>> {
        let mut nodes = self.collect_focusable_nodes();
        if let Some(cursor) = cursor
            && !nodes.iter().any(|node| Rc::ptr_eq(node, cursor))
            && self.inner.has_descendant_node(cursor)
        {
            nodes.push(Rc::clone(cursor));
        }
        self.traversal_policy.borrow().sort_descendants(&nodes)
    }

    /// Resolve one traversal step without applying it.
    pub fn resolve_traversal(
        &self,
        current: Option<&Rc<FocusNode>>,
        forward: bool,
    ) -> ResolvedStep {
        let order = self.sorted_traversal_order(current);

        let Some(current) = current else {
            let target = if forward {
                order.iter().find(|node| is_traversable(node))
            } else {
                order.iter().rev().find(|node| is_traversable(node))
            };
            return target.map_or(ResolvedStep::None, |node| {
                ResolvedStep::Focus(Rc::clone(node))
            });
        };

        let Some(position) = order.iter().position(|node| Rc::ptr_eq(node, current)) else {
            return ResolvedStep::None;
        };

        let target = if forward {
            order[position + 1..]
                .iter()
                .find(|node| is_traversable(node))
        } else {
            order[..position]
                .iter()
                .rev()
                .find(|node| is_traversable(node))
        };
        if let Some(node) = target {
            return ResolvedStep::Focus(Rc::clone(node));
        }

        match self.traversal_edge_behavior() {
            TraversalEdgeBehavior::ParentScope if self.inner.enclosing_scope().is_some() => {
                ResolvedStep::RetryInParent
            }
            TraversalEdgeBehavior::ClosedLoop | TraversalEdgeBehavior::ParentScope => {
                let wrap = if forward {
                    order.iter().find(|node| is_traversable(node))
                } else {
                    order.iter().rev().find(|node| is_traversable(node))
                };
                wrap.map_or(ResolvedStep::None, |node| {
                    ResolvedStep::Focus(Rc::clone(node))
                })
            }
            TraversalEdgeBehavior::Stop => ResolvedStep::None,
            TraversalEdgeBehavior::LeaveFlutterView => ResolvedStep::Unfocus,
        }
    }

    /// Focus the next node in this scope.
    pub fn focus_next_in_scope(&self, current: &Rc<FocusNode>) -> bool {
        self.perform(self.step(Some(current), true))
    }

    /// Focus the previous node in this scope.
    pub fn focus_previous_in_scope(&self, current: &Rc<FocusNode>) -> bool {
        self.perform(self.step(Some(current), false))
    }

    /// Resolve a step, following parent-scope edge behavior.
    pub fn step(&self, current: Option<&Rc<FocusNode>>, forward: bool) -> ResolvedStep {
        let mut scope: Option<Rc<FocusScopeNode>> = None;
        loop {
            let step = scope.as_ref().map_or_else(
                || self.resolve_traversal(current, forward),
                |scope| scope.resolve_traversal(current, forward),
            );
            if !matches!(step, ResolvedStep::RetryInParent) {
                return step;
            }
            let node = scope
                .as_ref()
                .map_or_else(|| Rc::clone(&self.inner), |scope| Rc::clone(&scope.inner));
            let Some(parent) = node.enclosing_scope() else {
                return ResolvedStep::None;
            };
            scope = Some(parent);
        }
    }

    fn perform(&self, step: ResolvedStep) -> bool {
        match step {
            ResolvedStep::Focus(node) => matches!(
                node.request_focus(),
                FocusRequestOutcome::Focused | FocusRequestOutcome::Queued
            ),
            ResolvedStep::Unfocus => {
                if let Some(manager) = self.inner.manager() {
                    manager.unfocus();
                }
                false
            }
            ResolvedStep::None | ResolvedStep::RetryInParent => false,
        }
    }

    pub(crate) fn perform_with_manager(manager: &FocusManager, step: ResolvedStep) -> bool {
        match step {
            ResolvedStep::Focus(node) => manager.request_focus(&node),
            ResolvedStep::Unfocus => {
                manager.unfocus();
                false
            }
            ResolvedStep::None | ResolvedStep::RetryInParent => false,
        }
    }

    /// Record a descendant as most recently focused.
    pub(crate) fn record_focus(&self, node: &Rc<FocusNode>) {
        // Any descendant focus satisfies a queued "first focus" request,
        // including an explicitly focused node excluded from traversal.
        self.pending_first_focus.set(false);
        let mut history = self.focus_history.borrow_mut();
        history.retain(|held| held.upgrade().is_some_and(|held| !Rc::ptr_eq(&held, node)));
        history.push_front(Rc::downgrade(node));
        history.truncate(10);
    }

    fn forget_subtree(&self, subtree: &Rc<FocusNode>) {
        self.focus_history.borrow_mut().retain(|held| {
            held.upgrade().is_some_and(|node| {
                !Rc::ptr_eq(&node, subtree) && !subtree.has_descendant_node(&node)
            })
        });
    }

    fn collect_focusable_nodes(&self) -> Vec<Rc<FocusNode>> {
        self.inner.descendants().filter(is_traversable).collect()
    }
}

impl std::fmt::Debug for FocusScopeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusScopeNode")
            .field("id", &self.id())
            .field("debug_label", &self.inner.debug_label())
            .field("autofocus", &self.autofocus())
            .field("traps_focus", &self.traps_focus())
            .field("pending_first_focus", &self.pending_first_focus.get())
            .field("focused_child", &self.focused_child().map(|node| node.id()))
            .finish_non_exhaustive()
    }
}

/// What traversal does when it runs off a scope edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraversalEdgeBehavior {
    /// Wrap to the other end.
    #[default]
    ClosedLoop,
    /// Release focus so the host can move outside this view.
    LeaveFlutterView,
    /// Continue in the enclosing scope.
    ParentScope,
    /// Keep current focus.
    Stop,
}

/// A typed traversal intent.
#[derive(Clone)]
pub enum ResolvedStep {
    /// Move primary focus to this node.
    Focus(Rc<FocusNode>),
    /// Release primary focus.
    Unfocus,
    /// No focus change.
    None,
    /// Re-resolve the step in the enclosing scope.
    RetryInParent,
}

impl std::fmt::Debug for ResolvedStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Focus(node) => f.debug_tuple("Focus").field(&node.id()).finish(),
            Self::Unfocus => f.write_str("Unfocus"),
            Self::None => f.write_str("None"),
            Self::RetryInParent => f.write_str("RetryInParent"),
        }
    }
}

/// Whether Tab may land on `node`.
fn is_traversable(node: &Rc<FocusNode>) -> bool {
    !node.is_scope() && node.can_request_focus() && !node.skip_traversal()
}

/// Orders traversal candidates.
pub trait FocusTraversalPolicy: std::fmt::Debug {
    /// Return `nodes` in policy order.
    fn sort_descendants(&self, nodes: &[Rc<FocusNode>]) -> Vec<Rc<FocusNode>>;
}

/// Top-to-bottom, then left-to-right traversal.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadingOrderPolicy;

impl FocusTraversalPolicy for ReadingOrderPolicy {
    fn sort_descendants(&self, nodes: &[Rc<FocusNode>]) -> Vec<Rc<FocusNode>> {
        Self::sorted_indices(nodes)
            .into_iter()
            .map(|index| Rc::clone(&nodes[index]))
            .collect()
    }
}

impl ReadingOrderPolicy {
    fn sorted_indices(nodes: &[Rc<FocusNode>]) -> Vec<usize> {
        let mut indices: Vec<_> = (0..nodes.len()).collect();
        indices.sort_by(|&left, &right| {
            let left_rect = nodes[left].rect();
            let right_rect = nodes[right].rect();
            let y = left_rect.top().0.total_cmp(&right_rect.top().0);
            if y != Ordering::Equal {
                return y;
            }
            left_rect.left().0.total_cmp(&right_rect.left().0)
        });
        indices
    }
}
