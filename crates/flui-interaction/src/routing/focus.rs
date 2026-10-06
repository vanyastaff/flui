//! Owner-local keyboard focus manager.
//!
//! A [`FocusManager`] is explicitly owned by one presentation. It is neither
//! global nor thread-local. Nodes reach it only through the weak owner stored
//! when their subtree is attached below [`FocusManager::root_scope`].

use std::{
    cell::{Cell, RefCell},
    collections::{HashSet, VecDeque},
    rc::{Rc, Weak},
};

use flui_foundation::ListenerId;

use crate::{
    events::KeyEvent,
    routing::focus_scope::{FocusNode, FocusScopeNode, KeyEventResult},
};

/// Callback invoked after primary focus or its focus-tree ancestry changes.
pub type FocusChangeCallback = Rc<dyn Fn(Option<Rc<FocusNode>>, Option<Rc<FocusNode>>)>;

/// Owner-local global key handler.
pub type KeyEventCallback = Rc<dyn Fn(&KeyEvent) -> bool>;

pub(super) use crate::__runtime::ClosePanic as FocusClosePanic;
use crate::__runtime::{CloseMode, CloseTombstone};

/// Presentation-owned focus state and root focus tree.
///
/// # Focus-change notification ordering
///
/// `request_focus`/[`Self::unfocus`] apply and publish a focus transition
/// synchronously: commit the new primary, notify every focus-tree node
/// whose focused ancestry changed, then publish `(previous, new)` to every
/// [`FocusChangeCallback`] registered via [`Self::add_listener`]. A
/// listener that itself calls `request_focus`/`unfocus` from inside that
/// publication — a reentrant request — is never applied inline: it is
/// queued, then re-validated (attached, focusable, still owned by this
/// manager — see the private `is_eligible` predicate) immediately before
/// its turn, and applied only once every currently in-flight notification
/// has finished publishing, in the order it was requested (FIFO). A
/// target that lost eligibility while it waited — a later listener in the
/// same chain detached it, revoked its focusability, or reparented it
/// elsewhere — is skipped rather than committed stale. By the time the
/// outermost `request_focus`/`unfocus` call returns,
/// [`Self::primary_focus`] and the last edge any listener observed always
/// agree — no listener can observe a stale destination (issue #1040). The
/// crate-internal node-replacement completion publishes its own outer
/// edge under this same guard; [`Self::close`] does not participate in it
/// at all — see its own doc for the distinct contract it keeps instead. A
/// reentrant chain that never settles (two listeners that keep
/// re-requesting each other) is bounded: past a fixed per-call budget of
/// applications, the remaining queue is dropped with a single
/// `tracing::warn!`. A listener that panics cannot leave this guard stuck
/// open: `notification_depth` is held by a private RAII guard that
/// decrements on unwind exactly as it does on a normal return, and —
/// because the notification it was guarding never got to finish, so
/// whatever it queued is only half a transaction — also discards the
/// pending queue in that case (warning once, naming the count, if it was
/// non-empty — an unwind must not silently erase requests a healthy
/// caller had already had accepted), so a later, healthy call is never
/// asked to replay a chain a panic interrupted partway through.
/// See `## Mapping decisions` in `crates/flui-interaction/docs/ARCHITECTURE.md`
/// for why transitions apply synchronously rather than being deferred.
pub struct FocusManager {
    root_scope: Rc<FocusScopeNode>,
    primary_focus: RefCell<Option<Rc<FocusNode>>>,
    listeners: RefCell<Vec<(ListenerId, FocusChangeCallback)>>,
    next_listener_id: Cell<usize>,
    global_key_handlers: RefCell<Vec<KeyEventCallback>>,
    /// Nodes that asked to start a key's walk while nothing is focused,
    /// oldest first ([`Self::claim_unfocused_keys`]).
    unfocused_key_claims: RefCell<Vec<Weak<FocusNode>>>,
    closed: Cell<bool>,
    close_mode: CloseTombstone,
    /// Depth of the commit+notify transaction currently publishing a focus
    /// transition. Zero between transitions; `>0` while node or manager
    /// listeners for that transition are running, including reentrant
    /// nesting.
    notification_depth: Cell<u32>,
    /// Focus transitions requested while `notification_depth` is nonzero.
    /// `None` means [`Self::unfocus`]. Drained FIFO by
    /// [`Self::drain_pending_focus_transitions`] once the outermost
    /// notification finishes.
    pending_focus_transitions: RefCell<VecDeque<Option<Rc<FocusNode>>>>,
}

/// RAII scope for one nested level of [`FocusManager::notification_depth`].
///
/// `enter` increments on construction; `Drop` decrements unconditionally,
/// including when the drop runs while unwinding — a listener that panics
/// mid-notification must not leave `notification_depth` stuck above zero,
/// or every later `request_focus`/`unfocus` on that manager would queue
/// forever instead of applying. Unwinding also means the notification this
/// guard was covering never reached the point where it would drain what it
/// queued, so any such entries describe a transaction the panic left half
/// finished; replaying them under a later, healthy call would silently
/// resurrect it, so the drop clears [`FocusManager::pending_focus_transitions`]
/// in that case too.
struct NotificationDepthGuard<'a> {
    depth: &'a Cell<u32>,
    pending: &'a RefCell<VecDeque<Option<Rc<FocusNode>>>>,
}

impl<'a> NotificationDepthGuard<'a> {
    fn enter(depth: &'a Cell<u32>, pending: &'a RefCell<VecDeque<Option<Rc<FocusNode>>>>) -> Self {
        depth.set(depth.get() + 1);
        Self { depth, pending }
    }
}

impl Drop for NotificationDepthGuard<'_> {
    fn drop(&mut self) {
        self.depth.set(self.depth.get() - 1);
        if std::thread::panicking() {
            let mut pending = self.pending.borrow_mut();
            if !pending.is_empty() {
                // Unlike the drain-budget drop (which is a caller's own
                // ping-pong exhausting a documented limit), this discard
                // erases requests a healthy caller had already had
                // accepted — silently losing them would be worse than the
                // panic itself.
                tracing::warn!(
                    dropped_requests = pending.len(),
                    "focus requests queued during a notification were discarded because \
                     a listener panicked"
                );
            }
            pending.clear();
        }
    }
}

impl FocusManager {
    /// Maximum reentrant focus transitions applied per outermost
    /// `request_focus`/`unfocus`/`close` call.
    ///
    /// FLUI applies focus transitions synchronously (unlike a
    /// microtask-deferred model, which merely yields a frame per bounce), so
    /// two listeners that keep redirecting focus to each other would
    /// otherwise spin the caller forever. Past the budget,
    /// [`Self::drain_pending_focus_transitions`] drops whatever is left
    /// and warns once. The budget counts *applications* — queued
    /// transitions that actually commit a change — not queue pops: a
    /// queued entry that turns out to already name the current primary,
    /// or whose target lost eligibility while it waited (see
    /// [`Self::is_eligible`]), is skipped for free and never touches the
    /// counter.
    const REENTRANT_FOCUS_DRAIN_BUDGET: usize = 32;

