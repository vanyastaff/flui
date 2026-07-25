//! Owner-local keyboard focus manager.
//!
//! A [`FocusManager`] is explicitly owned by one presentation. It is neither
//! global nor thread-local. Nodes reach it only through the weak owner stored
//! when their subtree is attached below [`FocusManager::root_scope`].

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
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
pub struct FocusManager {
    root_scope: Rc<FocusScopeNode>,
    primary_focus: RefCell<Option<Rc<FocusNode>>>,
    listeners: RefCell<Vec<(ListenerId, FocusChangeCallback)>>,
    next_listener_id: Cell<usize>,
    global_key_handlers: RefCell<Vec<KeyEventCallback>>,
    closed: Cell<bool>,
}

impl FocusManager {
    /// Create an isolated focus owner and its attached root scope.
    #[must_use]
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|manager| Self {
            root_scope: FocusScopeNode::new_root(manager.clone()),
            primary_focus: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            next_listener_id: Cell::new(1),
            global_key_handlers: RefCell::new(Vec::new()),
            closed: Cell::new(false),
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

    pub(crate) fn request_focus(&self, node: &Rc<FocusNode>) -> bool {
        if self.closed.get() || !node.is_attached() || !node.can_request_focus() {
            return false;
        }
        let Some(owner) = node.manager() else {
            return false;
        };
        if !std::ptr::eq(owner.as_ref(), self) {
            tracing::warn!(
                node = node.id().get(),
                "focus request rejected because the node belongs to another manager"
            );
            return false;
        }
        self.set_primary_focus(Some(Rc::clone(node)));
        true
    }

    fn set_primary_focus(&self, node: Option<Rc<FocusNode>>) {
        let previous = {
            let mut primary = self.primary_focus.borrow_mut();
            let unchanged = match (&*primary, &node) {
                (Some(previous), Some(next)) => Rc::ptr_eq(previous, next),
                (None, None) => true,
                (Some(_), None) | (None, Some(_)) => false,
            };
            if unchanged {
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
        Self::notify_focus_nodes(previous.as_ref(), node.as_ref());
        self.notify_listeners(previous, node);
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
    pub(crate) fn finish_node_replacement(
        &self,
        previous_primary: Rc<FocusNode>,
        previous_focus_path: Vec<Rc<FocusNode>>,
    ) {
        let current = self.primary_focus();
        if let Some(primary) = &current {
            Self::refresh_focus_history(primary);
        }
        let current_focus_path = current.as_ref().map_or_else(Vec::new, |primary| {
            std::iter::once(Rc::clone(primary))
                .chain(primary.ancestors())
                .collect()
        });
        Self::notify_focus_path_change(previous_focus_path, current_focus_path);
        let latest = self.primary_focus();
        let focus_is_unchanged = match (&current, &latest) {
            (Some(current), Some(latest)) => Rc::ptr_eq(current, latest),
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        };
        if focus_is_unchanged {
            self.notify_listeners(Some(previous_primary), current);
        }
    }

    /// Release primary focus.
    pub fn unfocus(&self) {
        if !self.closed.get() {
            self.set_primary_focus(None);
        }
    }

    /// Register a focus or focused-ancestry change listener.
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
        self.listeners.borrow_mut().retain(|(held, _)| *held != id);
    }

    /// Remove all focus-change listeners.
    pub fn clear_listeners(&self) {
        self.listeners.borrow_mut().clear();
    }

    /// Number of registered listeners.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn listener_count(&self) -> usize {
        self.listeners.borrow().len()
    }

    fn notify_listeners(&self, previous: Option<Rc<FocusNode>>, new: Option<Rc<FocusNode>>) {
        let listeners = self.listeners.borrow().clone();
        for (_, listener) in listeners {
            listener(previous.clone(), new.clone());
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
        self.global_key_handlers.borrow_mut().clear();
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

        let Some(focused) = self.primary_focus() else {
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

    /// Deterministically retire this focus owner.
    ///
    /// Closing is idempotent. It sends the final focus-loss notification,
    /// clears manager and node callbacks, and tombstones every owned node.
    /// Tombstoned nodes cannot later attach to a different manager.
    pub fn close(&self) {
        if self.closed.replace(true) {
            return;
        }

        let previous = self.primary_focus.borrow_mut().take();
        if let Some(previous) = previous {
            Self::notify_focus_nodes(Some(&previous), None);
            self.notify_listeners(Some(previous), None);
        }
        self.listeners.borrow_mut().clear();
        self.global_key_handlers.borrow_mut().clear();
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
        assert_eq!(
            manager_edges.borrow().as_slice(),
            &[(Some(leaf.id()), Some(leaf.id()))],
            "focused ancestry changed while primary focus identity stayed stable"
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
}
