//! Owner-affine focus tree and traversal primitives.
//!
//! Focus nodes form a tree parallel to the view tree. A node is created
//! unbound, then acquires the exact [`crate::FocusManager`] owner when its
//! subtree is attached below that manager's root scope. There is no ambient
//! focus manager and no registry keyed by node IDs.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    num::NonZeroU64,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
};

use super::traversal::{FocusTraversalOverrides, GroupConfig, GroupOrderCache, GroupOrderSnapshot};
use flui_foundation::ListenerId;
use flui_foundation::geometry::Rect;
use flui_painting::typography::TextDirection;
use flui_platform_api::keyboard::KeyEvent;
use thiserror::Error;

use super::focus::FocusClosePanic;
use crate::__runtime::{CloseMode, CloseTombstone};
use crate::FocusManager;

pub use crate::ids::FocusNodeId;

static NEXT_FOCUS_NODE_ID: AtomicU64 = AtomicU64::new(1);

fn allocate_focus_node_id(counter: &AtomicU64) -> FocusNodeId {
    let raw = counter
        .try_update(
            AtomicOrdering::Relaxed,
            AtomicOrdering::Relaxed,
            |current| (current != 0).then(|| current.checked_add(1).unwrap_or(0)),
        )
        .expect("BUG: focus node identity capacity exhausted");
    FocusNodeId::new(NonZeroU64::new(raw).expect("BUG: allocated focus identity is nonzero"))
}

/// Owner-local callback for handling key events.
pub type KeyEventHandler = Rc<dyn Fn(&KeyEvent) -> KeyEventResult>;

/// Computes a node's bounding rectangle on demand, in root coordinates.
pub type RectProvider = Rc<dyn Fn() -> Option<Rect<f64>>>;

/// What the widget layer records about where a node sits in its tree. This
/// crate cannot hold an element reference itself because it sits below the
/// element tree.
///
/// Opaque here: this crate stores and returns it and never reads it.
/// `flui-widgets` records the `Actions` chain visible at the node's `Focus`
/// widget, so a `Shortcuts` above the focused widget resolves an intent from
/// the focused widget's position.
pub type NodeContext = Rc<dyn std::any::Any>;

/// ChangeNotifier-style callback for one focus node.
pub type FocusNodeChangeCallback = Rc<dyn Fn()>;

/// Result of one focus-node key handler.
///
/// These three outcomes form the closed propagation algebra: continue, consume,
/// or stop without consuming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum KeyEventResult {
    /// Stop propagation and consume the event.
    Handled,
    /// Continue to the parent focus node.
    Ignored,
    /// Stop propagation without consuming the event.
    SkipRemainingHandlers,
}

impl KeyEventResult {
    /// Whether native default handling should be prevented.
    #[must_use]
    pub const fn is_handled(self) -> bool {
        matches!(self, Self::Handled)
    }