    /// Create an isolated focus owner and its attached root scope.
    #[must_use]
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|manager| Self {
            root_scope: FocusScopeNode::new_root(manager.clone()),
            primary_focus: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            next_listener_id: Cell::new(1),
            global_key_handlers: RefCell::new(Vec::new()),
            unfocused_key_claims: RefCell::new(Vec::new()),
            closed: Cell::new(false),
            close_mode: CloseTombstone::default(),
            notification_depth: Cell::new(0),
            pending_focus_transitions: RefCell::new(VecDeque::new()),
        })
    }

    /// Root of this manager's focus tree.
    #[inline]
    pub fn root_scope(&self) -> &Rc<FocusScopeNode> {
        &self.root_scope
    }

    /// Current primary focus node.
    #[inline]
    pub fn primary_focus(&self) -> Option<Rc<FocusNode>> {
        self.primary_focus.borrow().clone()
    }

    /// Whether this manager currently has primary focus.
    #[inline]
    pub fn is_focused(&self) -> bool {
        self.primary_focus.borrow().is_some()
    }

    /// Accepted — applied immediately, or, when requested from inside a
    /// focus-change listener, queued and applied after the in-flight
    /// notification completes (see the type-level ordering contract).
    /// Returns `true` whenever the request is accepted, regardless of
    /// which of the two happens.
    pub(crate) fn request_focus(&self, node: &Rc<FocusNode>) -> bool {
        // Ownership is checked first, ahead of `is_eligible`, purely so a
        // foreign-manager request gets its own distinct warning rather than
        // the generic silent rejection the other conditions share.
        if !self.owns(node) {
            tracing::warn!(
                node = node.id().get(),
                "focus request rejected because the node belongs to another manager"
            );
            return false;
        }
        if !self.is_eligible(node) {
            return false;
        }
        self.set_primary_focus(Some(Rc::clone(node)));
        true
    }

    /// Whether `node` is currently attached under this manager's tree and
    /// still bound to it (not, for instance, reparented under a different
    /// manager after this manager last saw it).
    fn owns(&self, node: &Rc<FocusNode>) -> bool {
        node.manager()
            .is_some_and(|owner| std::ptr::eq(owner.as_ref(), self))
    }

    /// Whether `node` may become primary focus on this manager right now.
    ///
    /// The single source of truth for that question: [`Self::request_focus`]
    /// calls it directly (after its own `owns` check, evaluated first only
    /// so a foreign-manager rejection gets its own distinct warning — see
    /// that method), and [`Self::drain_pending_focus_transitions`] calls it
    /// again immediately before a queued transition is applied, since the
    /// world can change while a request waits in the queue — a reentrant
    /// listener earlier in the same chain can detach `node`, flip its
    /// [`FocusNode::can_request_focus`], or reparent it under a different
    /// manager before its turn comes, and a stale queued target must be
    /// skipped rather than committed.
    fn is_eligible(&self, node: &Rc<FocusNode>) -> bool {
        !self.closed.get() && node.is_attached() && node.can_request_focus() && self.owns(node)
    }

    /// Request that `node` (or `None` for [`Self::unfocus`]) become the
    /// committed primary focus.
    ///
    /// Queues the request instead of applying it when a notification is
    /// already in flight — see the type-level ordering contract.
    fn set_primary_focus(&self, node: Option<Rc<FocusNode>>) {
        if self.notification_depth.get() > 0 {
            self.pending_focus_transitions.borrow_mut().push_back(node);
            return;
        }
        self.apply_focus_transition(node);
        self.drain_pending_focus_transitions();
    }

    /// Commit one focus transition and publish it.
    ///
    /// A no-op if `node` is already the committed primary. Otherwise
    /// commits, refreshes focus history, then notifies focus-tree nodes
    /// and manager listeners with `notification_depth` held above zero
    /// (via [`NotificationDepthGuard`]) so a reentrant
    /// `request_focus`/`unfocus` queues instead of applying inline. The
    /// caller is responsible for draining `pending_focus_transitions` once
    /// this returns at depth zero.
    fn apply_focus_transition(&self, node: Option<Rc<FocusNode>>) {
        let previous = {
            let mut primary = self.primary_focus.borrow_mut();
            if Self::focus_identity_eq(primary.as_ref(), node.as_ref()) {
                return;
            }
            std::mem::replace(&mut *primary, node.clone())
        };

        tracing::trace!(
            previous = ?previous.as_ref().map(|node| node.id().get()),
            new = ?node.as_ref().map(|node| node.id().get()),
            "focus changed"
        );

        if let Some(node) = &node {
            Self::refresh_focus_history(node);
        }

        let _guard = NotificationDepthGuard::enter(
            &self.notification_depth,
            &self.pending_focus_transitions,
        );
        Self::notify_focus_nodes(previous.as_ref(), node.as_ref());
        self.notify_listeners(previous, node);
    }

    /// Whether `a` and `b` name the identical focus node (or both `None`).
    fn focus_identity_eq(a: Option<&Rc<FocusNode>>, b: Option<&Rc<FocusNode>>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }

    /// Apply every focus transition queued by a reentrant listener, in the
    /// order it was requested, until the queue is empty, the manager
    /// closes, or [`Self::REENTRANT_FOCUS_DRAIN_BUDGET`] is exhausted.
    ///
    /// Each dequeued `Some(node)` is re-validated with [`Self::is_eligible`]
    /// before it is applied — the world can change between when a
    /// reentrant listener queued it and when its turn comes — and skipped
    /// with a `tracing::trace!` if the target is no longer eligible. A
    /// dequeued `None` ([`Self::unfocus`]) is always eligible. Neither an
    /// eligibility skip nor a same-identity no-op counts against the drain
    /// budget: it bounds applications, not queue pops.
    fn drain_pending_focus_transitions(&self) {
        let mut applied = 0usize;
        loop {
            if self.closed.get() {
                let _prev = std::mem::take(&mut *self.pending_focus_transitions.borrow_mut());
                return;
            }
            let Some(node) = self.pending_focus_transitions.borrow_mut().pop_front() else {
                return;
            };
            if let Some(target) = &node
                && !self.is_eligible(target)
            {
                tracing::trace!(
                    node = target.id().get(),
                    "skipping a queued focus transition whose target is no longer eligible"
                );
                continue;
            }
            if Self::focus_identity_eq(self.primary_focus.borrow().as_ref(), node.as_ref()) {
                // Already the committed primary: applying it would be the
                // same no-op `apply_focus_transition` itself would detect,
                // so it never counted as an application either.
                continue;
            }
            applied += 1;
            if applied > Self::REENTRANT_FOCUS_DRAIN_BUDGET {
                // `node` (the one that tripped the budget) plus whatever is
                // still queued behind it are both dropped below — count
                // both, so the warning is the one piece of evidence a
                // ping-pong author will ever see for this drop.
                let dropped = 1 + self.pending_focus_transitions.borrow().len();
                tracing::warn!(
                    budget = Self::REENTRANT_FOCUS_DRAIN_BUDGET,
                    last_requested = ?node.as_ref().map(|node| node.id().get()),
                    dropped_requests = dropped,
                    "reentrant focus requests exceeded the drain budget; \
                     dropping the rest of the queue"
                );
                let _prev = std::mem::take(&mut *self.pending_focus_transitions.borrow_mut());
                return;
            }
            self.apply_focus_transition(node);
        }
    }

    /// Clear an exact primary node without exposing a half-mutated tree to
    /// callbacks. [`Self::finish_node_replacement`] completes notification
    /// after the structural transaction is stable.
    pub(crate) fn clear_primary_for_node_replacement(&self, current: &Rc<FocusNode>) {
        let mut primary = self.primary_focus.borrow_mut();
        if primary
            .as_ref()
            .is_some_and(|focused| Rc::ptr_eq(focused, current))
        {
            primary.take();
        }
    }

    /// Refresh focus history and deliver the deferred half of an atomic node
    /// replacement.
    ///
    /// Publishes `(previous_primary, current)` to manager listeners only
    /// when primary-focus identity actually changed across the
    /// replacement — a replacement that leaves primary on the same node
    /// (only its ancestry changed) is not a transition and publishes
    /// nothing; the affected ancestors' node-level listeners still fire
    /// via [`Self::notify_focus_path_change`]. Runs that node notification
    /// with `notification_depth` held above zero (via
    /// [`NotificationDepthGuard`]), so a reentrant `request_focus`/`unfocus`
    /// from one of those listeners is queued and applied after this
    /// publication — see the type-level ordering contract.
    ///
    /// Every caller today (`replace_node`, from ordinary frame-phase code)
    /// enters with `notification_depth` already zero. Entering nested — a
    /// `replace_node` called from inside a focus-change listener — would
    /// publish this call's own outer edge ahead of the notification still
    /// in flight, the one stale-destination shape the reentrant-queue
    /// contract above does not cover: the primary was already cleared
    /// structurally by [`Self::clear_primary_for_node_replacement`], so
    /// there is nothing left to *queue* the way `request_focus`/`unfocus`
    /// do. Guarded with `debug_assert_eq!` rather than a `Result` because
    /// no reachable caller can trip it today.
    pub(crate) fn finish_node_replacement(
        &self,
        previous_primary: Rc<FocusNode>,
        previous_focus_path: Vec<Rc<FocusNode>>,
    ) {
        debug_assert_eq!(
            self.notification_depth.get(),
            0,
            "BUG: finish_node_replacement entered while a notification is already in \
             flight; see this method's doc for the nested-publish hazard that would follow"
        );

        let current = self.primary_focus();
        if let Some(primary) = &current {
            Self::refresh_focus_history(primary);
        }
        let current_focus_path = current.as_ref().map_or_else(Vec::new, |primary| {
            std::iter::once(Rc::clone(primary))
                .chain(primary.ancestors())
                .collect()
        });

        {
            let _guard = NotificationDepthGuard::enter(
                &self.notification_depth,
                &self.pending_focus_transitions,
            );
            Self::notify_focus_path_change(previous_focus_path, current_focus_path);
            if !Self::focus_identity_eq(current.as_ref(), Some(&previous_primary)) {
                self.notify_listeners(Some(previous_primary), current);
            }
        }
        if self.notification_depth.get() == 0 {
            self.drain_pending_focus_transitions();
        }
    }

    /// Release primary focus.
    ///
    /// Subject to the same reentrant-queueing contract as `request_focus`
    /// when called from inside a focus-change listener: queued and applied
    /// after the in-flight notification finishes rather than recursing.
    pub fn unfocus(&self) {
        if !self.closed.get() {
            self.set_primary_focus(None);
        }
    }

    /// Register a focus or focused-ancestry change listener.
    ///
    /// The callback receives `(previous, new)` in the order transitions
    /// are committed. A reentrant `request_focus`/`unfocus` made from
    /// inside a callback is applied only after every currently in-flight
    /// notification finishes publishing, so `new` from one call this
    /// listener observes is always `previous` on the next — no listener
    /// ever sees a destination that a later, already-applied transition
    /// has superseded.
    pub fn add_listener(&self, callback: FocusChangeCallback) -> ListenerId {
        let id = ListenerId::new(self.next_listener_id.get());
        let next = self
            .next_listener_id
            .get()
            .checked_add(1)
            .expect("BUG: focus-manager listener ID space exhausted");
        self.next_listener_id.set(next);
        if self.closed.get() {
            let mut failure = FocusClosePanic::for_rejection(self.close_mode.mode());
            failure.retire(callback);
            failure.finish();
        } else {
            self.listeners.borrow_mut().push((id, callback));
        }
        id
    }

    /// Remove one focus-change listener.
    pub fn remove_listener(&self, id: ListenerId) {
        let removed = {
            let mut listeners = self.listeners.borrow_mut();
            listeners
                .iter()
                .position(|(held, _)| *held == id)
                .map(|index| listeners.remove(index))
        };
        drop(removed);
    }

    /// Remove all focus-change listeners.
    pub fn clear_listeners(&self) {
        let listeners = std::mem::take(&mut *self.listeners.borrow_mut());
        let mut failure = FocusClosePanic::for_rejection(self.close_mode.mode());
        for (_, listener) in listeners {
            failure.retire(listener);
        }
        failure.finish();
    }

    /// Number of registered listeners.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn listener_count(&self) -> usize {
        self.listeners.borrow().len()
    }

    fn notify_listeners(&self, previous: Option<Rc<FocusNode>>, new: Option<Rc<FocusNode>>) {
        let ids: Vec<_> = self.listeners.borrow().iter().map(|(id, _)| *id).collect();
        for id in ids {
            // A listener already dispatched in this loop may have removed
            // a later one (itself included) — skip it: once removed, a
            // listener is never called again, even mid-dispatch.
            let listener = self
                .listeners
                .borrow()
                .iter()
                .find(|(registered, _)| *registered == id)
                .map(|(_, listener)| Rc::clone(listener));
            if let Some(listener) = listener {
                let mut failure = FocusClosePanic::for_rejection(self.close_mode.mode());
                let _ = failure.invoke(|| listener(previous.clone(), new.clone()));
                failure.retire(listener);
                failure.finish();
            }
        }
    }

    fn notify_focus_nodes(previous: Option<&Rc<FocusNode>>, new: Option<&Rc<FocusNode>>) {
        let previous_path: Vec<_> = previous
            .into_iter()
            .flat_map(|node| node.ancestors())
            .collect();
        let new_path: Vec<_> = new.into_iter().flat_map(|node| node.ancestors()).collect();
        let previous_ids: HashSet<_> = previous_path.iter().map(|node| node.id()).collect();
        let new_ids: HashSet<_> = new_path.iter().map(|node| node.id()).collect();

        let mut seen = HashSet::new();
        let mut changed = Vec::new();
        for node in previous_path {
            if !new_ids.contains(&node.id()) && seen.insert(node.id()) {
                changed.push(node);
            }
        }
        for node in new_path {
            if !previous_ids.contains(&node.id()) && seen.insert(node.id()) {
                changed.push(node);
            }
        }
        for endpoint in [previous, new].into_iter().flatten() {
            if seen.insert(endpoint.id()) {
                changed.push(Rc::clone(endpoint));
            }
        }
        for node in changed {
            node.notify_listeners();
        }
    }

    fn notify_focus_path_change(
        previous_path: Vec<Rc<FocusNode>>,
        current_path: Vec<Rc<FocusNode>>,
    ) {
        let previous_ids: HashSet<_> = previous_path.iter().map(|node| node.id()).collect();
        let current_ids: HashSet<_> = current_path.iter().map(|node| node.id()).collect();
        let mut seen = HashSet::new();
        let changed = previous_path
            .into_iter()
            .filter(|node| !current_ids.contains(&node.id()))
            .chain(
                current_path
                    .into_iter()
                    .filter(|node| !previous_ids.contains(&node.id())),
            )
            .filter(|node| seen.insert(node.id()))
            .collect::<Vec<_>>();

        for node in changed {
            node.notify_listeners_after_tree_change();
        }
    }

    pub(crate) fn refresh_focus_history(primary: &Rc<FocusNode>) {
        let mut scope_focus = Rc::clone(primary);
        for ancestor in primary.ancestors() {
            if let Some(scope) = ancestor.as_scope() {
                scope.record_focus(&scope_focus);
                scope_focus = ancestor;
            }
        }
    }

    /// Move focus forward in the primary node's enclosing traversal scope.
    pub fn focus_next(&self) -> bool {
        self.traverse(true)
    }

    /// Move focus backward in the primary node's enclosing traversal scope.
    pub fn focus_previous(&self) -> bool {
        self.traverse(false)
    }

    fn traverse(&self, forward: bool) -> bool {
        if self.closed.get() {
            return false;
        }
        let current = self.primary_focus();
        let scope = current
            .as_ref()
            .and_then(|node| node.as_scope().or_else(|| node.enclosing_scope()))
            .unwrap_or_else(|| Rc::clone(&self.root_scope));
        // A scope may temporarily hold primary focus while an explicit
        // first-focus intent waits for its first eligible descendant. Treat
        // that parked scope like "no cursor" so Tab enters its descendants.
        let cursor = current
            .as_ref()
            .filter(|node| !Rc::ptr_eq(node, scope.as_focus_node()));
        let step = scope.step(cursor, forward);
        FocusScopeNode::perform_with_manager(self, step)
    }

    /// Register an owner-local handler that runs before the focus-tree walk.
    ///
    /// A closed owner rejects incoming callback ownership. Healthy rejection
    /// runs its destructor outside internal borrows; rejection during an active
    /// unwind retains it to preserve the original failure.
    pub fn add_global_key_handler(&self, handler: KeyEventCallback) {
        if self.closed.get() {
            let mut failure = FocusClosePanic::for_rejection(self.close_mode.mode());
            failure.retire(handler);
            failure.finish();
        } else {
            self.global_key_handlers.borrow_mut().push(handler);
        }
    }

    /// Remove all global key handlers.
    pub fn clear_global_key_handlers(&self) {
        let handlers = std::mem::take(&mut *self.global_key_handlers.borrow_mut());
        let mut failure = FocusClosePanic::for_rejection(self.close_mode.mode());
        for handler in handlers {
            failure.retire(handler);
        }
        failure.finish();
    }

    /// Dispatch a key event through global handlers, then focused leaf to root.
    ///
    /// Global handlers are snapshotted by identity. A handler removed before
    /// its turn is skipped even if a caller still retains its `Rc`; a later
    /// snapshot identity that remains registered can still handle the key.
    pub fn dispatch_key_event(&self, event: &KeyEvent) -> bool {
        if self.closed.get() {
            return false;
        }

        let global_handlers: Vec<_> = self
            .global_key_handlers
            .borrow()
            .iter()
            .map(Rc::downgrade)
            .collect();
        for handler in global_handlers {
            if self.closed.get() {
                return false;
            }
            let Some(handler) = handler.upgrade() else {
                continue;
            };
            let registered = self
                .global_key_handlers
                .borrow()
                .iter()
                .any(|live| Rc::ptr_eq(live, &handler));
            if !registered {
                continue;
            }
            let mut failure = FocusClosePanic::for_rejection(self.close_mode.mode());
            let handled = failure.invoke(|| handler(event)).unwrap_or(false);
            failure.retire(handler);
            failure.finish();
            if handled {
                tracing::trace!("key event handled by global focus handler");
                return true;
            }
        }

        let Some(focused) = self.primary_focus().or_else(|| self.unfocused_key_target()) else {
            tracing::trace!("key event ignored because nothing is focused");
            return false;
        };

        for node in std::iter::once(Rc::clone(&focused)).chain(focused.ancestors()) {
            match node.handle_key_event(event) {
                KeyEventResult::Ignored => {}
                KeyEventResult::Handled => {
                    tracing::trace!(node = node.id().get(), "key event handled");
                    return true;
                }
                KeyEventResult::SkipRemainingHandlers => {
                    tracing::trace!(
                        node = node.id().get(),
                        "key propagation stopped without consuming the event"
                    );
                    return false;
                }
            }
        }

        tracing::trace!("key event not handled");
        false
    }

    /// Ask for keys to start their walk at `node` while nothing is focused.
    ///
    /// The walk normally starts at the primary focus. A window opened with
    /// nothing focused has none, so without a target every key is dropped —
    /// including the first Tab that would bring the focus in. FLUI's
    /// default bindings claim their own node here, so the walk
    /// reaches them and nothing about `primary_focus` changes.
    ///
    /// Claims nest: the newest one that is still alive, attached and owned by
    /// this manager is the target ([`Self::unfocused_key_target`]), so a
    /// nested claimant going away hands the keys back to the one it covered.
    /// Held weakly: the node's owner decides its lifetime. Claiming a node
    /// again moves it to the top.
    pub fn claim_unfocused_keys(&self, node: &Rc<FocusNode>) {
        let mut claims = self.unfocused_key_claims.borrow_mut();
        claims.retain(|claim| claim.upgrade().is_some_and(|live| !Rc::ptr_eq(&live, node)));
        claims.push(Rc::downgrade(node));
    }

    /// Withdraw `node`'s claim, wherever it sits among the claims.
    pub fn release_unfocused_keys(&self, node: &Rc<FocusNode>) {
        self.unfocused_key_claims
            .borrow_mut()
            .retain(|claim| claim.upgrade().is_some_and(|live| !Rc::ptr_eq(&live, node)));
    }

    /// Where a key starts while nothing is focused: the newest claim whose
    /// node is alive, attached and owned by this manager. A node of another
    /// window's tree never qualifies, so a key cannot reach its handlers.
    #[must_use]
    pub fn unfocused_key_target(&self) -> Option<Rc<FocusNode>> {
        self.unfocused_key_claims
            .borrow()
            .iter()
            .rev()
            .filter_map(Weak::upgrade)
            .find(|node| node.is_attached() && self.owns(node))
    }

    /// Deterministically retire this focus owner.
    ///
    /// Closing is idempotent. It sends the final focus-loss notification,
    /// clears manager and node callbacks, and tombstones every owned node.
    /// Tombstoned nodes cannot later attach to a different manager.
    /// The complete tree is detached and its key, geometry, context and policy
    /// ownership moved out before final listeners run. Listener removal still
    /// takes effect during that final delivery; registration on a closed owner
    /// is inert. Healthy captures retire independently. After the first failure,
    /// remaining notifications are skipped and outgoing captures are retained
    /// without invoking arbitrary destruction.
    ///
    /// The first panic propagates after the terminal state is committed. During
    /// an already active unwind, notifications are skipped and outgoing captures
    /// are retained so the outer failure remains authoritative instead.
    /// As with other containment boundaries, a user value whose own aggregate
    /// destruction double-panics can abort before any outer catch is reached.
    ///
    /// Primary focus is always cleared to `None`, even when `close` runs
    /// reentrantly from inside a focus-change listener — unlike
    /// `request_focus`/[`Self::unfocus`], `close` never takes the private
    /// notification-depth guard itself, so its healthy node-level notifications
    /// run immediately, nested inside whatever notification is already in
    /// flight. Notifications stop at the first failure and are omitted during
    /// an already-active unwind. Its
    /// *manager*-level publication is conditional: it fires `(previous,
    /// None)` to [`FocusChangeCallback`] listeners only when
    /// `notification_depth` reads zero (no outer notification is currently
    /// publishing), and is skipped — rather than interleaved out of order
    /// — otherwise. The net effect matches `request_focus`/`unfocus`'s
    /// ordering goal, reached here by omission instead of participation.
    /// Any request a reentrant listener queues afterward is dropped, never
    /// applied, once closed.
    pub fn close(&self) {
        self.close_with_mode(CloseMode::Ordinary);
    }

    pub(crate) fn close_tombstone(&self) -> CloseTombstone {
        self.close_mode.clone()
    }

    pub(crate) fn close_with_mode(&self, mode: CloseMode) {
        let mut failure = FocusClosePanic::for_close(mode, self.close_mode.clone());
        if self.closed.replace(true) {
            return;
        }
        let pending = std::mem::take(&mut *self.pending_focus_transitions.borrow_mut());
        let previous = self.primary_focus.borrow_mut().take();
        let mut notified = previous.as_ref().map_or_else(Vec::new, |node| {
            node.ancestors()
                .filter(|ancestor| ancestor.parent().is_some())
                .collect()
        });
        notified.extend(previous.iter().cloned());
        let retired =
            FocusNode::close_owned_tree(self.root_scope.as_focus_node(), self.close_mode.clone());
        let global_handlers = std::mem::take(&mut *self.global_key_handlers.borrow_mut());

        for node in notified {
            node.notify_close_listeners(&mut failure);
        }
        if previous.is_some() && self.notification_depth.get() == 0 {
            let ids: Vec<_> = self.listeners.borrow().iter().map(|(id, _)| *id).collect();
            for id in ids {
                let listener = self
                    .listeners
                    .borrow()
                    .iter()
                    .find(|(registered, _)| *registered == id)
                    .map(|(_, listener)| Rc::clone(listener));
                if let Some(listener) = listener {
                    failure.run(|| listener(previous.clone(), None));
                    failure.retire(listener);
                }
            }
        }
        let listeners = std::mem::take(&mut *self.listeners.borrow_mut());
        for (_, listener) in listeners {
            failure.retire(listener);
        }
        for handler in global_handlers {
            failure.retire(handler);
        }
        for node in retired {
            node.retire(&mut failure);
        }
        for node in pending {
            failure.retire(node);
        }
        failure.retire(previous);
        failure.finish();
    }

    /// Whether deterministic teardown has run.
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }
}

