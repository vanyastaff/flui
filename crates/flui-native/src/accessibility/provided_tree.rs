//! The tree an Android view's node provider answers from: the AccessKit
//! adapter, the handlers it calls, and the end of the tree when the last
//! accessibility service stops. Pure Rust over [`ProvidedAdapter`], so its
//! rules run on every host; `accessibility::android` holds it over
//! `accesskit_android`'s adapter.
//!
//! # One thread
//!
//! Every call reaches it on the Java main thread: the node provider's
//! native methods, the updates and hovers the owner posts there, and the
//! accessibility manager's notice. The adapter is a [`RefCell`], and a call
//! that finds it borrowed is a call made from inside another one on the same
//! thread: it answers [`Busy`].
//!
//! # When the tree is built
//!
//! The tree starts on the first question a service asks through the node
//! provider (`createAccessibilityNodeInfo`, `findFocus`), which runs the
//! adapter's activation. It ends when Android reports that no accessibility
//! service is enabled ([`ProvidedTree::services_stopped`]): the adapter goes
//! back to its first state, which holds no tree and drops every update, the
//! bridge drops its retained tree, and the composition root is told, which
//! stops building the tree. The next question starts it again, and the root
//! builds the whole tree once for it.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use accesskit::TreeUpdate;

use crate::accessibility::bridge::{Action, Activation, BridgeShared};

/// What the tree needs of an AccessKit adapter: its first state, which holds
/// no tree, and an update applied only while a tree is held.
pub(crate) trait ProvidedAdapter: Default {
    /// The events an applied update raises, which the caller raises once the
    /// adapter is no longer borrowed.
    type Events;

    /// Apply `update` while the adapter holds a tree; drop it otherwise.
    fn update_if_active(&mut self, update: TreeUpdate) -> Option<Self::Events>;
}

/// The adapter with the handlers it calls.
pub(crate) struct Provided<A> {
    pub(crate) adapter: A,
    pub(crate) activation: Activation,
    pub(crate) action: Action,
}

/// A call made from inside another call into the adapter, on the same thread:
/// TalkBack answering an event by asking for node info while the adapter is
/// still borrowed, or a listener re-entering it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Busy;

/// The tree one view's node provider answers from (see the module doc).
pub(crate) struct ProvidedTree<A> {
    shared: Arc<BridgeShared>,
    provided: RefCell<Provided<A>>,
    /// Set by a stop that arrived while a call was inside the adapter: it
    /// takes effect as soon as that call returns.
    stop_pending: Cell<bool>,
}

impl<A: ProvidedAdapter> ProvidedTree<A> {
    /// A tree not started yet, over the bridge's `shared` state.
    pub(crate) fn new(shared: Arc<BridgeShared>) -> Self {
        Self {
            provided: RefCell::new(Provided {
                adapter: A::default(),
                activation: Activation(Arc::clone(&shared)),
                action: Action(Arc::clone(&shared)),
            }),
            shared,
            stop_pending: Cell::new(false),
        }
    }