    /// Combine several handler channels on one node.
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
#[non_exhaustive]
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
#[non_exhaustive]
#[must_use]
pub enum FocusRequestOutcome {
    /// Accepted — applied immediately, or, when requested from inside a
    /// focus-change listener, queued and applied after the in-flight
    /// notification completes (see `FocusManager`'s notification-ordering
    /// contract). Does not by itself mean the node has primary focus yet:
    /// for a queued request, `has_primary_focus()` only becomes true once
    /// the notification that queued it finishes.
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
#[non_exhaustive]
#[must_use]
pub enum FocusDetachOutcome {
    /// The live attachment was detached.
    Detached,
    /// The handle no longer describes the node's current attachment.
    Stale,
    /// The owner closed and retired the node.
    OwnerClosed,
}

#[derive(Debug, Clone)]
enum ManagerBinding {
    Unbound,
    Bound(Weak<FocusManager>),
    Closed(CloseTombstone),
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
        if matches!(*node.manager_binding.borrow(), ManagerBinding::Closed(_)) {
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
        if matches!(*node.manager_binding.borrow(), ManagerBinding::Closed(_)) {
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

/// Ownership of one focus-node listener registration.
///
/// Dropping the subscription withdraws its listener. The subscription holds a
/// weak node reference, so retaining it cannot keep the focus tree alive.
/// Withdrawal commits before callback captures are retired and follows the
/// owner's existing panic-preservation policy.
#[must_use = "retain the subscription for as long as the listener should remain installed"]
#[derive(Debug)]
pub struct FocusSubscription {
    node: Weak<FocusNode>,
    listener_id: ListenerId,
}

impl Drop for FocusSubscription {
    fn drop(&mut self) {
        if let Some(node) = self.node.upgrade() {
            node.remove_listener(self.listener_id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusNodeRegistrationKind {
    KeyHandler,
    RectProvider,
    Context,
    TraversalGroup,
    TraversalOverrides,
}

/// Generation-checked ownership of one replaceable [`FocusNode`] property.
///
/// A registration is returned by
/// [`FocusNode::register_on_key_event`],
/// [`FocusNode::register_rect_provider`] or [`FocusNode::register_context`].
/// Dropping it clears the installed
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
            && self.node.upgrade().is_some_and(|node| {
                !node.is_closed()
                    && match self.kind {
                        FocusNodeRegistrationKind::KeyHandler => {
                            node.on_key_event_generation.get() == self.generation
                        }
                        FocusNodeRegistrationKind::RectProvider => {
                            node.rect_provider_generation.get() == self.generation
                        }
                        FocusNodeRegistrationKind::Context => {
                            node.context_generation.get() == self.generation
                        }
                        FocusNodeRegistrationKind::TraversalGroup => {
                            node.traversal_group_generation.get() == self.generation
                        }
                        FocusNodeRegistrationKind::TraversalOverrides => {
                            node.traversal_overrides_generation.get() == self.generation
                        }
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
            FocusNodeRegistrationKind::Context => {
                node.clear_context_generation(self.generation);
            }
            FocusNodeRegistrationKind::TraversalGroup => {
                node.clear_traversal_group_generation(self.generation)
            }
            FocusNodeRegistrationKind::TraversalOverrides => {
                if node.traversal_overrides_generation.get() == self.generation {
                    *node.traversal_overrides.borrow_mut() = FocusTraversalOverrides::default();
                }
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
    rect: Cell<Rect<f64>>,
    rect_provider: RefCell<Option<RectProvider>>,
    rect_provider_generation: Cell<u64>,
    context: RefCell<Option<NodeContext>>,
    context_generation: Cell<u64>,
    manager_binding: RefCell<ManagerBinding>,
    attached: Cell<bool>,
    pending_focus_request: Cell<bool>,
    attachment_generation: Cell<u64>,
    on_key_event_generation: Cell<u64>,
    traversal_group: RefCell<Option<GroupConfig>>,
    traversal_group_generation: Cell<u64>,
    traversal_overrides: RefCell<FocusTraversalOverrides>,
    traversal_overrides_generation: Cell<u64>,
}

pub(super) struct ClosedFocusNode {
    node: Rc<FocusNode>,
    key_handler: Option<KeyEventHandler>,
    rect_provider: Option<RectProvider>,
    context: Option<NodeContext>,
    policy: Option<Rc<dyn FocusTraversalPolicy>>,
    group_policy: Option<Rc<dyn FocusTraversalPolicy>>,
}

impl ClosedFocusNode {
    pub(super) fn retire(self, failure: &mut FocusClosePanic) {
        let Self {
            node,
            key_handler,
            rect_provider,
            context,
            policy,
            group_policy,
        } = self;
        // The order a healthy close always kept: key handler, listeners,
        // rect provider, context.
        failure.retire(key_handler);
        let listeners = std::mem::take(&mut *node.listeners.borrow_mut());
        for (_, listener) in listeners {
            failure.retire(listener);
        }
        failure.retire(rect_provider);
        failure.retire(context);
        failure.retire(policy);
        failure.retire(group_policy);
        failure.retire(node);
    }
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
        Self::create_with_counter(label, scope_owner, &NEXT_FOCUS_NODE_ID)
    }

    fn create_with_counter(
        label: Option<String>,
        scope_owner: Option<Weak<FocusScopeNode>>,
        counter: &AtomicU64,
    ) -> Rc<Self> {
        Rc::new(Self {
            id: allocate_focus_node_id(counter),
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
            context: RefCell::new(None),
            context_generation: Cell::new(0),
            manager_binding: RefCell::new(ManagerBinding::Unbound),
            attached: Cell::new(false),
            pending_focus_request: Cell::new(false),
            attachment_generation: Cell::new(1),
            on_key_event_generation: Cell::new(0),
            traversal_group: RefCell::new(None),
            traversal_group_generation: Cell::new(0),
            traversal_overrides: RefCell::new(FocusTraversalOverrides::default()),
            traversal_overrides_generation: Cell::new(0),
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
    pub fn rect(&self) -> Rect<f64> {
        let provider = self.rect_provider.borrow().clone();
        if let Some(provider) = provider
            && let Some(rect) = provider()
        {
            return rect;
        }
        self.rect.get()
    }

    /// Store fallback traversal geometry.
    pub fn set_rect(&self, rect: Rect<f64>) {
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

    /// What the widget layer recorded about this node's position, if
    /// anything ([`NodeContext`]).
    #[must_use]
    pub fn context(&self) -> Option<NodeContext> {
        self.context.borrow().clone()
    }

    /// Record the widget layer's [`NodeContext`] with generation-checked
    /// cleanup ownership.
    ///
    /// Dropping the returned registration clears `context` only if no later
    /// writer has replaced it.
    pub fn register_context(self: &Rc<Self>, context: NodeContext) -> FocusNodeRegistration {
        let generation = self.replace_context(Some(context));
        FocusNodeRegistration::new(self, generation, FocusNodeRegistrationKind::Context)
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
        if self.is_closed() {
            let mut failure = FocusClosePanic::for_rejection(self.close_mode());
            failure.retire(provider);
            failure.finish();
            return self.rect_provider_generation.get();
        }
        let generation = Self::next_property_generation(&self.rect_provider_generation);
        let _prev = std::mem::replace(&mut *self.rect_provider.borrow_mut(), provider);
        generation
    }

    fn clear_rect_provider_generation(&self, generation: u64) {
        if self.rect_provider_generation.get() == generation {
            self.replace_rect_provider(None);
        }
    }

    fn replace_context(&self, context: Option<NodeContext>) -> u64 {
        if self.is_closed() {
            let mut failure = FocusClosePanic::for_rejection(self.close_mode());
            failure.retire(context);
            failure.finish();
            return self.context_generation.get();
        }
        let generation = Self::next_property_generation(&self.context_generation);
        let _prev = std::mem::replace(&mut *self.context.borrow_mut(), context);
        generation
    }

    fn clear_context_generation(&self, generation: u64) {
        if self.context_generation.get() == generation {
            self.replace_context(None);
        }
    }

    fn replace_on_key_event(&self, handler: Option<KeyEventHandler>) -> u64 {
        if self.is_closed() {
            let mut failure = FocusClosePanic::for_rejection(self.close_mode());
            failure.retire(handler);
            failure.finish();
            return self.on_key_event_generation.get();
        }
        let generation = Self::next_property_generation(&self.on_key_event_generation);
        let _prev = std::mem::replace(&mut *self.on_key_event.borrow_mut(), handler);
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

    /// Install weak linear traversal links with generation-checked cleanup.
    pub fn register_traversal_overrides(
        self: &Rc<Self>,
        overrides: FocusTraversalOverrides,
    ) -> FocusNodeRegistration {
        let generation = Self::next_property_generation(&self.traversal_overrides_generation);
        if !self.is_closed() {
            *self.traversal_overrides.borrow_mut() = overrides;
        }
        FocusNodeRegistration::new(
            self,
            generation,
            FocusNodeRegistrationKind::TraversalOverrides,
        )
    }

    /// Establish a policy boundary without introducing a focus scope or history.
    pub fn register_traversal_group(
        self: &Rc<Self>,
        policy: Rc<dyn FocusTraversalPolicy>,
        direction: TextDirection,
        edge: TraversalEdgeBehavior,
    ) -> FocusNodeRegistration {
        let generation = Self::next_property_generation(&self.traversal_group_generation);
        let mut failure = FocusClosePanic::for_rejection(self.close_mode());
        if self.is_closed() {
            failure.retire(policy);
        } else {
            let previous = self.traversal_group.borrow_mut().replace(GroupConfig {
                policy,
                direction,
                edge,
            });
            if let Some(previous) = previous {
                failure.retire(previous.policy);
            }
        }
        failure.finish();
        FocusNodeRegistration::new(self, generation, FocusNodeRegistrationKind::TraversalGroup)
    }

    fn clear_traversal_group_generation(&self, generation: u64) {
        if self.traversal_group_generation.get() != generation {
            return;
        }
        let previous = self.traversal_group.borrow_mut().take();
        let mut failure = FocusClosePanic::for_rejection(self.close_mode());
        if let Some(previous) = previous {
            failure.retire(previous.policy);
        }
        failure.finish();
    }

    pub(super) fn traversal_group_snapshot(&self) -> Option<GroupConfig> {
        self.traversal_group.borrow().clone()
    }
    pub(super) fn traversal_group_edge(&self) -> Option<TraversalEdgeBehavior> {
        self.traversal_group
            .borrow()
            .as_ref()
            .map(|group| group.edge)
    }
    pub(super) fn traversal_override_target(
        &self,
        direction: TraversalDirection,
    ) -> Option<Rc<FocusNode>> {
        self.traversal_overrides.borrow().target(direction)
    }
    pub(super) fn traversal_close_mode(&self) -> CloseMode {
        self.close_mode()
    }
    pub(super) fn traversal_geometry_snapshot(&self) -> (Option<RectProvider>, Rect<f64>) {
        (self.rect_provider.borrow().clone(), self.rect.get())
    }

    /// Register a listener for focus or focusability changes on this node.
    pub(super) fn add_listener(&self, callback: FocusNodeChangeCallback) -> ListenerId {
        let id = ListenerId::new(self.next_listener_id.get());
        let next = self
            .next_listener_id
            .get()
            .checked_add(1)
            .expect("BUG: focus-node listener ID space exhausted");
        self.next_listener_id.set(next);
        if self.is_closed() {
            let mut failure = FocusClosePanic::for_rejection(self.close_mode());
            failure.retire(callback);
            failure.finish();
        } else {
            self.listeners.borrow_mut().push((id, callback));
        }
        id
    }

    /// Subscribe to focus or focusability changes until the returned guard is dropped.
    pub fn subscribe(self: &Rc<Self>, callback: FocusNodeChangeCallback) -> FocusSubscription {
        FocusSubscription {
            node: Rc::downgrade(self),
            listener_id: self.add_listener(callback),
        }
    }

    /// Remove one node listener.
    pub(super) fn remove_listener(&self, id: ListenerId) {
        let removed = {
            let mut listeners = self.listeners.borrow_mut();
            listeners
                .iter()
                .position(|(held, _)| *held == id)
                .map(|index| listeners.remove(index))
        };
        let mut failure = FocusClosePanic::for_rejection(self.close_mode());
        if let Some((_, callback)) = removed {
            failure.retire(callback);
        }
        failure.finish();
    }

    pub(crate) fn notify_listeners(&self) {
        let mut failure = FocusClosePanic::for_rejection(self.close_mode());
        self.notify_listeners_in_round(&mut failure);
        failure.finish();
    }

    pub(super) fn notify_listeners_in_round(&self, failure: &mut FocusClosePanic) {
        if self.parent.borrow().is_none() {
            return;
        }
        self.notify_tree_change_in_round(failure);
    }

    pub(super) fn notify_tree_change_in_round(&self, failure: &mut FocusClosePanic) {
        let ids: Vec<_> = self.listeners.borrow().iter().map(|(id, _)| *id).collect();
        for id in ids {
            // Mirrors `FocusManager::notify_listeners`: a listener removed
            // by an earlier one in this same dispatch (itself included) is
            // never called.
            let listener = self
                .listeners
                .borrow()
                .iter()
                .find(|(registered, _)| *registered == id)
                .map(|(_, listener)| Rc::clone(listener));
            if let Some(listener) = listener {
                let owner = self.manager();
                let failure_guard = owner
                    .as_ref()
                    .map(|owner| owner.notification_failure_scope(failure));
                failure.adopt(self.close_mode());
                let _ = failure.invoke(|| listener());
                if let Some(guard) = &failure_guard {
                    guard.preserve(failure.preserving());
                }
                failure.retire(listener);
            }
        }
    }

    pub(super) fn notify_close_listeners(&self, failure: &mut FocusClosePanic) {
        let ids: Vec<_> = self.listeners.borrow().iter().map(|(id, _)| *id).collect();
        for id in ids {
            let listener = self
                .listeners
                .borrow()
                .iter()
                .find(|(registered, _)| *registered == id)
                .map(|(_, listener)| Rc::clone(listener));
            if let Some(listener) = listener {
                failure.run(|| listener());
                failure.retire(listener);
            }
        }
    }

    fn close_mode(&self) -> CloseMode {
        match &*self.manager_binding.borrow() {
            ManagerBinding::Closed(tombstone) => tombstone.mode(),
            ManagerBinding::Bound(owner) => owner.upgrade().map_or(CloseMode::Ordinary, |owner| {
                owner.notification_failure_mode()
            }),
            _ => CloseMode::Ordinary,
        }
    }

    fn is_closed(&self) -> bool {
        matches!(*self.manager_binding.borrow(), ManagerBinding::Closed(_))
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
    ///
    /// A request accepted while attached is either applied immediately or,
    /// when made from inside a focus-change listener, queued and applied
    /// after the in-flight notification completes (see `FocusManager`'s
    /// notification-ordering contract) — both return
    /// [`FocusRequestOutcome::Focused`].
    pub fn request_focus(self: &Rc<Self>) -> FocusRequestOutcome {
        if !self.can_request_focus() {
            return FocusRequestOutcome::Rejected;
        }
        // Clone the binding out of the `RefCell` before acting on it: the
        // `Bound` arm below calls into the manager, which can (through a
        // reentrant focus listener) close this same manager and tombstone
        // this very node — a nested `manager_binding.borrow_mut()` while
        // this match's scrutinee borrow were still held would panic.
        let binding = self.manager_binding.borrow().clone();
        match binding {
            ManagerBinding::Unbound => {
                self.pending_focus_request.set(true);
                FocusRequestOutcome::Queued
            }
            ManagerBinding::Closed(_) => FocusRequestOutcome::OwnerClosed,
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
        let Some(handler) = handler else {
            return KeyEventResult::Ignored;
        };
        let mut failure = FocusClosePanic::for_rejection(self.close_mode());
        let result = failure
            .invoke(|| handler(event))
            .unwrap_or(KeyEventResult::Ignored);
        failure.retire(handler);
        failure.finish();
        result
    }

    /// Iterate parent-first over ancestors.
    pub fn ancestors(&self) -> impl Iterator<Item = Rc<FocusNode>> {
        AncestorIterator {
            current: self.parent(),
        }
    }

    /// Iterate depth-first over descendants in sibling attachment order.
    pub fn descendants(&self) -> impl Iterator<Item = Rc<FocusNode>> {
        let mut stack = self.children();
        stack.reverse();
        DescendantIterator { stack }
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
            ManagerBinding::Unbound | ManagerBinding::Closed(_) => None,
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
            ManagerBinding::Closed(_) => {
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
        let _old_children = std::mem::replace(&mut *replacement.children.borrow_mut(), children);

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
            ManagerBinding::Closed(_) => Err(FocusTreeError::OwnerClosed { node: self.id }),
        }
    }

    fn validate_subtree_owner(
        node: &Rc<FocusNode>,
        expected: Option<&Rc<FocusManager>>,
    ) -> Result<(), FocusTreeError> {
        match &*node.manager_binding.borrow() {
            ManagerBinding::Unbound => {}
            ManagerBinding::Closed(_) => {
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
            let _ = node.request_focus();
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

    pub(super) fn close_owned_tree(
        root: &Rc<FocusNode>,
        tombstone: CloseTombstone,
    ) -> Vec<ClosedFocusNode> {
        // Post-order, siblings in insertion order: a healthy close retires
        // each child's ownership before its parent's (ADR-0127).
        let mut nodes = Vec::new();
        let mut stack = vec![(Rc::clone(root), false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                nodes.push(node);
            } else {
                let children = node.children();
                stack.push((node, true));
                stack.extend(children.into_iter().rev().map(|child| (child, false)));
            }
        }
        let mut retired = Vec::with_capacity(nodes.len());
        for node in nodes {
            node.attached.set(false);
            node.pending_focus_request.set(false);
            *node.manager_binding.borrow_mut() = ManagerBinding::Closed(tombstone.clone());
            node.parent.borrow_mut().take();
            // All child nodes are retained by the snapshot until their own
            // terminal state and outgoing ownership have been committed.
            node.children.borrow_mut().clear();
            let key_handler = node.on_key_event.borrow_mut().take();
            let rect_provider = node.rect_provider.borrow_mut().take();
            let context = node.context.borrow_mut().take();
            let group_policy = node
                .traversal_group
                .borrow_mut()
                .take()
                .map(|group| group.policy);
            *node.traversal_overrides.borrow_mut() = FocusTraversalOverrides::default();
            let policy = node.as_scope().map(|scope| {
                scope.pending_first_focus.set(false);
                scope.focus_history.borrow_mut().clear();
                std::mem::replace(
                    &mut *scope.traversal_policy.borrow_mut(),
                    Rc::new(ReadingOrderPolicy),
                )
            });
            retired.push(ClosedFocusNode {
                node,
                key_handler,
                rect_provider,
                context,
                policy,
                group_policy,
            });
        }
        retired
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

#[cfg(test)]
pub(crate) fn focus_node_identity_exhaustion_preserves_notifications() {
    let manager = FocusManager::new();
    let start = manager
        .root_scope()
        .id()
        .get()
        .checked_add(1)
        .expect("root identity");
    let counter = AtomicU64::new(start);
    let first = FocusNode::create_with_counter(None, None, &counter);
    counter.store(u64::MAX, AtomicOrdering::Relaxed);
    let last = FocusNode::create_with_counter(None, None, &counter);
    manager
        .root_scope()
        .attach_node(&first)
        .expect("first node attaches");
    manager
        .root_scope()
        .attach_node(&last)
        .expect("last node attaches");
    let notifications = Rc::new(RefCell::new(Vec::new()));
    for (name, node) in [("first", &first), ("last", &last)] {
        let notifications = Rc::clone(&notifications);
        let node = Rc::downgrade(node);
        let target = node.upgrade().expect("live node");
        target.add_listener(Rc::new(move || {
            let node = node.upgrade().expect("attached node");
            notifications
                .borrow_mut()
                .push((name, node.has_primary_focus()));
        }));
    }
    let _ = first.request_focus();
    let _ = last.request_focus();
    for _ in 0..8 {
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            FocusNode::create_with_counter(None, None, &counter)
        }))
        .expect_err("exhausted allocator must permanently refuse");
        flui_foundation::panic::retain_opaque_payload(failure);
    }
    let _ = first.request_focus();
    assert_eq!(
        *notifications.borrow(),
        vec![
            ("first", true),
            ("first", false),
            ("last", true),
            ("last", false),
            ("first", true),
        ],
        "distinct live endpoints remain independently notified after refusal"
    );
    assert!(Rc::ptr_eq(
        &manager.primary_focus().expect("focused node"),
        &first
    ));
    let fresh_counter = AtomicU64::new(start.checked_add(1).expect("fresh identity"));
    let fresh = FocusNode::create_with_counter(None, None, &fresh_counter);
    manager
        .root_scope()
        .attach_node(&fresh)
        .expect("fresh node attaches");
    let _ = fresh.request_focus();
    assert!(Rc::ptr_eq(
        &manager.primary_focus().expect("fresh focus"),
        &fresh
    ));
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
    text_direction: Cell<TextDirection>,
    traversal_edge_behavior: Cell<TraversalEdgeBehavior>,
}

impl FocusScopeNode {
    fn close_mode(&self) -> CloseMode {
        self.inner.close_mode()
    }
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
            text_direction: Cell::new(TextDirection::Ltr),
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
        if self.inner.is_closed() {
            let mut failure = FocusClosePanic::for_rejection(self.close_mode());
            failure.retire(policy);
            failure.finish();
            return;
        }
        let _prev = std::mem::replace(&mut *self.traversal_policy.borrow_mut(), policy);
    }

    /// Reading direction used by this scope's traversal policy.
    #[must_use]
    pub fn text_direction(&self) -> TextDirection {
        self.text_direction.get()
    }

    /// Record the inherited direction without replacing the custom policy.
    pub fn set_text_direction(&self, direction: TextDirection) {
        if !self.inner.is_closed() {
            self.text_direction.set(direction);
        }
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
                    if !matches!(*node.manager_binding.borrow(), ManagerBinding::Closed(_))
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
        if matches!(
            *self.inner.manager_binding.borrow(),
            ManagerBinding::Closed(_)
        ) || self
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
    ///
    /// The current policy is retained for this call without borrowing the
    /// policy cell across user code. A replacement installed by the policy
    /// applies to the next traversal.
    /// A sorting panic propagates before publishing an order. Policy and node
    /// ownership remains outside that callback's unwind: after a failure,
    /// outgoing values are retained rather than running arbitrary destruction.
    /// During an existing unwind, sorting is skipped and the order is empty.
    pub fn sorted_traversal_order(&self, cursor: Option<&Rc<FocusNode>>) -> Vec<Rc<FocusNode>> {
        self.sorted_traversal_order_with_cache(cursor, &mut Vec::new())
    }

    fn sorted_traversal_order_with_cache(
        &self,
        cursor: Option<&Rc<FocusNode>>,
        cache: &mut GroupOrderCache,
    ) -> Vec<Rc<FocusNode>> {
        let mut nodes = self.collect_focusable_nodes();
        if let Some(cursor) = cursor
            && !nodes.iter().any(|node| Rc::ptr_eq(node, cursor))
            && self.inner.has_descendant_node(cursor)
        {
            nodes.push(Rc::clone(cursor));
        }
        let groups = GroupOrderSnapshot::new(&nodes, &self.inner);
        let policy = Rc::clone(&self.traversal_policy.borrow());
        let direction = self.text_direction.get();
        let mut failure = FocusClosePanic::for_rejection(self.close_mode());
        failure.run(|| policy.order(&mut nodes, direction));
        groups.order(&mut nodes, cache, &mut failure);
        groups.retire(&mut failure);
        failure.retire(policy);
        failure.finish_with(nodes)
    }

    /// Resolve one traversal step without applying it.
    pub fn resolve_traversal(
        &self,
        current: Option<&Rc<FocusNode>>,
        direction: TraversalDirection,
    ) -> ResolvedStep {
        self.resolve_traversal_with_cache(current, direction, &mut Vec::new())
    }

    pub(super) fn resolve_traversal_with_cache(
        &self,
        current: Option<&Rc<FocusNode>>,
        direction: TraversalDirection,
        cache: &mut GroupOrderCache,
    ) -> ResolvedStep {
        let forward = matches!(direction, TraversalDirection::Forward);
        let order = self.sorted_traversal_order_with_cache(current, cache);

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
            TraversalEdgeBehavior::LeaveView => ResolvedStep::Unfocus,
        }
    }

    /// Focus the next node in this scope.
    pub fn focus_next_in_scope(&self, current: &Rc<FocusNode>) -> bool {
        self.perform(self.step(Some(current), TraversalDirection::Forward))
    }

    /// Focus the previous node in this scope.
    pub fn focus_previous_in_scope(&self, current: &Rc<FocusNode>) -> bool {
        self.perform(self.step(Some(current), TraversalDirection::Backward))
    }

    /// Resolve a step, following parent-scope edge behavior.
    pub fn step(
        &self,
        current: Option<&Rc<FocusNode>>,
        direction: TraversalDirection,
    ) -> ResolvedStep {
        let mut scope: Option<Rc<FocusScopeNode>> = None;
        loop {
            let step = scope.as_ref().map_or_else(
                || self.resolve_traversal(current, direction),
                |scope| scope.resolve_traversal(current, direction),
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

/// The direction through the scope's ordered traversal candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalDirection {
    /// Move toward the following candidate.
    Forward,
    /// Move toward the preceding candidate.
    Backward,
}

/// What traversal does when it runs off a scope edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraversalEdgeBehavior {
    /// Wrap to the other end.
    #[default]
    ClosedLoop,
    /// Release focus so the host can move outside this view.
    LeaveView,
    /// Continue in the enclosing scope.
    ParentScope,
    /// Keep current focus.
    Stop,
}

/// A typed traversal intent.
#[derive(Clone, Default)]
pub enum ResolvedStep {
    /// Move primary focus to this node.
    Focus(Rc<FocusNode>),
    /// Release primary focus.
    Unfocus,
    /// No focus change.
    #[default]
    None,
    /// Re-resolve the step in the enclosing scope.
    RetryInParent,
}

impl crate::retain::Retain for ResolvedStep {
    fn retain(self) {
        if let Self::Focus(node) = self {
            crate::retain::Retain::retain(node);
        }
    }
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
    /// Permute the supplied candidates in place using the scope's direction.
    ///
    /// Implementations must retain every supplied node exactly once. Direction
    /// is frozen for the current traversal; changes apply to the next call.
    fn order(&self, nodes: &mut [Rc<FocusNode>], direction: TextDirection);
}

/// Top-to-bottom rows, then leading-edge traversal within each row.
///
/// A row has a shared, strictly nonempty vertical intersection; a tall node
/// cannot bridge disjoint rows. Exact ties retain structural order. Invalid
/// or empty rectangles follow valid rows in structural order.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadingOrderPolicy;

impl FocusTraversalPolicy for ReadingOrderPolicy {
    fn order(&self, nodes: &mut [Rc<FocusNode>], direction: TextDirection) {
        // Rect providers are user code: snapshot once before comparisons.
        let rectangles: Vec<_> = nodes.iter().map(|node| node.rect()).collect();
        let (mut spatial, fallback): (Vec<_>, Vec<_>) = (0..nodes.len()).partition(|&index| {
            let rect = rectangles[index];
            [rect.left(), rect.top(), rect.right(), rect.bottom()]
                .into_iter()
                .all(f64::is_finite)
                && rect.left() < rect.right()
                && rect.top() < rect.bottom()
        });
        spatial.sort_by(|&left, &right| {
            rectangles[left]
                .top()
                .total_cmp(&rectangles[right].top())
                .then_with(|| left.cmp(&right))
        });
        let mut row_start = 0;
        while row_start < spatial.len() {
            let first = rectangles[spatial[row_start]];
            let mut top = first.top();
            let mut bottom = first.bottom();
            let mut row_end = row_start + 1;
            while row_end < spatial.len() {
                let next = rectangles[spatial[row_end]];
                let shared_top = top.max(next.top());
                let shared_bottom = bottom.min(next.bottom());
                if shared_top >= shared_bottom {
                    break;
                }
                top = shared_top;
                bottom = shared_bottom;
                row_end += 1;
            }
            spatial[row_start..row_end].sort_by(|&left, &right| {
                let left_rect = rectangles[left];
                let right_rect = rectangles[right];
                match direction {
                    TextDirection::Ltr => left_rect.left().total_cmp(&right_rect.left()),
                    TextDirection::Rtl => right_rect.right().total_cmp(&left_rect.right()),
                }
                .then_with(|| left.cmp(&right))
            });
            row_start = row_end;
        }
        spatial.extend(fallback);
        // Convert source indices into destinations, then follow permutation
        // cycles without cloning or retiring any candidate handle.
        let mut destinations = vec![0; nodes.len()];
        for (destination, source) in spatial.into_iter().enumerate() {
            destinations[source] = destination;
        }
        for index in 0..nodes.len() {
            while destinations[index] != index {
                let destination = destinations[index];
                nodes.swap(index, destination);
                destinations.swap(index, destination);
            }
        }
    }
}