impl std::fmt::Debug for FocusManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusManager")
            .field("primary_focus", &self.primary_focus().map(|node| node.id()))
            .field("root_scope_id", &self.root_scope.id())
            .field("listener_count", &self.listeners.borrow().len())
            .field("closed", &self.closed.get())
            .finish_non_exhaustive()
    }
}

impl Drop for FocusManager {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use flui_foundation::geometry::Rect;

    use super::*;
    use crate::{
        events::{Key, KeyState, Modifiers},
        routing::focus_scope::{FocusDetachOutcome, TraversalEdgeBehavior},
    };

    fn manager_with_nodes(count: usize) -> (Rc<FocusManager>, Vec<Rc<FocusNode>>) {
        let manager = FocusManager::new();
        let nodes: Vec<_> = (0..count)
            .map(|index| FocusNode::with_debug_label(format!("node-{index}")))
            .collect();
        for (index, node) in nodes.iter().enumerate() {
            node.set_rect(Rect::from_xywh(index as f64 * 20.0, 0.0, 10.0, 10.0));
            manager.root_scope().attach_node(node).unwrap();
        }
        (manager, nodes)
    }

    fn key_event() -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            ..KeyEvent::default()
        }
    }

    // Focus traversal matrix: key dispatch order, traversal policy, scope memory.
    #[test]
    fn focus_traversal_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "key_dispatch_walks_leaf_to_root_and_honors_skip",
                key_dispatch_walks_leaf_to_root_and_honors_skip,
            ),
            (
                "traversal_uses_policy_order_and_edge_behavior",
                traversal_uses_policy_order_and_edge_behavior,
            ),
            (
                "set_first_focus_restores_the_scopes_remembered_descendant",
                set_first_focus_restores_the_scopes_remembered_descendant,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    fn key_dispatch_walks_leaf_to_root_and_honors_skip() {
        let manager = FocusManager::new();
        let parent = FocusNode::new();
        let child = FocusNode::new();
        manager.root_scope().attach_node(&parent).unwrap();
        parent.attach_node(&child).unwrap();

        let calls = Rc::new(RefCell::new(Vec::new()));
        let leaf_calls = Rc::clone(&calls);
        child.set_on_key_event(Rc::new(move |_| {
            leaf_calls.borrow_mut().push("leaf");
            KeyEventResult::Ignored
        }));
        let parent_calls = Rc::clone(&calls);
        parent.set_on_key_event(Rc::new(move |_| {
            parent_calls.borrow_mut().push("parent");
            KeyEventResult::Handled
        }));
        child.request_focus();

        assert!(manager.dispatch_key_event(&key_event()));
        assert_eq!(calls.borrow().as_slice(), &["leaf", "parent"]);

        child.set_on_key_event(Rc::new(|_| KeyEventResult::SkipRemainingHandlers));
        calls.borrow_mut().clear();
        assert!(!manager.dispatch_key_event(&key_event()));
        assert!(calls.borrow().is_empty());
    }

    fn traversal_uses_policy_order_and_edge_behavior() {
        let (manager, nodes) = manager_with_nodes(2);
        assert!(manager.focus_next());
        assert!(Rc::ptr_eq(
            manager.primary_focus().as_ref().unwrap(),
            &nodes[0]
        ));
        assert!(manager.focus_next());
        assert!(Rc::ptr_eq(
            manager.primary_focus().as_ref().unwrap(),
            &nodes[1]
        ));

        manager
            .root_scope()
            .set_traversal_edge_behavior(TraversalEdgeBehavior::Stop);
        assert!(!manager.focus_next());
        assert!(Rc::ptr_eq(
            manager.primary_focus().as_ref().unwrap(),
            &nodes[1]
        ));
    }

    fn set_first_focus_restores_the_scopes_remembered_descendant() {
        let manager = FocusManager::new();
        let scope = FocusScopeNode::with_debug_label("route");
        let scope_attachment = manager
            .root_scope()
            .attach_node(scope.as_focus_node())
            .unwrap();
        let first = FocusNode::with_debug_label("first");
        let second = FocusNode::with_debug_label("second");
        scope.attach_node(&first).unwrap();
        scope.attach_node(&second).unwrap();

        second.request_focus();
        manager.unfocus();

        assert!(scope.set_first_focus());
        assert!(
            second.has_primary_focus(),
            "route reactivation restores focus history before policy order"
        );

        assert_eq!(scope_attachment.detach(), FocusDetachOutcome::Detached);
        assert!(
            scope.set_first_focus(),
            "a detached scope queues its remembered descendant"
        );
        manager
            .root_scope()
            .attach_node(scope.as_focus_node())
            .unwrap();
        assert!(
            second.has_primary_focus(),
            "reattachment fulfills the remembered descendant request"
        );
    }

    // ── Reentrant focus-notification ordering (issue #1040) ─────────────
    //
    // `set_primary_focus` commits and publishes synchronously. A listener
    // that requests another focus change from inside that publication must
    // not see its request applied out of turn, and no manager listener may
    // ever observe a transition that a later, already-applied one has
    // superseded. These tests pin the ordering contract documented on
    // `FocusManager` and on `request_focus`/`unfocus`/`add_listener`.
    //
    // `tracing` capture for these tests goes through
    // `flui_testing::log_capture::capture`, race-free in a thread-parallel
    // test binary for the reason documented there — this crate sits below
    // `flui-testing` in the layer DAG for everything else, but keeps it as
    // a dev-dependency for exactly this.

    // Focus failure and reentrancy matrix: listener panics, bounded ping-pong,
    /// queued requests against detached targets or a closing manager, and reentrant
    /// requests during notification.
    #[test]
    fn focus_failure_and_reentrancy_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "closed_rejections_preserve_outer_failure_and_healthy_retirement",
                closed_rejections_preserve_outer_failure_and_healthy_retirement,
            ),
            (
                "close_retires_each_capture_and_preserves_first_failure",
                close_retires_each_capture_and_preserves_first_failure,
            ),
            (
                "close_commits_terminal_state_before_reentrant_callbacks",
                close_commits_terminal_state_before_reentrant_callbacks,
            ),
            (
                "global_key_dispatch_skips_removed_live_handlers_and_continues",
                global_key_dispatch_skips_removed_live_handlers_and_continues,
            ),
            (
                "close_during_active_unwind_preserves_outer_failure",
                close_during_active_unwind_preserves_outer_failure,
            ),
            (
                "listener_panic_does_not_leave_notification_depth_stuck",
                listener_panic_does_not_leave_notification_depth_stuck,
            ),
            (
                "ping_pong_listeners_are_bounded_and_warned",
                ping_pong_listeners_are_bounded_and_warned,
            ),
            (
                "queued_focus_target_detached_before_its_turn_is_skipped_not_applied",
                queued_focus_target_detached_before_its_turn_is_skipped_not_applied,
            ),
            (
                "queued_requests_are_dropped_when_the_manager_closes_mid_drain",
                queued_requests_are_dropped_when_the_manager_closes_mid_drain,
            ),
            (
                "reentrant_request_during_notification_is_applied_after_and_published_in_order",
                reentrant_request_during_notification_is_applied_after_and_published_in_order,
            ),
        ];
        if let Ok(case) = std::env::var(HOSTILE_CHILD) {
            let (_, run) = HOSTILE_CASES
                .iter()
                .find(|(name, _)| *name == case)
                .expect("a known hostile case");
            run();
            std::process::exit(CHILD_COMPLETED);
        }
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    /// Names the single hostile case a child process of
    /// `focus_failure_and_reentrancy_matrix` runs instead of the matrix.
    const HOSTILE_CHILD: &str = "FLUI_FOCUS_HOSTILE_CHILD";
    /// A child that ran its case to completion exits with this status, so a
    /// filter that matched no test (status 0) does not pass for one.
    const CHILD_COMPLETED: i32 = 86;
    const HOSTILE_CASES: &[(&str, fn())] = &[
        (
            "close_during_active_unwind_child",
            close_during_active_unwind_child,
        ),
        ("closed_rejections_child", closed_rejections_child),
    ];

    /// Run one hostile case in a child process: what it guards against is an
    /// abort, which would otherwise take the whole test binary down.
    fn run_hostile_case_in_child(case: &str) {
        let mut child = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "routing::focus::tests::focus_failure_and_reentrancy_matrix",
                "--nocapture",
            ])
            .env(HOSTILE_CHILD, case)
            .env("RUST_BACKTRACE", "0")
            .env("RUST_LIB_BACKTRACE", "0")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn hostile case");
        let mut stderr = child.stderr.take().expect("child stderr");
        let stderr_reader = std::thread::spawn(move || {
            let mut output = Vec::new();
            std::io::Read::read_to_end(&mut stderr, &mut output).expect("read child stderr");
            output
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll hostile case") {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().expect("stop timed-out hostile case");
                child.wait().expect("reap hostile case");
                stderr_reader.join().expect("child stderr reader");
                panic!("hostile case `{case}` timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let stderr = stderr_reader.join().expect("child stderr reader");
        assert_eq!(
            status.code(),
            Some(CHILD_COMPLETED),
            "hostile case `{case}` failed: {status}
{}",
            String::from_utf8_lossy(&stderr)
        );
    }

    struct CloseCapture {
        label: &'static str,
        panic: bool,
        drops: Rc<RefCell<Vec<&'static str>>>,
        terminal: Rc<Cell<bool>>,
        node: Weak<FocusNode>,
    }

    impl Drop for CloseCapture {
        fn drop(&mut self) {
            let terminal = self.node.upgrade().is_none_or(|node| {
                !node.is_attached()
                    && matches!(
                        node.request_focus(),
                        crate::routing::FocusRequestOutcome::OwnerClosed
                    )
            });
            self.terminal.set(self.terminal.get() && terminal);
            self.drops.borrow_mut().push(self.label);
            if self.panic {
                std::panic::panic_any(self.label);
            }
        }
    }

    struct ClosingPolicy(CloseCapture);

    impl std::fmt::Debug for ClosingPolicy {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("ClosingPolicy")
        }
    }

    impl crate::routing::FocusTraversalPolicy for ClosingPolicy {
        fn sort_descendants(&self, nodes: &[Rc<FocusNode>]) -> Vec<Rc<FocusNode>> {
            let _ = &self.0;
            nodes.to_vec()
        }
    }

    fn close_retires_each_capture_and_preserves_first_failure() {
        use crate::routing::{FocusRequestOutcome, ReadingOrderPolicy};
        use std::panic::{AssertUnwindSafe, catch_unwind};

        // Final notifications precede outgoing ownership retirement. Each
        // failure point runs alone, followed by chronological competition.
        let cases: &[(&[&str], Option<&str>, &str)] = &[
            (&[], Some("node callback"), "node callback"),
            (&[], Some("manager callback"), "manager callback"),
            (&["node listener"], None, "node listener"),
            (&["manager listener"], None, "manager listener"),
            (&["global handler"], None, "global handler"),
            (&["key handler"], None, "key handler"),
            (&["rect provider"], None, "rect provider"),
            (&["context"], None, "context"),
            (&["policy"], None, "policy"),
            (
                &["global handler", "context"],
                Some("node callback"),
                "node callback",
            ),
            (&["key handler", "rect provider"], None, "key handler"),
            (
                &["manager listener", "global handler"],
                None,
                "manager listener",
            ),
        ];
        for &(panicking_drops, callback_failure, expected) in cases {
            let (manager, nodes) = manager_with_nodes(2);
            nodes[0].request_focus();
            let drops = Rc::new(RefCell::new(Vec::new()));
            let terminal = Rc::new(Cell::new(true));
            let capture = |label| CloseCapture {
                label,
                panic: panicking_drops.contains(&label),
                drops: Rc::clone(&drops),
                terminal: Rc::clone(&terminal),
                node: Rc::downgrade(&nodes[1]),
            };
            let owner = capture("node listener");
            let node_listener = nodes[0].add_listener(Rc::new(move || {
                let _ = &owner;
                assert!(callback_failure != Some("node callback"), "node callback");
            }));
            let owner = capture("manager listener");
            let manager_listener = manager.add_listener(Rc::new(move |_, _| {
                let _ = &owner;
                assert!(
                    callback_failure != Some("manager callback"),
                    "manager callback"
                );
            }));
            let owner = capture("global handler");
            manager.add_global_key_handler(Rc::new(move |_| {
                let _ = &owner;
                false
            }));
            let owner = capture("key handler");
            let key_registration = nodes[1].register_on_key_event(Rc::new(move |_| {
                let _ = &owner;
                KeyEventResult::Handled
            }));
            let owner = capture("rect provider");
            let rect_registration = nodes[1].register_rect_provider(Rc::new(move || {
                let _ = &owner;
                None
            }));
            let context_registration = nodes[1].register_context(Rc::new(capture("context")));
            manager
                .root_scope()
                .set_traversal_policy(Rc::new(ClosingPolicy(capture("policy"))));

            let outcome = catch_unwind(AssertUnwindSafe(|| manager.close()));
            let first = outcome.err().map(|payload| {
                let text =
                    flui_foundation::panic::payload_text(payload.as_ref()).map(str::to_owned);
                flui_foundation::panic::retain_opaque_payload(payload);
                text
            });
            let detached = nodes.iter().all(|node| !node.is_attached());
            let stale_tokens = !key_registration.is_current()
                && !rect_registration.is_current()
                && !context_registration.is_current();
            let cleared = nodes[1].context().is_none()
                && nodes[1].handle_key_event(&key_event()) == KeyEventResult::Ignored;

            // Safe negative control: if old close stopped early, release its
            // leftover captures separately before asserting the defect.
            let _ = catch_unwind(AssertUnwindSafe(|| nodes[0].remove_listener(node_listener)));
            let _ = catch_unwind(AssertUnwindSafe(|| {
                manager.remove_listener(manager_listener);
            }));
            let _ = catch_unwind(AssertUnwindSafe(|| manager.clear_global_key_handlers()));
            let _ = catch_unwind(AssertUnwindSafe(|| nodes[1].clear_on_key_event()));
            let _ = catch_unwind(AssertUnwindSafe(|| nodes[1].clear_rect_provider()));
            let _ = catch_unwind(AssertUnwindSafe(|| {
                nodes[1].register_context(Rc::new(())).relinquish();
            }));
            let _ = catch_unwind(AssertUnwindSafe(|| {
                manager
                    .root_scope()
                    .set_traversal_policy(Rc::new(ReadingOrderPolicy));
            }));

            assert_eq!(
                first.flatten().as_deref(),
                Some(expected),
                "{panicking_drops:?}/{callback_failure:?}"
            );
            assert!(
                detached && cleared && stale_tokens,
                "complete terminal cleanup after {expected}"
            );
            assert!(
                terminal.get(),
                "every capture sees the complete closed tree after {expected}"
            );
            if callback_failure.is_some() {
                assert!(
                    drops.borrow().is_empty(),
                    "captures retained after {expected}"
                );
            } else {
                assert_eq!(
                    drops.borrow().last().copied(),
                    Some(expected),
                    "retirement stops at the first destructor failure"
                );
                assert_eq!(
                    drops
                        .borrow()
                        .iter()
                        .filter(|label| panicking_drops.contains(label))
                        .count(),
                    1
                );
            }
            assert!(matches!(
                nodes[1].request_focus(),
                FocusRequestOutcome::OwnerClosed
            ));
            assert!(manager.root_scope().attach_node(&FocusNode::new()).is_err());
            assert!(!manager.dispatch_key_event(&key_event()));
            manager.close();
        }
    }

    struct CloseBomb(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Drop for CloseBomb {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            panic!("hostile aggregate context field");
        }
    }

    fn close_preserves_first_failure_against_hostile_context() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        struct OpaquePayload(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for OpaquePayload {
            fn drop(&mut self) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                panic!("opaque secondary payload destructor");
            }
        }
        struct ThrowsOpaque(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for ThrowsOpaque {
            fn drop(&mut self) {
                std::panic::panic_any(OpaquePayload(std::sync::Arc::clone(&self.0)));
            }
        }
        for notification_fails in [true, false] {
            let (manager, nodes) = manager_with_nodes(1);
            nodes[0].request_focus();
            nodes[0].add_listener(Rc::new(move || {
                assert!(!notification_fails, "first final notification");
            }));
            let payload_drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let aggregate_drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let captured = ThrowsOpaque(std::sync::Arc::clone(&payload_drops));
            manager.add_global_key_handler(Rc::new(move |_| {
                let _ = &captured;
                false
            }));
            nodes[0]
                .register_context(Rc::new((
                    CloseBomb(std::sync::Arc::clone(&aggregate_drops)),
                    CloseBomb(std::sync::Arc::clone(&aggregate_drops)),
                )))
                .relinquish();
            let outcome = catch_unwind(AssertUnwindSafe(|| manager.close()));
            let payload = outcome.expect_err("the first failure propagates");
            if notification_fails {
                assert_eq!(
                    flui_foundation::panic::payload_text(payload.as_ref()),
                    Some("first final notification")
                );
            } else {
                assert!(
                    payload.is::<OpaquePayload>(),
                    "the original opaque destructor payload propagates"
                );
            }
            flui_foundation::panic::retain_opaque_payload(payload);
            assert!(!nodes[0].is_attached());
            assert!(nodes[0].context().is_none());
            assert_eq!(payload_drops.load(std::sync::atomic::Ordering::Relaxed), 0);
            assert_eq!(
                aggregate_drops.load(std::sync::atomic::Ordering::Relaxed),
                0
            );
        }
    }

    fn close_commits_terminal_state_before_reentrant_callbacks() {
        use crate::routing::FocusRequestOutcome;

        let (manager, nodes) = manager_with_nodes(2);
        nodes[0].request_focus();
        let other = Rc::clone(&nodes[1]);
        let owner = Rc::downgrade(&manager);
        let notified = Rc::new(Cell::new(false));
        let observed = Rc::clone(&notified);
        nodes[0].add_listener(Rc::new(move || {
            let manager = owner.upgrade().expect("the caller retains the manager");
            assert!(manager.primary_focus().is_none());
            assert!(!other.is_attached());
            assert!(matches!(
                other.request_focus(),
                FocusRequestOutcome::OwnerClosed
            ));
            other.set_on_key_event(Rc::new(|_| panic!("closed node accepted a key handler")));
            other.register_context(Rc::new(())).relinquish();
            manager.add_listener(Rc::new(|_, _| panic!("closed owner accepted a listener")));
            manager
                .add_global_key_handler(Rc::new(|_| panic!("closed owner accepted a key handler")));
            manager.close();
            observed.set(true);
        }));
        manager.close();
        assert!(notified.get());
        assert!(nodes[1].context().is_none());
        assert_eq!(
            nodes[1].handle_key_event(&key_event()),
            KeyEventResult::Ignored
        );
        assert!(!manager.dispatch_key_event(&key_event()));
        assert_eq!(manager.listener_count(), 0);
    }

    fn global_key_dispatch_skips_removed_live_handlers_and_continues() {
        let manager = FocusManager::new();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&calls);
        let later: KeyEventCallback = Rc::new(move |_| {
            recorded.borrow_mut().push("later healthy handler");
            true
        });
        let recorded = Rc::clone(&calls);
        let kept_removed: KeyEventCallback = Rc::new(move |_| {
            recorded.borrow_mut().push("removed handler");
            false
        });
        let owner = Rc::downgrade(&manager);
        let surviving = Rc::clone(&later);
        let recorded = Rc::clone(&calls);
        manager.add_global_key_handler(Rc::new(move |_| {
            recorded.borrow_mut().push("removing handler");
            let owner = owner.upgrade().expect("the caller retains the manager");
            owner.clear_global_key_handlers();
            // Keep only the later snapshot identity registered. The removed
            // callback still has an independent consumer-owned Rc below.
            owner.add_global_key_handler(Rc::clone(&surviving));
            false
        }));
        manager.add_global_key_handler(Rc::clone(&kept_removed));
        manager.add_global_key_handler(later);

        assert!(manager.dispatch_key_event(&key_event()));
        assert_eq!(
            *calls.borrow(),
            ["removing handler", "later healthy handler"]
        );
        calls.borrow_mut().clear();
        assert!(manager.dispatch_key_event(&key_event()));
        assert_eq!(*calls.borrow(), ["later healthy handler"]);
        drop(kept_removed);
        manager.close();
    }

    fn close_from_key_callback_preserves_first_failure_against_hostile_capture() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        for global in [true, false] {
            let (manager, nodes) = manager_with_nodes(1);
            nodes[0].request_focus();
            let capture_drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let hostile = (
                CloseBomb(std::sync::Arc::clone(&capture_drops)),
                CloseBomb(std::sync::Arc::clone(&capture_drops)),
            );
            let owner = Rc::downgrade(&manager);
            let callback = move || -> ! {
                let _ = &hostile;
                owner
                    .upgrade()
                    .expect("the dispatch caller retains the owner")
                    .close();
                panic!("first key callback");
            };
            if global {
                manager.add_global_key_handler(Rc::new(move |_| callback()));
            } else {
                nodes[0].set_on_key_event(Rc::new(move |_| callback()));
            }
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                manager.dispatch_key_event(&key_event())
            }));
            let payload = outcome.expect_err("the original key callback failure propagates");
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some("first key callback")
            );
            flui_foundation::panic::retain_opaque_payload(payload);
            assert!(!nodes[0].is_attached());
            assert_eq!(capture_drops.load(std::sync::atomic::Ordering::Relaxed), 0);
            assert!(!manager.dispatch_key_event(&key_event()));
        }
    }

    fn close_during_active_unwind_preserves_outer_failure() {
        run_hostile_case_in_child("close_during_active_unwind_child");
    }

    fn close_during_active_unwind_child() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        close_preserves_first_failure_against_hostile_context();
        close_from_key_callback_preserves_first_failure_against_hostile_capture();
        let survivor = Rc::new(RefCell::new(None));
        let aggregate_drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let (manager, nodes) = manager_with_nodes(1);
            *survivor.borrow_mut() = Some(Rc::clone(&nodes[0]));
            nodes[0].request_focus();
            nodes[0].add_listener(Rc::new(|| panic!("secondary final listener")));
            nodes[0]
                .register_context(Rc::new((
                    CloseBomb(std::sync::Arc::clone(&aggregate_drops)),
                    CloseBomb(std::sync::Arc::clone(&aggregate_drops)),
                )))
                .relinquish();
            // The final strong manager owner drops during this unwind.
            let _ = &manager;
            panic!("outer failure");
        }));
        let payload = outcome.expect_err("the outer panic propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(payload.as_ref()),
            Some("outer failure")
        );
        flui_foundation::panic::retain_opaque_payload(payload);
        assert_eq!(
            aggregate_drops.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        let node = survivor
            .borrow()
            .as_ref()
            .expect("the surviving public node")
            .clone();
        assert!(!node.is_attached());
        assert!(matches!(
            node.request_focus(),
            crate::routing::FocusRequestOutcome::OwnerClosed
        ));
    }

    struct RejectedPolicy<T>(T);

    impl<T> std::fmt::Debug for RejectedPolicy<T> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("RejectedPolicy")
        }
    }

    impl<T> crate::routing::FocusTraversalPolicy for RejectedPolicy<T> {
        fn sort_descendants(&self, _: &[Rc<FocusNode>]) -> Vec<Rc<FocusNode>> {
            panic!("closed scope accepted policy")
        }
    }

    fn reject_focus_capture<T: 'static>(
        manager: &Rc<FocusManager>,
        node: &Rc<FocusNode>,
        kind: usize,
        capture: T,
    ) {
        match kind {
            0 => manager.add_global_key_handler(Rc::new(move |_| {
                let _ = &capture;
                panic!("closed manager accepted global handler");
            })),
            1 => {
                manager.add_listener(Rc::new(move |_, _| {
                    let _ = &capture;
                    panic!("closed manager accepted listener");
                }));
            }
            2 => {
                node.add_listener(Rc::new(move || {
                    let _ = &capture;
                    panic!("closed node accepted listener");
                }));
            }
            3 => node.set_rect_provider(Rc::new(move || {
                let _ = &capture;
                panic!("closed node accepted rectangle provider");
            })),
            4 => {
                assert!(!node.register_context(Rc::new(capture)).is_current());
            }
            5 => node.set_on_key_event(Rc::new(move |_| {
                let _ = &capture;
                panic!("closed node accepted key handler");
            })),
            6 => manager
                .root_scope()
                .set_traversal_policy(Rc::new(RejectedPolicy(capture))),
            _ => panic!("unknown rejection case"),
        }
    }

    struct RejectedCapture {
        owner: Weak<FocusManager>,
        node: Weak<FocusNode>,
        drops: Rc<Cell<usize>>,
        panic: bool,
    }

    impl Drop for RejectedCapture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            let manager = self.owner.upgrade().expect("caller retains closed manager");
            let node = self.node.upgrade().expect("caller retains closed node");
            assert!(manager.is_closed());
            assert!(!node.is_attached());
            assert!(matches!(
                node.request_focus(),
                crate::routing::FocusRequestOutcome::OwnerClosed
            ));
            manager.add_global_key_handler(Rc::new(|_| panic!("reentry accepted handler")));
            node.clear_on_key_event();
            assert!(!self.panic, "first rejected capture retirement");
        }
    }

    fn closed_rejections_preserve_outer_failure_and_healthy_retirement() {
        run_hostile_case_in_child("closed_rejections_child");
    }

    fn closed_rejections_child() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        struct RejectOnDrop {
            manager: Rc<FocusManager>,
            node: Rc<FocusNode>,
            kind: usize,
            drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }
        impl Drop for RejectOnDrop {
            fn drop(&mut self) {
                reject_focus_capture(
                    &self.manager,
                    &self.node,
                    self.kind,
                    (
                        CloseBomb(std::sync::Arc::clone(&self.drops)),
                        CloseBomb(std::sync::Arc::clone(&self.drops)),
                    ),
                );
            }
        }
        for kind in 0..7 {
            let (manager, nodes) = manager_with_nodes(1);
            manager.close();
            let drops = Rc::new(Cell::new(0));
            for panic in [false, true] {
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    reject_focus_capture(
                        &manager,
                        &nodes[0],
                        kind,
                        RejectedCapture {
                            owner: Rc::downgrade(&manager),
                            node: Rc::downgrade(&nodes[0]),
                            drops: Rc::clone(&drops),
                            panic,
                        },
                    );
                }));
                if panic {
                    let payload =
                        outcome.expect_err("ordinary rejected capture destruction propagates");
                    assert_eq!(
                        flui_foundation::panic::payload_text(payload.as_ref()),
                        Some("first rejected capture retirement")
                    );
                    flui_foundation::panic::retain_opaque_payload(payload);
                } else {
                    outcome.expect("healthy rejected capture destruction succeeds");
                }
            }
            assert_eq!(
                drops.get(),
                2,
                "healthy rejection destroys each incoming capture: {kind}"
            );
            let hostile_drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let _guard = RejectOnDrop {
                    manager: Rc::clone(&manager),
                    node: Rc::clone(&nodes[0]),
                    kind,
                    drops: std::sync::Arc::clone(&hostile_drops),
                };
                panic!("outer rejected owner failure");
            }));
            let payload = outcome.expect_err("outer failure propagates unchanged");
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some("outer rejected owner failure")
            );
            flui_foundation::panic::retain_opaque_payload(payload);
            assert_eq!(
                hostile_drops.load(std::sync::atomic::Ordering::Relaxed),
                0,
                "unwinding rejection retains aggregate: {kind}"
            );
            assert!(!manager.dispatch_key_event(&key_event()));
            assert!(nodes[0].context().is_none());
            reject_focus_capture(
                &manager,
                &nodes[0],
                kind,
                RejectedCapture {
                    owner: Rc::downgrade(&manager),
                    node: Rc::downgrade(&nodes[0]),
                    drops: Rc::clone(&drops),
                    panic: false,
                },
            );
            assert_eq!(
                drops.get(),
                3,
                "later healthy rejection resumes destruction: {kind}"
            );
            manager.close();
            // A different owner still accepts and delivers the next operation.
            let (healthy, healthy_nodes) = manager_with_nodes(1);
            let calls = Rc::new(Cell::new(0));
            let count = Rc::clone(&calls);
            healthy_nodes[0].set_on_key_event(Rc::new(move |_| {
                count.set(count.get() + 1);
                KeyEventResult::Handled
            }));
            healthy_nodes[0].request_focus();
            assert!(healthy.dispatch_key_event(&key_event()));
            assert_eq!(calls.get(), 1);
        }
    }

    /// The issue #1040 reproducer: A is focused, B is requested, and B's
    /// own node listener reentrantly requests C while B is (momentarily)
    /// primary. The reentrant request must be applied only after the
    /// outer A -> B notification finishes, and published in the order
    /// requested — never the reversed `[(B, C), (A, B)]` the pre-fix code
    /// produced.
    fn reentrant_request_during_notification_is_applied_after_and_published_in_order() {
        let (manager, nodes) = manager_with_nodes(3);
        nodes[0].request_focus();

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));

        let next = Rc::clone(&nodes[2]);
        let intermediate = Rc::downgrade(&nodes[1]);
        nodes[1].add_listener(Rc::new(move || {
            if intermediate.upgrade().unwrap().has_primary_focus() {
                next.request_focus();
            }
        }));

        nodes[1].request_focus();

        assert!(nodes[2].has_primary_focus());
        assert_eq!(
            edges.borrow().as_slice(),
            &[
                (Some(nodes[0].id()), Some(nodes[1].id())),
                (Some(nodes[1].id()), Some(nodes[2].id())),
            ]
        );
    }

    /// A queued transition's eligibility is checked again immediately
    /// before it is applied, not only when it was accepted: B's listener
    /// requests C, then detaches C before the outer notification finishes.
    /// The drain must skip the now-detached target rather than committing
    /// it as primary.
    fn queued_focus_target_detached_before_its_turn_is_skipped_not_applied() {
        let manager = FocusManager::new();
        let a = FocusNode::with_debug_label("a");
        manager.root_scope().attach_node(&a).unwrap();
        let b = FocusNode::with_debug_label("b");
        manager.root_scope().attach_node(&b).unwrap();
        let c = FocusNode::with_debug_label("c");
        let c_attachment = manager.root_scope().attach_node(&c).unwrap();
        a.request_focus();

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));

        let c_for_listener = Rc::clone(&c);
        let b_weak = Rc::downgrade(&b);
        b.add_listener(Rc::new(move || {
            if b_weak.upgrade().unwrap().has_primary_focus() {
                c_for_listener.request_focus();
                c_attachment.detach();
            }
        }));

        b.request_focus();

        assert!(
            b.has_primary_focus(),
            "the detached queued target must not become primary"
        );
        assert!(!c.is_attached());
        assert_eq!(
            edges.borrow().as_slice(),
            &[(Some(a.id()), Some(b.id()))],
            "a queued transition whose target detached before its turn runs must be \
             skipped entirely, publishing no (B, C) edge"
        );
    }

    /// A listener that panics mid-notification must not leave
    /// `notification_depth` stuck above zero: every later, healthy
    /// `request_focus` on this manager would otherwise queue silently and
    /// never apply, since nothing would ever bring the depth back to zero
    /// to drain it.
    fn listener_panic_does_not_leave_notification_depth_stuck() {
        let (manager, nodes) = manager_with_nodes(2);
        nodes[0].request_focus();

        let panicking_listener =
            nodes[0].add_listener(Rc::new(|| panic!("boom: listener under test panics")));

        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            nodes[1].request_focus();
        }));
        assert!(panicked.is_err(), "the listener panic must propagate");
        // Uninstall it: it would otherwise re-panic on every later
        // notification that touches node 0, including the manager's own
        // teardown at the end of this test — this test is about
        // `notification_depth` recovering from ONE panic, not about a
        // permanently misbehaving listener.
        nodes[0].remove_listener(panicking_listener);

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));

        nodes[0].request_focus();

        assert!(
            nodes[0].has_primary_focus(),
            "a healthy call after a listener panic must apply immediately, not queue \
             forever behind a notification_depth stuck above zero"
        );
        assert_eq!(
            edges.borrow().as_slice(),
            &[(Some(nodes[1].id()), Some(nodes[0].id()))]
        );
    }

    /// Two listeners that keep redirecting focus to each other cannot spin
    /// the caller forever: FLUI applies transitions synchronously, so the
    /// drain is bounded at
    /// `FocusManager::REENTRANT_FOCUS_DRAIN_BUDGET` applications and warns
    /// once when it drops the rest.
    fn ping_pong_listeners_are_bounded_and_warned() {
        let (manager, nodes) = manager_with_nodes(2);

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));

        // Each listener re-requests the other node only while it is itself
        // the current primary, so every application enqueues exactly one
        // more — a clean, unbounded ping-pong rather than one that would
        // also waste applications re-requesting the already-current node.
        let target_of_0 = Rc::clone(&nodes[1]);
        let self_of_0 = Rc::downgrade(&nodes[0]);
        let listener_0 = nodes[0].add_listener(Rc::new(move || {
            if self_of_0.upgrade().unwrap().has_primary_focus() {
                target_of_0.request_focus();
            }
        }));
        let target_of_1 = Rc::clone(&nodes[0]);
        let self_of_1 = Rc::downgrade(&nodes[1]);
        let listener_1 = nodes[1].add_listener(Rc::new(move || {
            if self_of_1.upgrade().unwrap().has_primary_focus() {
                target_of_1.request_focus();
            }
        }));

        let ((), log) = flui_testing::log_capture::capture(|| {
            nodes[0].request_focus();
        });

        assert_eq!(
            edges.borrow().len(),
            FocusManager::REENTRANT_FOCUS_DRAIN_BUDGET + 1,
            "the initial (non-reentrant) transition plus one published edge \
             per budgeted drain application"
        );
        assert_eq!(
            log.count_containing("drain budget"),
            1,
            "the latched warning must fire exactly once, not once per dropped entry: {log}"
        );

        // The concrete node left focused after the drop must match the
        // last transition the drain actually committed, not a hardcoded
        // parity assumption — read it back from the same trace-level
        // "focus changed" events `apply_focus_transition` emits.
        let last_committed = log
            .records()
            .iter()
            .rev()
            .find(|record| record.message == "focus changed")
            .expect("the drain must have committed at least one transition");
        let expected_primary = last_committed
            .field("new")
            .expect("every \"focus changed\" event carries a `new` field")
            .to_owned();
        assert_eq!(
            format!("{:?}", manager.primary_focus().map(|node| node.id().get())),
            expected_primary,
            "the manager's committed primary must match the last transition actually \
             applied, not whatever the dropped queue tail would have produced"
        );

        // The budget resets per outermost call rather than leaking state
        // across calls: with the ping-pong wiring removed, a plain request
        // from outside now behaves like any other healthy transition —
        // applied immediately, publishing exactly one edge — proving the
        // manager was left fully functional after the bounded drop.
        nodes[0].remove_listener(listener_0);
        nodes[1].remove_listener(listener_1);
        edges.borrow_mut().clear();
        let settled = manager
            .primary_focus()
            .expect("a node is focused after the drop");
        let other = if Rc::ptr_eq(&settled, &nodes[0]) {
            Rc::clone(&nodes[1])
        } else {
            Rc::clone(&nodes[0])
        };

        other.request_focus();

        assert!(other.has_primary_focus());
        assert_eq!(
            edges.borrow().as_slice(),
            &[(Some(settled.id()), Some(other.id()))],
            "a healthy call after the bounded drop must apply as a single ordinary \
             transition, not still be primed to warn or queue from the earlier storm"
        );
    }

    /// Once the manager closes — even reentrantly, from inside a
    /// notification already in flight — no further manager-level
    /// publication happens and any request still queued by a reentrant
    /// listener is dropped rather than applied.
    fn queued_requests_are_dropped_when_the_manager_closes_mid_drain() {
        let (manager, nodes) = manager_with_nodes(3);
        nodes[0].request_focus();

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));

        let manager_for_listener = Rc::clone(&manager);
        let sibling = Rc::clone(&nodes[2]);
        nodes[1].add_listener(Rc::new(move || {
            manager_for_listener.close();
            sibling.request_focus();
        }));

        nodes[1].request_focus();

        assert!(
            edges.borrow().is_empty(),
            "closing mid-notification drops every pending and in-flight publication"
        );
        assert!(manager.primary_focus().is_none());
        assert!(manager.is_closed());
    }
}
