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
/// for how this compares to Flutter's microtask-deferred model.
pub struct FocusManager {
    root_scope: Rc<FocusScopeNode>,
    primary_focus: RefCell<Option<Rc<FocusNode>>>,
    listeners: RefCell<Vec<(ListenerId, FocusChangeCallback)>>,
    next_listener_id: Cell<usize>,
    global_key_handlers: RefCell<Vec<KeyEventCallback>>,
    /// Where a key starts its leaf-to-root walk while nothing is focused
    /// ([`Self::set_unfocused_key_target`]).
    unfocused_key_target: RefCell<Weak<FocusNode>>,
    closed: Cell<bool>,
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
    /// FLUI applies focus transitions synchronously, unlike Flutter's
    /// microtask-scheduled `applyFocusChangesIfNeeded`
    /// (`focus_manager.dart`), which merely yields a frame per bounce —
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
            unfocused_key_target: RefCell::new(Weak::new()),
            closed: Cell::new(false),
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
        self.listeners.borrow_mut().push((id, callback));
        id
    }

    /// Remove one focus-change listener.
    pub fn remove_listener(&self, id: ListenerId) {
        let mut listeners = std::mem::take(&mut *self.listeners.borrow_mut());
        listeners.retain(|(held, _)| *held != id);
        let _prev = std::mem::replace(&mut *self.listeners.borrow_mut(), listeners);
    }

    /// Remove all focus-change listeners.
    pub fn clear_listeners(&self) {
        let _prev = std::mem::take(&mut *self.listeners.borrow_mut());
    }

    /// Number of registered listeners.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn listener_count(&self) -> usize {
        self.listeners.borrow().len()
    }

    fn notify_listeners(&self, previous: Option<Rc<FocusNode>>, new: Option<Rc<FocusNode>>) {
        let listeners = self.listeners.borrow().clone();
        for (id, listener) in listeners {
            // A listener already dispatched in this loop may have removed
            // a later one (itself included) — skip it, matching Flutter's
            // `_HighlightModeManager.notifyListeners`
            // (`if (_listeners.contains(listener))`): once removed, a
            // listener is never called again, even mid-dispatch.
            let still_registered = self.listeners.borrow().iter().any(|(held, _)| *held == id);
            if still_registered {
                listener(previous.clone(), new.clone());
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
    pub fn add_global_key_handler(&self, handler: KeyEventCallback) {
        self.global_key_handlers.borrow_mut().push(handler);
    }

    /// Remove all global key handlers.
    pub fn clear_global_key_handlers(&self) {
        let _prev = std::mem::take(&mut *self.global_key_handlers.borrow_mut());
    }

    /// Dispatch a key event through global handlers, then focused leaf to root.
    pub fn dispatch_key_event(&self, event: &KeyEvent) -> bool {
        if self.closed.get() {
            return false;
        }

        let global_handlers = self.global_key_handlers.borrow().clone();
        for handler in global_handlers {
            if handler(event) {
                tracing::trace!("key event handled by global focus handler");
                return true;
            }
        }

        let unfocused_target = || self.unfocused_key_target.borrow().upgrade();
        let Some(focused) = self.primary_focus().or_else(unfocused_target) else {
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

    /// Name the node a key starts its walk at while nothing is focused, or
    /// `None` to stop.
    ///
    /// The walk normally starts at the primary focus. A window opened with
    /// nothing focused has none, so without a target every key is dropped —
    /// including the first Tab that would bring the focus in. Flutter never
    /// has that state: its primary focus falls back to the root scope, and an
    /// app's route scope sits under the `WidgetsApp` shortcuts. FLUI's
    /// default bindings name their own node here instead, so the walk reaches
    /// them and nothing about `primary_focus` changes. Held weakly: the node's
    /// owner decides its lifetime.
    pub fn set_unfocused_key_target(&self, node: Option<&Rc<FocusNode>>) {
        *self.unfocused_key_target.borrow_mut() = node.map_or_else(Weak::new, Rc::downgrade);
    }

    /// The node [`Self::set_unfocused_key_target`] named, while it lives.
    #[must_use]
    pub fn unfocused_key_target(&self) -> Option<Rc<FocusNode>> {
        self.unfocused_key_target.borrow().upgrade()
    }

    /// Deterministically retire this focus owner.
    ///
    /// Closing is idempotent. It sends the final focus-loss notification,
    /// clears manager and node callbacks, and tombstones every owned node.
    /// Tombstoned nodes cannot later attach to a different manager.
    ///
    /// Primary focus is always cleared to `None`, even when `close` runs
    /// reentrantly from inside a focus-change listener — unlike
    /// `request_focus`/[`Self::unfocus`], `close` never takes the private
    /// notification-depth guard itself, so its own node-level notification
    /// (the private `notify_focus_nodes`) always runs immediately, nested
    /// inside whatever notification is already in flight. Only its
    /// *manager*-level publication is conditional: it fires `(previous,
    /// None)` to [`FocusChangeCallback`] listeners only when
    /// `notification_depth` reads zero (no outer notification is currently
    /// publishing), and is skipped — rather than interleaved out of order
    /// — otherwise. The net effect matches `request_focus`/`unfocus`'s
    /// ordering goal, reached here by omission instead of participation.
    /// Any request a reentrant listener queues afterward is dropped, never
    /// applied, once closed.
    pub fn close(&self) {
        if self.closed.replace(true) {
            return;
        }

        {
            let _prev = std::mem::take(&mut *self.pending_focus_transitions.borrow_mut());
        }
        let previous = self.primary_focus.borrow_mut().take();
        if let Some(previous) = previous {
            Self::notify_focus_nodes(Some(&previous), None);
            if self.notification_depth.get() == 0 {
                self.notify_listeners(Some(previous), None);
            }
        }
        {
            let _prev = std::mem::take(&mut *self.listeners.borrow_mut());
        }
        {
            let _prev = std::mem::take(&mut *self.global_key_handlers.borrow_mut());
        }
        FocusNode::close_owned_tree(self.root_scope.as_focus_node());
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
    use std::cell::{Cell, RefCell};

    use flui_types::geometry::{Pixels, Rect};

    use super::*;
    use crate::{
        events::{Key, KeyState, Modifiers},
        routing::focus_scope::{
            FocusDetachOutcome, FocusRequestOutcome, FocusTreeError, TraversalEdgeBehavior,
        },
    };

    fn manager_with_nodes(count: usize) -> (Rc<FocusManager>, Vec<Rc<FocusNode>>) {
        let manager = FocusManager::new();
        let nodes: Vec<_> = (0..count)
            .map(|index| FocusNode::with_debug_label(format!("node-{index}")))
            .collect();
        for (index, node) in nodes.iter().enumerate() {
            node.set_rect(Rect::from_xywh(
                Pixels(index as f32 * 20.0),
                Pixels(0.0),
                Pixels(10.0),
                Pixels(10.0),
            ));
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

    #[test]
    fn managers_are_isolated_and_roots_are_bound() {
        let first = FocusManager::new();
        let second = FocusManager::new();

        assert!(!Rc::ptr_eq(&first, &second));
        assert!(first.root_scope().as_focus_node().is_attached());
        assert!(second.root_scope().as_focus_node().is_attached());
        assert!(!Rc::ptr_eq(
            first.root_scope().as_focus_node(),
            second.root_scope().as_focus_node()
        ));
    }

    #[test]
    fn focus_change_is_node_typed_and_manager_local() {
        let (manager, nodes) = manager_with_nodes(2);
        let changes = Rc::new(RefCell::new(Vec::new()));
        let changes_for_listener = Rc::clone(&changes);
        manager.add_listener(Rc::new(move |previous, next| {
            changes_for_listener
                .borrow_mut()
                .push((previous.map(|node| node.id()), next.map(|node| node.id())));
        }));

        assert_eq!(nodes[0].request_focus(), FocusRequestOutcome::Focused);
        assert_eq!(nodes[1].request_focus(), FocusRequestOutcome::Focused);
        assert!(Rc::ptr_eq(
            manager.primary_focus().as_ref().unwrap(),
            &nodes[1]
        ));
        assert_eq!(
            changes.borrow().as_slice(),
            &[
                (None, Some(nodes[0].id())),
                (Some(nodes[0].id()), Some(nodes[1].id()))
            ]
        );
    }

    #[test]
    fn detached_request_is_fulfilled_when_bound() {
        let manager = FocusManager::new();
        let node = FocusNode::new();
        assert_eq!(node.request_focus(), FocusRequestOutcome::Queued);
        let attachment = manager.root_scope().attach_node(&node).unwrap();

        assert!(attachment.is_attached());
        assert!(node.has_primary_focus());
    }

    #[test]
    fn cross_manager_aliasing_is_rejected() {
        let first = FocusManager::new();
        let second = FocusManager::new();
        let node = FocusNode::new();
        first.root_scope().attach_node(&node).unwrap();

        let error = second.root_scope().attach_node(&node).unwrap_err();
        assert!(matches!(
            error,
            FocusTreeError::AlreadyAttached { .. } | FocusTreeError::ManagerMismatch { .. }
        ));
    }

    #[test]
    fn stale_attachment_cannot_detach_a_reparented_node() {
        let manager = FocusManager::new();
        let first_parent = FocusScopeNode::new();
        let second_parent = FocusScopeNode::new();
        manager
            .root_scope()
            .attach_node(first_parent.as_focus_node())
            .unwrap();
        manager
            .root_scope()
            .attach_node(second_parent.as_focus_node())
            .unwrap();
        let node = FocusNode::new();
        let stale = first_parent.attach_node(&node).unwrap();
        let current = second_parent.adopt_node(&node).unwrap();

        assert_eq!(stale.detach(), FocusDetachOutcome::Stale);
        assert_eq!(current.detach(), FocusDetachOutcome::Detached);
    }

    #[test]
    fn replacing_an_attached_ancestor_preserves_descendant_attachment_and_focus() {
        let manager = FocusManager::new();
        let old_parent = FocusNode::with_debug_label("old-parent");
        let old_attachment = manager.root_scope().attach_node(&old_parent).unwrap();
        let child = FocusNode::with_debug_label("child");
        let child_attachment = old_parent.attach_node(&child).unwrap();
        let leaf = FocusNode::with_debug_label("leaf");
        child.attach_node(&leaf).unwrap();
        leaf.request_focus();

        let manager_edges = Rc::new(RefCell::new(Vec::new()));
        let manager_edges_for_listener = Rc::clone(&manager_edges);
        manager.add_listener(Rc::new(move |previous, current| {
            manager_edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));
        let old_notifications = Rc::new(Cell::new(0));
        let old_notifications_for_listener = Rc::clone(&old_notifications);
        old_parent.add_listener(Rc::new(move || {
            old_notifications_for_listener.set(old_notifications_for_listener.get() + 1);
        }));
        let replacement = FocusNode::with_debug_label("replacement");
        let replacement_notifications = Rc::new(Cell::new(0));
        let replacement_notifications_for_listener = Rc::clone(&replacement_notifications);
        replacement.add_listener(Rc::new(move || {
            replacement_notifications_for_listener
                .set(replacement_notifications_for_listener.get() + 1);
        }));

        let replacement_attachment = old_attachment.replace_node(&replacement).unwrap();

        assert!(!old_attachment.is_attached());
        assert_eq!(old_attachment.detach(), FocusDetachOutcome::Stale);
        assert!(replacement_attachment.is_attached());
        assert!(!old_parent.is_attached());
        assert!(old_parent.parent().is_none());
        assert!(old_parent.children().is_empty());
        assert_eq!(
            replacement.parent().map(|parent| parent.id()),
            Some(manager.root_scope().id())
        );
        assert_eq!(
            replacement.children().first().map(|node| node.id()),
            Some(child.id())
        );
        assert_eq!(
            child.parent().map(|parent| parent.id()),
            Some(replacement.id())
        );
        assert!(
            child_attachment.is_attached(),
            "replacing an ancestor must not supersede a descendant's attachment"
        );
        assert!(leaf.has_primary_focus());
        assert!(
            manager_edges.borrow().is_empty(),
            "primary focus identity never moved (still `leaf`), so the manager-level \
             contract publishes no edge; the ancestry change is carried by the \
             node-level listeners asserted below (old_notifications / \
             replacement_notifications), not by a same-identity manager edge"
        );
        assert_eq!(old_notifications.get(), 1);
        assert_eq!(replacement_notifications.get(), 1);
        assert_eq!(
            replacement_attachment.detach(),
            FocusDetachOutcome::Detached
        );
    }

    #[test]
    fn replacing_the_primary_node_releases_focus() {
        let manager = FocusManager::new();
        let old = FocusNode::with_debug_label("old");
        let old_attachment = manager.root_scope().attach_node(&old).unwrap();
        old.request_focus();
        let replacement = FocusNode::with_debug_label("replacement");

        let replacement_attachment = old_attachment.replace_node(&replacement).unwrap();

        assert!(manager.primary_focus().is_none());
        assert!(!old.is_attached());
        assert!(replacement_attachment.is_attached());
        assert!(!replacement.has_focus());
    }

    #[test]
    fn replacement_releases_a_descendant_that_the_new_ancestor_disallows() {
        let manager = FocusManager::new();
        let old_parent = FocusNode::with_debug_label("old-parent");
        let old_attachment = manager.root_scope().attach_node(&old_parent).unwrap();
        let child = FocusNode::with_debug_label("child");
        old_parent.attach_node(&child).unwrap();
        child.request_focus();
        let replacement = FocusNode::with_debug_label("replacement");
        replacement.set_descendants_are_focusable(false);

        old_attachment.replace_node(&replacement).unwrap();

        assert!(manager.primary_focus().is_none());
        assert!(!child.can_request_focus());
    }

    /// The replacement itself does not move primary focus (`child` stays
    /// primary throughout the structural swap), so `finish_node_replacement`
    /// publishes no outer edge for it — see its ordering contract. The
    /// reentrant `sibling.request_focus()` made from inside the node-level
    /// replacement notification is queued (`notification_depth` is held
    /// above zero for that notification) and applied only once it
    /// completes, publishing exactly one edge, `(child, sibling)`, in the
    /// order it was requested — never interleaved with, or preceding, a
    /// transition that had not happened yet (issue #1040).
    #[test]
    fn replacement_notification_reentry_does_not_emit_a_stale_outer_edge() {
        let manager = FocusManager::new();
        let old_parent = FocusNode::with_debug_label("old-parent");
        let old_attachment = manager.root_scope().attach_node(&old_parent).unwrap();
        let child = FocusNode::with_debug_label("child");
        old_parent.attach_node(&child).unwrap();
        let sibling = FocusNode::with_debug_label("sibling");
        manager.root_scope().attach_node(&sibling).unwrap();
        child.request_focus();

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));
        let replacement = FocusNode::with_debug_label("replacement");
        let sibling_for_listener = Rc::clone(&sibling);
        let replacement_for_listener = Rc::downgrade(&replacement);
        let child_for_listener = Rc::downgrade(&child);
        replacement.add_listener(Rc::new(move || {
            let replacement = replacement_for_listener.upgrade().unwrap();
            let child = child_for_listener.upgrade().unwrap();
            assert_eq!(
                child.parent().map(|parent| parent.id()),
                Some(replacement.id()),
                "callbacks observe the completed structural transaction"
            );
            sibling_for_listener.request_focus();
        }));

        old_attachment.replace_node(&replacement).unwrap();

        assert!(sibling.has_primary_focus());
        assert_eq!(
            edges.borrow().as_slice(),
            &[(Some(child.id()), Some(sibling.id()))]
        );
    }

    /// Companion to the test above: here the replaced node itself (`old`)
    /// was primary, so the replacement genuinely releases focus (the
    /// existing, unchanged contract — see `replacing_the_primary_node_releases_focus`)
    /// and `finish_node_replacement` DOES publish that outer edge,
    /// `(old, None)`. A reentrant `sibling.request_focus()` made from
    /// `old`'s own node-level listener during that notification is still
    /// queued and applied only afterward, publishing its own edge,
    /// `(None, sibling)`, in order — the outer edge is not lost, and the
    /// reentrant one is not interleaved ahead of it.
    #[test]
    fn replacement_reentry_after_a_real_outer_edge_orders_both_transitions() {
        let manager = FocusManager::new();
        let old = FocusNode::with_debug_label("old");
        let old_attachment = manager.root_scope().attach_node(&old).unwrap();
        let sibling = FocusNode::with_debug_label("sibling");
        manager.root_scope().attach_node(&sibling).unwrap();
        old.request_focus();

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));
        let replacement = FocusNode::with_debug_label("replacement");
        let sibling_for_listener = Rc::clone(&sibling);
        old.add_listener(Rc::new(move || {
            sibling_for_listener.request_focus();
        }));

        old_attachment.replace_node(&replacement).unwrap();

        assert!(sibling.has_primary_focus());
        assert_eq!(
            edges.borrow().as_slice(),
            &[(Some(old.id()), None), (None, Some(sibling.id()))],
            "the genuine outer edge (old released) publishes before the queued \
             reentrant request (sibling gained), never interleaved or reordered"
        );
    }

    #[test]
    fn replacing_a_scope_preserves_children_and_rebuilds_focus_history() {
        let manager = FocusManager::new();
        let old_scope = FocusScopeNode::with_debug_label("old-scope");
        let old_attachment = manager
            .root_scope()
            .attach_node(old_scope.as_focus_node())
            .unwrap();
        let child = FocusNode::with_debug_label("child");
        let child_attachment = old_scope.attach_node(&child).unwrap();
        child.request_focus();
        let replacement_scope = FocusScopeNode::with_debug_label("replacement-scope");

        let replacement_attachment = old_attachment
            .replace_node(replacement_scope.as_focus_node())
            .unwrap();

        assert!(replacement_attachment.is_attached());
        assert!(child_attachment.is_attached());
        assert_eq!(
            child.parent().map(|parent| parent.id()),
            Some(replacement_scope.id())
        );
        assert!(child.has_primary_focus());
        assert!(Rc::ptr_eq(
            replacement_scope.focused_child().as_ref().unwrap(),
            &child
        ));
        assert!(old_scope.focused_child().is_none());
        assert!(Rc::ptr_eq(
            manager.root_scope().focused_child().as_ref().unwrap(),
            replacement_scope.as_focus_node()
        ));
    }

    #[test]
    fn replacement_preconditions_fail_without_mutating_either_tree() {
        let manager = FocusManager::new();
        let old = FocusNode::with_debug_label("old");
        let old_attachment = manager.root_scope().attach_node(&old).unwrap();

        let attached = FocusNode::with_debug_label("attached");
        manager.root_scope().attach_node(&attached).unwrap();
        assert!(matches!(
            old_attachment.replace_node(&attached),
            Err(FocusTreeError::ReplacementAttached { replacement })
                if replacement == attached.id()
        ));

        let nonempty = FocusNode::with_debug_label("nonempty");
        let offline_child = FocusNode::with_debug_label("offline-child");
        nonempty.attach_node(&offline_child).unwrap();
        assert!(matches!(
            old_attachment.replace_node(&nonempty),
            Err(FocusTreeError::ReplacementNotEmpty { replacement })
                if replacement == nonempty.id()
        ));

        let scope = FocusScopeNode::with_debug_label("scope");
        assert!(matches!(
            old_attachment.replace_node(scope.as_focus_node()),
            Err(FocusTreeError::ReplacementKindMismatch {
                current,
                replacement,
            }) if current == old.id() && replacement == scope.id()
        ));

        assert!(old_attachment.is_attached());
        assert_eq!(
            old.parent().map(|parent| parent.id()),
            Some(manager.root_scope().id())
        );
        assert!(attached.is_attached());
        assert_eq!(
            nonempty.children().first().map(|node| node.id()),
            Some(offline_child.id())
        );
        assert!(!scope.as_focus_node().is_attached());

        let current_attachment = manager.root_scope().adopt_node(&old).unwrap();
        let untouched = FocusNode::with_debug_label("untouched");
        assert!(matches!(
            old_attachment.replace_node(&untouched),
            Err(FocusTreeError::StaleAttachment { node }) if node == old.id()
        ));
        assert!(current_attachment.is_attached());
        assert!(!untouched.is_attached());
    }

    #[test]
    fn replacement_honors_queued_node_and_scope_focus_intents() {
        let manager = FocusManager::new();
        let first_old = FocusNode::with_debug_label("first-old");
        let first_attachment = manager.root_scope().attach_node(&first_old).unwrap();
        let queued_node = FocusNode::with_debug_label("queued-node");
        assert_eq!(queued_node.request_focus(), FocusRequestOutcome::Queued);

        first_attachment.replace_node(&queued_node).unwrap();
        assert!(queued_node.has_primary_focus());

        let old_scope = FocusScopeNode::with_debug_label("old-scope");
        let old_scope_attachment = manager
            .root_scope()
            .attach_node(old_scope.as_focus_node())
            .unwrap();
        let pending_scope = FocusScopeNode::with_debug_label("pending-scope");
        assert!(pending_scope.set_first_focus());

        old_scope_attachment
            .replace_node(pending_scope.as_focus_node())
            .unwrap();
        assert!(pending_scope.as_focus_node().has_primary_focus());

        let first_descendant = FocusNode::with_debug_label("first-descendant");
        pending_scope.attach_node(&first_descendant).unwrap();
        assert!(
            first_descendant.has_primary_focus(),
            "the replacement scope keeps its pending first-focus intent"
        );
    }

    #[test]
    fn binding_an_offline_subtree_keeps_unchanged_child_attachment_live() {
        let manager = FocusManager::new();
        let parent = FocusNode::new();
        let child = FocusNode::new();
        let child_attachment = parent.attach_node(&child).unwrap();

        assert!(!child_attachment.is_attached());
        assert_eq!(child.request_focus(), FocusRequestOutcome::Queued);
        manager.root_scope().attach_node(&parent).unwrap();

        assert!(child_attachment.is_attached());
        assert!(child.has_primary_focus());
        assert_eq!(child_attachment.detach(), FocusDetachOutcome::Detached);
    }

    #[test]
    fn focus_history_records_the_child_for_every_ancestor_scope() {
        let manager = FocusManager::new();
        let inner = FocusScopeNode::new();
        manager
            .root_scope()
            .attach_node(inner.as_focus_node())
            .unwrap();
        let leaf = FocusNode::new();
        inner.attach_node(&leaf).unwrap();

        leaf.request_focus();

        assert!(Rc::ptr_eq(inner.focused_child().as_ref().unwrap(), &leaf));
        assert!(Rc::ptr_eq(
            manager.root_scope().focused_child().as_ref().unwrap(),
            inner.as_focus_node()
        ));
    }

    #[test]
    fn common_focus_ancestors_are_not_notified_for_a_sibling_move() {
        let manager = FocusManager::new();
        let parent = FocusNode::new();
        let first = FocusNode::new();
        let second = FocusNode::new();
        manager.root_scope().attach_node(&parent).unwrap();
        parent.attach_node(&first).unwrap();
        parent.attach_node(&second).unwrap();
        first.request_focus();

        let notifications = Rc::new(Cell::new(0));
        let notifications_for_listener = Rc::clone(&notifications);
        parent.add_listener(Rc::new(move || {
            notifications_for_listener.set(notifications_for_listener.get() + 1);
        }));

        second.request_focus();
        assert_eq!(notifications.get(), 0);
    }

    #[test]
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

    #[test]
    fn key_and_geometry_callbacks_may_replace_themselves() {
        let manager = FocusManager::new();
        let node = FocusNode::new();
        manager.root_scope().attach_node(&node).unwrap();

        let weak_node = Rc::downgrade(&node);
        node.set_on_key_event(Rc::new(move |_| {
            weak_node.upgrade().unwrap().clear_on_key_event();
            KeyEventResult::Ignored
        }));
        let weak_node = Rc::downgrade(&node);
        node.set_rect_provider(Rc::new(move || {
            weak_node.upgrade().unwrap().clear_rect_provider();
            Some(Rect::from_xywh(
                Pixels(1.0),
                Pixels(2.0),
                Pixels(3.0),
                Pixels(4.0),
            ))
        }));
        node.request_focus();

        assert!(!manager.dispatch_key_event(&key_event()));
        assert_eq!(
            node.rect(),
            Rect::from_xywh(Pixels(1.0), Pixels(2.0), Pixels(3.0), Pixels(4.0))
        );
    }

    #[test]
    fn property_registrations_clear_only_the_generation_they_installed() {
        let node = FocusNode::new();
        node.set_rect(Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(1.0),
            Pixels(1.0),
        ));

        let key_registration = node.register_on_key_event(Rc::new(|_| KeyEventResult::Handled));
        let rect_registration = node.register_rect_provider(Rc::new(|| {
            Some(Rect::from_xywh(
                Pixels(1.0),
                Pixels(2.0),
                Pixels(3.0),
                Pixels(4.0),
            ))
        }));
        assert!(key_registration.is_current());
        assert!(rect_registration.is_current());

        node.set_on_key_event(Rc::new(|_| KeyEventResult::SkipRemainingHandlers));
        node.set_rect_provider(Rc::new(|| {
            Some(Rect::from_xywh(
                Pixels(5.0),
                Pixels(6.0),
                Pixels(7.0),
                Pixels(8.0),
            ))
        }));
        assert!(!key_registration.is_current());
        assert!(!rect_registration.is_current());

        drop(key_registration);
        drop(rect_registration);
        assert_eq!(
            node.handle_key_event(&key_event()),
            KeyEventResult::SkipRemainingHandlers,
            "a stale key registration cannot erase a later writer"
        );
        assert_eq!(
            node.rect(),
            Rect::from_xywh(Pixels(5.0), Pixels(6.0), Pixels(7.0), Pixels(8.0)),
            "a stale geometry registration cannot erase a later writer"
        );
    }

    #[test]
    fn current_property_registrations_clean_up_or_can_relinquish_ownership() {
        let node = FocusNode::new();
        node.set_rect(Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(1.0),
            Pixels(1.0),
        ));

        let key_registration = node.register_on_key_event(Rc::new(|_| KeyEventResult::Handled));
        let rect_registration = node.register_rect_provider(Rc::new(|| {
            Some(Rect::from_xywh(
                Pixels(1.0),
                Pixels(2.0),
                Pixels(3.0),
                Pixels(4.0),
            ))
        }));
        drop(rect_registration);
        assert_eq!(
            node.rect(),
            Rect::from_xywh(Pixels(0.0), Pixels(0.0), Pixels(1.0), Pixels(1.0)),
            "dropping a current registration removes its provider"
        );

        key_registration.relinquish();
        assert_eq!(
            node.handle_key_event(&key_event()),
            KeyEventResult::Handled,
            "relinquishing transfers the installed handler to the node owner"
        );

        let replacement = node.register_on_key_event(Rc::new(|_| KeyEventResult::Handled));
        drop(replacement);
        assert_eq!(
            node.handle_key_event(&key_event()),
            KeyEventResult::Ignored,
            "dropping a current registration removes its handler"
        );
    }

    #[test]
    fn global_key_handlers_precede_the_focus_tree() {
        let (manager, nodes) = manager_with_nodes(1);
        let node_called = Rc::new(Cell::new(false));
        let node_called_by_handler = Rc::clone(&node_called);
        nodes[0].set_on_key_event(Rc::new(move |_| {
            node_called_by_handler.set(true);
            KeyEventResult::Handled
        }));
        nodes[0].request_focus();
        manager.add_global_key_handler(Rc::new(|_| true));

        assert!(manager.dispatch_key_event(&key_event()));
        assert!(!node_called.get());
    }

    #[test]
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

    #[test]
    fn traversal_derives_the_scope_from_primary_focus() {
        let manager = FocusManager::new();
        let inner = FocusScopeNode::with_debug_label("inner");
        manager
            .root_scope()
            .attach_node(inner.as_focus_node())
            .unwrap();
        inner.set_traversal_edge_behavior(TraversalEdgeBehavior::Stop);

        let inside = FocusNode::with_debug_label("inside");
        inside.set_rect(Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(10.0),
            Pixels(10.0),
        ));
        inner.attach_node(&inside).unwrap();

        let outside = FocusNode::with_debug_label("outside");
        outside.set_rect(Rect::from_xywh(
            Pixels(20.0),
            Pixels(0.0),
            Pixels(10.0),
            Pixels(10.0),
        ));
        manager.root_scope().attach_node(&outside).unwrap();

        inside.request_focus();

        assert!(
            !manager.focus_next(),
            "a Stop edge on the focused node's enclosing scope must win"
        );
        assert!(inside.has_primary_focus());
        assert!(!outside.has_primary_focus());
    }

    #[test]
    fn parent_scope_edge_retries_from_the_primary_nodes_scope() {
        let manager = FocusManager::new();
        let inner = FocusScopeNode::with_debug_label("inner");
        manager
            .root_scope()
            .attach_node(inner.as_focus_node())
            .unwrap();
        inner.set_traversal_edge_behavior(TraversalEdgeBehavior::ParentScope);

        let inside = FocusNode::with_debug_label("inside");
        inside.set_rect(Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(10.0),
            Pixels(10.0),
        ));
        inner.attach_node(&inside).unwrap();

        let outside = FocusNode::with_debug_label("outside");
        outside.set_rect(Rect::from_xywh(
            Pixels(20.0),
            Pixels(0.0),
            Pixels(10.0),
            Pixels(10.0),
        ));
        manager.root_scope().attach_node(&outside).unwrap();

        inside.request_focus();

        assert!(manager.focus_next());
        assert!(outside.has_primary_focus());
    }

    #[test]
    fn empty_scope_first_focus_waits_for_the_first_eligible_descendant_once() {
        let manager = FocusManager::new();
        let scope = FocusScopeNode::with_debug_label("route");

        assert!(
            scope.set_first_focus(),
            "the detached scope accepts the first-focus intent"
        );
        manager
            .root_scope()
            .attach_node(scope.as_focus_node())
            .unwrap();
        assert!(
            scope.as_focus_node().has_primary_focus(),
            "the scope parks focus until an eligible descendant exists"
        );

        let first = FocusNode::with_debug_label("first");
        first.set_can_request_focus(false);
        scope.attach_node(&first).unwrap();
        assert!(scope.as_focus_node().has_primary_focus());

        first.set_can_request_focus(true);
        assert!(
            first.has_primary_focus(),
            "becoming eligible fulfills the pending first-focus intent"
        );

        let second = FocusNode::with_debug_label("second");
        scope.attach_node(&second).unwrap();
        assert!(
            first.has_primary_focus(),
            "the fulfilled intent is one-shot"
        );
    }

    #[test]
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

    #[test]
    fn close_is_idempotent_and_tombstones_owned_nodes() {
        let (manager, nodes) = manager_with_nodes(1);
        nodes[0].request_focus();
        let attachment = manager.root_scope().adopt_node(&nodes[0]).unwrap();

        manager.close();
        manager.close();

        assert!(manager.is_closed());
        assert!(manager.primary_focus().is_none());
        assert!(!nodes[0].is_attached());
        assert_eq!(nodes[0].request_focus(), FocusRequestOutcome::OwnerClosed);
        assert_eq!(attachment.detach(), FocusDetachOutcome::OwnerClosed);

        let other = FocusManager::new();
        assert!(matches!(
            other.root_scope().attach_node(&nodes[0]),
            Err(FocusTreeError::OwnerClosed { .. })
        ));
    }

    #[test]
    fn focus_loss_callback_cannot_detach_a_node_out_of_closing_owner() {
        let manager = FocusManager::new();
        let node = FocusNode::new();
        let attachment = Rc::new(manager.root_scope().attach_node(&node).unwrap());
        node.request_focus();
        let callback_outcome = Rc::new(Cell::new(None));
        let callback_outcome_for_listener = Rc::clone(&callback_outcome);
        let attachment_for_listener = Rc::clone(&attachment);
        node.add_listener(Rc::new(move || {
            callback_outcome_for_listener.set(Some(attachment_for_listener.detach()));
        }));

        manager.close();

        assert_eq!(
            callback_outcome.get(),
            Some(FocusDetachOutcome::OwnerClosed)
        );
        assert_eq!(node.request_focus(), FocusRequestOutcome::OwnerClosed);
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

    /// The issue #1040 reproducer: A is focused, B is requested, and B's
    /// own node listener reentrantly requests C while B is (momentarily)
    /// primary. The reentrant request must be applied only after the
    /// outer A -> B notification finishes, and published in the order
    /// requested — never the reversed `[(B, C), (A, B)]` the pre-fix code
    /// produced.
    #[test]
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

    /// A reentrant `unfocus()` from inside a node listener is queued the
    /// same way a reentrant `request_focus` is, and publishes its own
    /// `(previous, None)` edge after the outer one.
    #[test]
    fn reentrant_unfocus_during_notification_is_applied_after_the_outer_edge() {
        let (manager, nodes) = manager_with_nodes(2);
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
        nodes[1].add_listener(Rc::new(move || {
            manager_for_listener.unfocus();
        }));

        nodes[1].request_focus();

        assert!(manager.primary_focus().is_none());
        assert_eq!(
            edges.borrow().as_slice(),
            &[
                (Some(nodes[0].id()), Some(nodes[1].id())),
                (Some(nodes[1].id()), None),
            ]
        );
    }

    /// Two reentrant requests issued from the same listener call (C then
    /// D) are applied and published FIFO, not last-wins and not reversed.
    #[test]
    fn two_reentrant_requests_are_applied_in_request_order() {
        let (manager, nodes) = manager_with_nodes(4);
        nodes[0].request_focus();

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));

        let third = Rc::clone(&nodes[2]);
        let fourth = Rc::clone(&nodes[3]);
        let intermediate = Rc::downgrade(&nodes[1]);
        nodes[1].add_listener(Rc::new(move || {
            if intermediate.upgrade().unwrap().has_primary_focus() {
                third.request_focus();
                fourth.request_focus();
            }
        }));

        nodes[1].request_focus();

        assert!(nodes[3].has_primary_focus());
        assert_eq!(
            edges.borrow().as_slice(),
            &[
                (Some(nodes[0].id()), Some(nodes[1].id())),
                (Some(nodes[1].id()), Some(nodes[2].id())),
                (Some(nodes[2].id()), Some(nodes[3].id())),
            ]
        );
    }

    /// Reentry from a *manager* listener (not a node listener): subscriber
    /// 1 requests C while receiving the A -> B edge. Manager listeners are
    /// dispatched from one snapshot of `(previous, new)` taken before the
    /// loop starts, so subscriber 2 must still receive that same A -> B
    /// edge — the reentrant C request is only queued, never rewinds what
    /// the rest of this dispatch delivers — and neither subscriber sees
    /// the queued B -> C edge until it is drained after this notification
    /// finishes.
    #[test]
    fn manager_listener_reentry_does_not_rewind_sibling_listeners_snapshot() {
        let (manager, nodes) = manager_with_nodes(3);
        nodes[0].request_focus();

        let edges = Rc::new(RefCell::new(Vec::new()));
        let node_b_id = nodes[1].id();
        let target = Rc::clone(&nodes[2]);
        let edges_for_first = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            let current_id = current.as_ref().map(|node| node.id());
            edges_for_first.borrow_mut().push((
                "first",
                previous.map(|node| node.id()),
                current_id,
            ));
            if current_id == Some(node_b_id) {
                target.request_focus();
            }
        }));
        let edges_for_second = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_second.borrow_mut().push((
                "second",
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));

        nodes[1].request_focus();

        assert!(nodes[2].has_primary_focus());
        assert_eq!(
            edges.borrow().as_slice(),
            &[
                ("first", Some(nodes[0].id()), Some(nodes[1].id())),
                ("second", Some(nodes[0].id()), Some(nodes[1].id())),
                ("first", Some(nodes[1].id()), Some(nodes[2].id())),
                ("second", Some(nodes[1].id()), Some(nodes[2].id())),
            ],
            "subscriber 2 must observe (A, B) before either subscriber observes the \
             reentrant (B, C) transition subscriber 1 queued"
        );
    }

    /// A -> B -> C -> B: focus returns to an earlier node's *identity*, but
    /// each hop is still a distinct, real transition and must publish as
    /// one — this is not the "already-current" no-op case
    /// (`focus_identity_eq` only short-circuits a request naming whatever
    /// is CURRENTLY primary, not one that merely matches something
    /// notified earlier in the same drain).
    #[test]
    fn reentrant_chain_returning_to_an_earlier_identity_publishes_every_hop() {
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

        // B redirects to C exactly once, the first time it becomes primary.
        let b_redirected = Cell::new(false);
        let c_for_b = Rc::clone(&nodes[2]);
        let b_weak = Rc::downgrade(&nodes[1]);
        nodes[1].add_listener(Rc::new(move || {
            let b = b_weak.upgrade().unwrap();
            if b.has_primary_focus() && !b_redirected.get() {
                b_redirected.set(true);
                c_for_b.request_focus();
            }
        }));
        // C redirects back to B exactly once, the first time it becomes primary.
        let c_redirected = Cell::new(false);
        let b_for_c = Rc::clone(&nodes[1]);
        let c_weak = Rc::downgrade(&nodes[2]);
        nodes[2].add_listener(Rc::new(move || {
            let c = c_weak.upgrade().unwrap();
            if c.has_primary_focus() && !c_redirected.get() {
                c_redirected.set(true);
                b_for_c.request_focus();
            }
        }));

        nodes[1].request_focus();

        assert!(nodes[1].has_primary_focus(), "the chain settles back on B");
        assert_eq!(
            edges.borrow().as_slice(),
            &[
                (Some(nodes[0].id()), Some(nodes[1].id())),
                (Some(nodes[1].id()), Some(nodes[2].id())),
                (Some(nodes[2].id()), Some(nodes[1].id())),
            ]
        );
    }

    /// A queued transition's eligibility is checked again immediately
    /// before it is applied, not only when it was accepted: B's listener
    /// requests C, then detaches C before the outer notification finishes.
    /// The drain must skip the now-detached target rather than committing
    /// it as primary.
    #[test]
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

    /// Companion to the detach case: B's listener requests C, then C
    /// becomes unfocusable before the outer notification finishes. The
    /// drain must skip C rather than committing an ineligible target.
    #[test]
    fn queued_focus_target_made_unfocusable_before_its_turn_is_skipped() {
        let manager = FocusManager::new();
        let a = FocusNode::with_debug_label("a");
        manager.root_scope().attach_node(&a).unwrap();
        let b = FocusNode::with_debug_label("b");
        manager.root_scope().attach_node(&b).unwrap();
        let c = FocusNode::with_debug_label("c");
        manager.root_scope().attach_node(&c).unwrap();
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
                c_for_listener.set_can_request_focus(false);
            }
        }));

        b.request_focus();

        assert!(
            b.has_primary_focus(),
            "a queued target that became unfocusable before its turn must not gain \
             primary focus"
        );
        assert!(!c.can_request_focus());
        assert_eq!(edges.borrow().as_slice(), &[(Some(a.id()), Some(b.id()))]);
    }

    /// A queued `None` (an [`Self::unfocus`] request) has no target to
    /// re-validate and is always eligible: it must apply even though the
    /// node that requested it may itself have detached in the meantime.
    #[test]
    fn queued_unfocus_is_always_eligible() {
        let manager = FocusManager::new();
        let a = FocusNode::with_debug_label("a");
        let a_attachment = manager.root_scope().attach_node(&a).unwrap();
        let b = FocusNode::with_debug_label("b");
        manager.root_scope().attach_node(&b).unwrap();
        a.request_focus();

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));

        let manager_for_listener = Rc::clone(&manager);
        let b_weak = Rc::downgrade(&b);
        b.add_listener(Rc::new(move || {
            if b_weak.upgrade().unwrap().has_primary_focus() {
                manager_for_listener.unfocus();
                a_attachment.detach();
            }
        }));

        b.request_focus();

        assert!(
            manager.primary_focus().is_none(),
            "a queued unfocus must apply even though the node that requested it detached"
        );
        assert_eq!(
            edges.borrow().as_slice(),
            &[(Some(a.id()), Some(b.id())), (Some(b.id()), None)]
        );
    }

    /// A listener that panics mid-notification must not leave
    /// `notification_depth` stuck above zero: every later, healthy
    /// `request_focus` on this manager would otherwise queue silently and
    /// never apply, since nothing would ever bring the depth back to zero
    /// to drain it.
    #[test]
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

    /// Companion to the test above: this one exercises the guard's OTHER
    /// unwind responsibility — discarding a *non-empty* queue, not just
    /// resetting the depth counter. B has two listeners: the first queues
    /// a reentrant request for C (accepted, `Focused`, but not yet
    /// applied); the second then panics. The panic must discard that
    /// queued request rather than leave it for a later, unrelated call to
    /// apply — proven through behavior (C never becomes primary, no
    /// `(_, C)` edge ever publishes), not by reaching into the private
    /// queue. It must also warn once, naming the drop, matching the
    /// budget-exceeded drop's own warning discipline.
    #[test]
    fn pending_requests_queued_before_a_listener_panic_are_discarded() {
        let (manager, nodes) = manager_with_nodes(4);
        nodes[0].request_focus();

        let edges = Rc::new(RefCell::new(Vec::new()));
        let edges_for_listener = Rc::clone(&edges);
        manager.add_listener(Rc::new(move |previous, current| {
            edges_for_listener.borrow_mut().push((
                previous.map(|node| node.id()),
                current.map(|node| node.id()),
            ));
        }));

        // Both listeners are guarded on B still being primary: on the
        // later, healthy transition below, B is the OUTGOING endpoint (no
        // longer primary), so neither fires again — the guard is what
        // keeps that second call from re-queuing C or re-panicking.
        let c = Rc::clone(&nodes[2]);
        let b_weak = Rc::downgrade(&nodes[1]);
        nodes[1].add_listener(Rc::new(move || {
            if b_weak.upgrade().unwrap().has_primary_focus() {
                assert_eq!(c.request_focus(), FocusRequestOutcome::Focused);
            }
        }));
        let b_weak = Rc::downgrade(&nodes[1]);
        nodes[1].add_listener(Rc::new(move || {
            assert!(
                !b_weak.upgrade().unwrap().has_primary_focus(),
                "boom: second listener under test panics"
            );
        }));

        let (panicked, log) = flui_testing::log_capture::capture(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                nodes[1].request_focus();
            }))
        });
        assert!(panicked.is_err(), "the listener panic must propagate");
        assert_eq!(
            log.count_containing("focus requests queued during a notification were discarded"),
            1,
            "the discard must warn exactly once, naming what it dropped: {log}"
        );

        nodes[3].request_focus();

        assert!(nodes[3].has_primary_focus());
        assert!(
            !nodes[2].has_primary_focus(),
            "the request the first listener queued before the second one panicked \
             must not have survived to apply later"
        );
        // The interrupted A -> B transition's own manager-level edge never
        // publishes either: `apply_focus_transition` runs node-level
        // notification before manager-level notification, and L2 panics
        // during the former, so `notify_listeners` for THIS transition
        // never runs at all. Only the later, healthy transition publishes.
        assert_eq!(
            edges.borrow().as_slice(),
            &[(Some(nodes[1].id()), Some(nodes[3].id()))],
            "no (_, C) edge may appear: the discarded request never applied"
        );
    }

    /// Two listeners that keep redirecting focus to each other cannot spin
    /// the caller forever: FLUI applies transitions synchronously (unlike
    /// Flutter's microtask-scheduled model, which merely yields a frame
    /// per bounce), so the drain is bounded at
    /// `FocusManager::REENTRANT_FOCUS_DRAIN_BUDGET` applications and warns
    /// once when it drops the rest.
    #[test]
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
    #[test]
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

    /// A node listener observes the manager's already-committed state, not
    /// a pending one: `has_primary_focus()` on the node just notified is
    /// true, a reentrant request is accepted (`FocusRequestOutcome::Focused`)
    /// but not yet applied, and only becomes visible on the target node
    /// once the outermost call returns.
    #[test]
    fn node_listeners_observe_committed_state_during_notification() {
        let (manager, nodes) = manager_with_nodes(3);
        nodes[0].request_focus();

        let observed = Rc::new(RefCell::new(None));
        let observed_for_listener = Rc::clone(&observed);
        let node_b = Rc::downgrade(&nodes[1]);
        let node_c = Rc::clone(&nodes[2]);
        nodes[1].add_listener(Rc::new(move || {
            let b = node_b.upgrade().unwrap();
            // `b` is also notified for the drained B -> C transition below
            // (it is the "previous" endpoint of that one too); only the
            // invocation where B is still the committed primary is the one
            // this test cares about.
            if !b.has_primary_focus() {
                return;
            }
            let outcome = node_c.request_focus();
            *observed_for_listener.borrow_mut() =
                Some((b.has_primary_focus(), outcome, node_c.has_primary_focus()));
        }));

        nodes[1].request_focus();

        let (b_had_focus_during, c_outcome, c_had_focus_during) =
            (*observed.borrow()).expect("the listener ran");
        assert!(
            b_had_focus_during,
            "the node listener observes the committed transition, not a pending one"
        );
        assert_eq!(c_outcome, FocusRequestOutcome::Focused);
        assert!(
            !c_had_focus_during,
            "a reentrant request is accepted but only queued, not yet applied, \
             during the outer notification"
        );
        assert!(
            nodes[2].has_primary_focus(),
            "the queued request is applied once the outer notification completes"
        );
        assert!(!manager.is_closed());
    }

    /// Matches Flutter's `_HighlightModeManager.notifyListeners`
    /// (`if (_listeners.contains(listener))`): a listener that removes
    /// another one (or itself) mid-dispatch must stop that listener from
    /// being called for the rest of this same dispatch, and must not panic
    /// on a re-borrow of the listener list.
    #[test]
    fn listener_removed_during_dispatch_is_not_called() {
        let (manager, nodes) = manager_with_nodes(2);
        nodes[0].request_focus();

        let second_calls = Rc::new(Cell::new(0));
        let second_calls_for_listener = Rc::clone(&second_calls);
        let second_id: Rc<RefCell<Option<ListenerId>>> = Rc::new(RefCell::new(None));
        let second_id_for_first_listener = Rc::clone(&second_id);
        let manager_for_first_listener = Rc::clone(&manager);
        manager.add_listener(Rc::new(move |_, _| {
            if let Some(id) = *second_id_for_first_listener.borrow() {
                manager_for_first_listener.remove_listener(id);
            }
        }));
        let second_id_value = manager.add_listener(Rc::new(move |_, _| {
            second_calls_for_listener.set(second_calls_for_listener.get() + 1);
        }));
        *second_id.borrow_mut() = Some(second_id_value);

        nodes[1].request_focus();

        assert_eq!(
            second_calls.get(),
            0,
            "a manager listener removed by an earlier one in the same dispatch \
             must not be called"
        );
        assert_eq!(manager.listener_count(), 1);
    }

    /// The node-level counterpart of
    /// `listener_removed_during_dispatch_is_not_called`: `FocusNode`'s
    /// listener dispatch follows the same contract.
    #[test]
    fn node_listener_removed_during_dispatch_is_not_called() {
        let manager = FocusManager::new();
        let node = FocusNode::new();
        manager.root_scope().attach_node(&node).unwrap();

        let second_calls = Rc::new(Cell::new(0));
        let second_calls_for_listener = Rc::clone(&second_calls);
        let second_id: Rc<RefCell<Option<ListenerId>>> = Rc::new(RefCell::new(None));
        let second_id_for_first_listener = Rc::clone(&second_id);
        let node_for_first_listener = Rc::clone(&node);
        node.add_listener(Rc::new(move || {
            if let Some(id) = *second_id_for_first_listener.borrow() {
                node_for_first_listener.remove_listener(id);
            }
        }));
        let second_id_value = node.add_listener(Rc::new(move || {
            second_calls_for_listener.set(second_calls_for_listener.get() + 1);
        }));
        *second_id.borrow_mut() = Some(second_id_value);

        node.request_focus();

        assert_eq!(
            second_calls.get(),
            0,
            "a node listener removed by an earlier one in the same dispatch must not be called"
        );
        assert_eq!(node.listener_count(), 1);
    }
}