    /// Run `f` with the adapter; a stop that arrived meanwhile then takes
    /// effect.
    ///
    /// # Errors
    ///
    /// [`Busy`] when a call on this thread is already inside the adapter.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut Provided<A>) -> R) -> Result<R, Busy> {
        let answer = {
            let mut provided = self.provided.try_borrow_mut().map_err(|_| Busy)?;
            f(&mut provided)
        };
        self.finish_pending_stop();
        Ok(answer)
    }

    /// Apply an update the owner published: dropped by an adapter that holds
    /// no tree, as after [`Self::services_stopped`].
    ///
    /// # Errors
    ///
    /// [`Busy`] when a call on this thread is inside the adapter. Posted work
    /// runs from the Java main thread's looper, never from inside a call into
    /// the adapter, so a posted update does not meet it.
    pub(crate) fn apply(&self, update: TreeUpdate) -> Result<Option<A::Events>, Busy> {
        self.with(|provided| provided.adapter.update_if_active(update))
    }

    /// No accessibility service is enabled any more: the tree ends (see the
    /// module doc). With a call inside the adapter, it ends as that call
    /// returns. A tree that never started, or already ended, tells nobody.
    pub(crate) fn services_stopped(&self) {
        self.stop_pending.set(true);
        self.finish_pending_stop();
    }

    /// End the tree a pending stop asked to end, unless a call is inside the
    /// adapter. The adapter's tree is dropped and the borrow released before
    /// the composition root is told: its listener is user code, which may
    /// call back in.
    fn finish_pending_stop(&self) {
        if !self.stop_pending.get() {
            return;
        }
        let Ok(mut provided) = self.provided.try_borrow_mut() else {
            return;
        };
        self.stop_pending.set(false);
        let ended = std::mem::take(&mut provided.adapter);
        drop(provided);
        drop(ended);
        self.shared.deactivate();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use accesskit::{ActivationHandler, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
    use parking_lot::Mutex;

    use super::{Busy, ProvidedAdapter, ProvidedTree};
    use crate::accessibility::bridge::BridgeShared;

    /// An adapter that holds the tree it was given, as `accesskit_android`'s
    /// does: none until the activation, and every update dropped without one.
    #[derive(Default)]
    struct HeldTree {
        tree: Option<TreeUpdate>,
        applied: usize,
    }

    impl ProvidedAdapter for HeldTree {
        type Events = ();

        fn update_if_active(&mut self, update: TreeUpdate) -> Option<()> {
            let tree = self.tree.as_mut()?;
            *tree = update;
            self.applied += 1;
            Some(())
        }
    }

    /// A whole tree of one button labelled `label`.
    fn whole(label: &str) -> TreeUpdate {
        let root = NodeId(1);
        let mut node = Node::new(Role::Button);
        node.set_label(label.to_owned());
        TreeUpdate {
            nodes: vec![(root, node)],
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        }
    }

    /// What the composition root was told, in order.
    type Heard = Arc<Mutex<Vec<bool>>>;

    /// A tree over a bridge whose composition root records what it is told.
    fn heard_tree() -> (Rc<ProvidedTree<HeldTree>>, Arc<BridgeShared>, Heard) {
        let shared = Arc::new(BridgeShared::new());
        let heard = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&heard);
        shared.set_activation_listener(Arc::new(move |active| log.lock().push(active)));
        (
            Rc::new(ProvidedTree::new(Arc::clone(&shared))),
            shared,
            heard,
        )
    }

    /// A service's first question, as `accesskit_android` runs it: the
    /// activation's answer, or a placeholder tree when it has none.
    fn ask(tree: &ProvidedTree<HeldTree>) {
        tree.with(|provided| {
            if provided.adapter.tree.is_none() {
                let answer = provided.activation.request_initial_tree();
                provided.adapter.tree = Some(answer.unwrap_or_else(|| whole("placeholder")));
            }
        })
        .expect("a question from the looper finds the adapter free");
    }

    /// Whether the adapter holds a tree.
    fn holds_tree(tree: &ProvidedTree<HeldTree>) -> bool {
        tree.with(|provided| provided.adapter.tree.is_some())
            .expect("the adapter is free")
    }

    /// The last service stops: the adapter's tree and the bridge's retained
    /// one are dropped, the root is told once, and nothing the owner posted
    /// before it heard is applied. A repeated notice tells nobody.
    /// Red-check: let `services_stopped` only tell the root (the adapter keeps
    /// its tree and applies the update posted after the stop).
    #[test]
    fn the_last_service_stopping_ends_the_tree_and_tells_the_root_once() {
        let (tree, shared, heard) = heard_tree();
        let _ = shared.publish(whole("Submit"));
        ask(&tree);
        assert!(shared.is_active() && holds_tree(&tree));
        assert!(matches!(tree.apply(whole("Count 1")), Ok(Some(()))));

        tree.services_stopped();
        assert!(!holds_tree(&tree), "the adapter's tree is dropped");
        assert!(shared.retained().is_none(), "the retained tree is dropped");
        assert!(!shared.is_active(), "a publish is posted no more");
        assert!(
            matches!(tree.apply(whole("Count 2")), Ok(None)),
            "an update posted before the root heard is not applied"
        );
        assert_eq!(
            tree.with(|provided| provided.adapter.applied),
            Ok(0),
            "the adapter after the stop applied nothing"
        );

        tree.services_stopped();
        assert_eq!(
            *heard.lock(),
            [true, false],
            "the root hears each change once"
        );
    }

    thread_local! {
        /// The tree the root's listener reads back while it is told.
        static TOLD_TREE: RefCell<Option<Rc<ProvidedTree<HeldTree>>>> = const { RefCell::new(None) };
    }

    /// The root is told with the tree already ended and the adapter free: a
    /// listener that reads the bridge or calls back into the adapter finds
    /// the state it is told of. Red-check: tell the root before the adapter's
    /// tree is taken, inside the borrow (the listener's call answers `Busy`).
    #[test]
    fn the_root_is_told_once_the_tree_ended_and_the_adapter_is_free() {
        let shared = Arc::new(BridgeShared::new());
        let tree = Rc::new(ProvidedTree::<HeldTree>::new(Arc::clone(&shared)));
        TOLD_TREE.with(|told| *told.borrow_mut() = Some(Rc::clone(&tree)));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (log, bridge) = (Arc::clone(&seen), Arc::clone(&shared));
        shared.set_activation_listener(Arc::new(move |active| {
            if active {
                return;
            }
            let reached = TOLD_TREE.with(|told| {
                told.borrow()
                    .as_ref()
                    .map(|tree| tree.with(|provided| provided.adapter.tree.is_some()))
            });
            log.lock()
                .push((reached, bridge.is_active(), bridge.retained().is_some()));
        }));
        let _ = shared.publish(whole("Submit"));
        ask(&tree);

        tree.services_stopped();
        TOLD_TREE.with(|told| told.borrow_mut().take());
        assert_eq!(*seen.lock(), [(Some(Ok(false)), false, false)]);
    }

    /// A stop that arrives while a call is inside the adapter waits for that
    /// call, and every call it nests, to return. Red-check: drop the pending
    /// stop when the adapter is borrowed (the tree outlives the notice).
    #[test]
    fn a_stop_during_a_call_into_the_adapter_ends_the_tree_as_the_call_returns() {
        let (tree, shared, heard) = heard_tree();
        ask(&tree);
        tree.with(|provided| {
            tree.services_stopped();
            assert!(provided.adapter.tree.is_some(), "not under the call");
            assert_eq!(*heard.lock(), [true], "the root is not told yet");
        })
        .expect("the adapter is free");
        assert!(!holds_tree(&tree));
        assert!(!shared.is_active());
        assert_eq!(*heard.lock(), [true, false]);
    }

    /// A call from inside another one answers `Busy` and leaves the outer
    /// call's adapter and handlers alone: an action the outer call performs
    /// after it still reaches the root.
    #[test]
    fn a_call_from_inside_the_adapter_answers_busy() {
        use accesskit::ActionHandler as _;

        let (tree, shared, _) = heard_tree();
        let actions = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&actions);
        shared.set_action_listener(Arc::new(move |request| log.lock().push(request.action)));
        let inner = tree
            .with(|provided| {
                let inner = tree.with(|_| ());
                provided.action.do_action(accesskit::ActionRequest {
                    action: accesskit::Action::Click,
                    target_tree: TreeId::ROOT,
                    target_node: NodeId(1),
                    data: None,
                });
                inner
            })
            .expect("the outer call reaches the adapter");
        assert_eq!(inner, Err(Busy));
        assert_eq!(*actions.lock(), [accesskit::Action::Click]);
        assert_eq!(tree.apply(whole("after")).map(|_| ()), Ok(()));
    }

    /// A service starting after the stop asks again: the root is told, the
    /// activation answers no stale tree, and the whole tree the root then
    /// builds once replaces the placeholder. Red-check: keep the retained
    /// tree at the stop (the activation answers `Submit`).
    #[test]
    fn a_service_starting_again_gets_the_whole_tree_built_for_it() {
        let (tree, shared, heard) = heard_tree();
        let _ = shared.publish(whole("Submit"));
        ask(&tree);
        tree.services_stopped();

        ask(&tree);
        assert_eq!(*heard.lock(), [true, false, true]);
        let answered = tree
            .with(|provided| {
                provided
                    .adapter
                    .tree
                    .as_ref()
                    .and_then(|tree| tree.nodes.first())
                    .and_then(|(_, node)| node.label().map(str::to_owned))
            })
            .expect("the adapter is free");
        assert_eq!(answered.as_deref(), Some("placeholder"));

        let update = shared
            .publish(whole("Count 0"))
            .expect("a publish outside the activation is handed on");
        assert!(matches!(tree.apply(update), Ok(Some(()))));
        assert_eq!(tree.with(|provided| provided.adapter.applied), Ok(1));
    }

    /// `FluiView` ends the tree on the manager's present answer, not on the
    /// notice's argument, so a stale notice cannot end a running service's
    /// tree; it registers the listener before it asks, so a change in between
    /// is heard, and removes it with the other observers.
    #[test]
    fn fluiview_ends_the_tree_when_the_manager_says_no_service_is_enabled() {
        let view = crate::owner::android::java_host::java_source("dev.flui.platform.FluiView");
        let at = |needle: &str| {
            view.find(needle)
                .unwrap_or_else(|| panic!("{needle:?} in FluiView"))
        };
        let asked = at("if (handle != 0 && accessibility != null && !accessibility.isEnabled()) {");
        let stopped = at("nativeAccessibilityStopped(handle);");
        assert!(asked < stopped);
        let registered = at("accessibility.addAccessibilityStateChangeListener(services);");
        let reported = at("        servicesChanged();\n    }");
        assert!(registered < reported, "registered, then asked once");
        let _ = at("accessibility.removeAccessibilityStateChangeListener(services);");
        assert!(
            !view.contains("onAccessibilityStateChanged(boolean enabled) {\n                nativeAccessibilityStopped"),
            "the notice's argument decides nothing"
        );
    }

    /// A root whose listener panics as it is told of the stop never heard
    /// it: the bridge stays active, and the next notice tells it again; the
    /// adapter's tree is gone either way. Red-check: tell only when the
    /// adapter held a tree (the second notice tells nobody).
    #[test]
    fn a_stop_the_root_failed_to_hear_is_told_again() {
        let (tree, shared, heard) = heard_tree();
        ask(&tree);
        let log = Arc::clone(&heard);
        shared.set_activation_listener(Arc::new(move |active| {
            log.lock().push(active);
            let first_stop = !active && log.lock().len() == 2;
            assert!(!first_stop, "the root failed to hear the stop");
        }));

        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.services_stopped();
        }));
        assert!(unwound.is_err());
        assert!(!holds_tree(&tree), "the adapter's tree is gone");
        assert!(shared.is_active(), "the root did not take the stop in");

        tree.services_stopped();
        assert!(!shared.is_active());
        assert_eq!(*heard.lock(), [true, false, false]);
    }
}
