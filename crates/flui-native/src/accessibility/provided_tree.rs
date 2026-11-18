//! The tree an Android view's node provider answers from: the AccessKit
//! adapter, the handlers it calls, and whether an accessibility service is
//! enabled to read it. Pure Rust over [`ProvidedAdapter`], so its rules run on
//! every host; `accessibility::android` holds it over `accesskit_android`'s
//! adapter.
//!
//! # One thread
//!
//! Every call reaches it on the Java main thread: the node provider's
//! native methods, the updates and hovers the owner posts there, and the
//! accessibility manager's report. The adapter is a [`RefCell`], and a call
//! that finds it borrowed is a call made from inside another one on the same
//! thread: it answers [`Busy`].
//!
//! # When the tree is built
//!
//! The tree is built only while Android reports an accessibility service
//! enabled ([`ProvidedTree::services_changed`], the only writer of that
//! fact). While one is, the first question that reaches the adapter (a node
//! provider call, a touch-exploration hover) runs its activation, which tells
//! the composition root and answers the tree it retained. When the report
//! turns to none, the adapter goes back to its first state, which holds no
//! tree and drops every update, the bridge drops its retained tree, and the
//! root is told, which stops building the tree; the next question after a
//! service is enabled again starts it, and the root builds the whole tree
//! once.
//!
//! A question while no service is enabled (Assist structure, the autofill
//! framework's compatibility mode, a hover posted before the report) is
//! answered from a placeholder tree, the composition root is not told, and
//! the placeholder is dropped before the call returns: nothing outside a
//! service's presence turns the root's semantics on.
//!
//! # What the adapter takes
//!
//! An adapter that answered its activation with no tree holds a placeholder,
//! which only a whole tree replaces: a fragment handed to it would be taken
//! for a whole tree. A fragment reaching it then is dropped, and the root is
//! asked for the whole tree again.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use accesskit::{ActivationHandler, TreeUpdate};

use crate::accessibility::bridge::{Action, Activation, BridgeShared};

/// What the tree needs of an AccessKit adapter: its first state, which holds
/// no tree, and an update applied only while a tree is held.
pub(crate) trait ProvidedAdapter: Default {
    /// The events an applied update raises, which the caller raises once the
    /// adapter is no longer borrowed.
    type Events;

    /// Apply `update` while the adapter holds a tree; drop it otherwise. An
    /// adapter holding a placeholder takes only a whole tree.
    fn update_if_active(&mut self, update: TreeUpdate) -> Option<Self::Events>;
}

/// What the adapter holds, as its activation left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    /// Nothing: the adapter's first state.
    Nothing,
    /// The placeholder the adapter shows when its activation answered no
    /// tree; only a whole tree replaces it.
    Placeholder,
    /// A tree that came from a whole tree.
    Tree,
}

/// What the tree and its activation handler share on the one thread.
struct Watch {
    /// Whether Android reports an accessibility service enabled.
    services_enabled: Cell<bool>,
    held: Cell<Held>,
}

/// The adapter's activation handler: the bridge's own while a service is
/// enabled, and otherwise one that answers no tree and tells nobody.
pub(crate) struct ServiceActivation {
    activation: Activation,
    watch: Rc<Watch>,
}

impl ActivationHandler for ServiceActivation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let answer = if self.watch.services_enabled.get() {
            self.activation.request_initial_tree()
        } else {
            None
        };
        self.watch.held.set(if answer.is_some() {
            Held::Tree
        } else {
            Held::Placeholder
        });
        answer
    }
}

/// The adapter with the handlers it calls.
pub(crate) struct Provided<A> {
    pub(crate) adapter: A,
    pub(crate) activation: ServiceActivation,
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
    watch: Rc<Watch>,
    /// Set by a report of no service that must tell the composition root;
    /// it takes effect once no call is inside the adapter.
    stop_pending: Cell<bool>,
}

impl<A: ProvidedAdapter> ProvidedTree<A> {
    /// A tree not started yet, over the bridge's `shared` state, with no
    /// service reported enabled.
    pub(crate) fn new(shared: Arc<BridgeShared>) -> Self {
        let watch = Rc::new(Watch {
            services_enabled: Cell::new(false),
            held: Cell::new(Held::Nothing),
        });
        Self {
            provided: RefCell::new(Provided {
                adapter: A::default(),
                activation: ServiceActivation {
                    activation: Activation(Arc::clone(&shared)),
                    watch: Rc::clone(&watch),
                },
                action: Action(Arc::clone(&shared)),
            }),
            shared,
            watch,
            stop_pending: Cell::new(false),
        }
    }

    /// Run `f` with the adapter. Then, with no call inside it, a tree no
    /// service may read ends, as a stop that arrived meanwhile does.
    ///
    /// # Errors
    ///
    /// [`Busy`] when a call on this thread is already inside the adapter.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut Provided<A>) -> R) -> Result<R, Busy> {
        let answer = {
            let mut provided = self.provided.try_borrow_mut().map_err(|_| Busy)?;
            f(&mut provided)
        };
        self.settle();
        Ok(answer)
    }

    /// Apply an update the owner published: dropped by an adapter that holds
    /// nothing, as after a stop; a fragment for a placeholder is dropped and
    /// the whole tree asked for again.
    ///
    /// # Errors
    ///
    /// [`Busy`] when a call on this thread is inside the adapter. Posted work
    /// runs from the Java main thread's looper, never from inside a call into
    /// the adapter, so a posted update does not meet it.
    pub(crate) fn apply(&self, update: TreeUpdate) -> Result<Option<A::Events>, Busy> {
        let mut fragment_refused = false;
        let events = self.with(|provided| match self.watch.held.get() {
            Held::Nothing => None,
            Held::Placeholder if update.tree.is_none() => {
                fragment_refused = true;
                None
            }
            Held::Placeholder | Held::Tree => {
                let events = provided.adapter.update_if_active(update);
                self.watch.held.set(Held::Tree);
                events
            }
        })?;
        if fragment_refused {
            self.shared.request_whole_tree();
        }
        Ok(events)
    }

    /// Android reports whether any accessibility service is enabled; the
    /// only writer of that fact. With none, the tree ends (see the module
    /// doc): the composition root is told when a service was reported before
    /// or the bridge reads attached, and a repeated report of none tells
    /// nobody. With a call inside the adapter, the tree ends as that call
    /// returns.
    pub(crate) fn services_changed(&self, enabled: bool) {
        let was_enabled = self.watch.services_enabled.replace(enabled);
        if !enabled && (was_enabled || self.shared.is_active()) {
            self.stop_pending.set(true);
        }
        self.settle();
    }

    /// End the tree when a stop is pending or no service may read what the
    /// adapter holds, unless a call is inside the adapter. The adapter's tree
    /// is dropped and the borrow released before the composition root is
    /// told: its listener is user code, which may call back in.
    fn settle(&self) {
        let tell = self.stop_pending.get();
        let unread = !self.watch.services_enabled.get() && self.watch.held.get() != Held::Nothing;
        if !tell && !unread {
            return;
        }
        let Ok(mut provided) = self.provided.try_borrow_mut() else {
            return;
        };
        self.stop_pending.set(false);
        self.watch.held.set(Held::Nothing);
        let ended = std::mem::take(&mut provided.adapter);
        drop(provided);
        drop(ended);
        if tell {
            self.shared.deactivate();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use accesskit::{ActivationHandler, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
    use parking_lot::Mutex;

    use super::{Busy, Provided, ProvidedAdapter, ProvidedTree};
    use crate::accessibility::bridge::BridgeShared;

    /// What `accesskit_android`'s adapter holds.
    #[derive(Debug, Default)]
    enum AdapterState {
        #[default]
        Inactive,
        Placeholder,
        Active(TreeUpdate),
    }

    /// An adapter that refuses what `accesskit_android`'s does: it holds
    /// nothing until its activation, drops every update then, and takes a
    /// fragment for its placeholder as the whole tree, which its tree
    /// refuses with a panic (`TreeUpdate::tree must be Some`).
    #[derive(Default)]
    struct HeldTree {
        state: AdapterState,
        applied: usize,
    }

    impl ProvidedAdapter for HeldTree {
        type Events = ();

        fn update_if_active(&mut self, update: TreeUpdate) -> Option<()> {
            match self.state {
                AdapterState::Inactive => return None,
                AdapterState::Placeholder => assert!(
                    update.tree.is_some(),
                    "TreeUpdate::tree must be Some when a placeholder is replaced"
                ),
                AdapterState::Active(_) => {}
            }
            self.state = AdapterState::Active(update);
            self.applied += 1;
            Some(())
        }
    }

    /// A question reaching the adapter (node info, a hover), as
    /// `accesskit_android` runs it: the activation when it holds nothing.
    fn question(provided: &mut Provided<HeldTree>) {
        if matches!(provided.adapter.state, AdapterState::Inactive) {
            provided.adapter.state = match provided.activation.request_initial_tree() {
                Some(tree) => AdapterState::Active(tree),
                None => AdapterState::Placeholder,
            };
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

    /// The fragment of `whole(label)`.
    fn fragment(label: &str) -> TreeUpdate {
        TreeUpdate {
            tree: None,
            ..whole(label)
        }
    }

    /// What the composition root was told, in order.
    type Heard = Arc<Mutex<Vec<bool>>>;

    /// A tree over a bridge whose composition root records what it is told,
    /// with a service reported enabled.
    fn heard_tree() -> (Rc<ProvidedTree<HeldTree>>, Arc<BridgeShared>, Heard) {
        let shared = Arc::new(BridgeShared::new());
        let heard = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&heard);
        shared.set_activation_listener(Arc::new(move |active| log.lock().push(active)));
        let tree = Rc::new(ProvidedTree::new(Arc::clone(&shared)));
        tree.services_changed(true);
        (tree, shared, heard)
    }

    /// A service's question from the looper.
    fn ask(tree: &ProvidedTree<HeldTree>) {
        tree.with(question)
            .expect("a question from the looper finds the adapter free");
    }

    /// Whether the adapter holds anything.
    fn holds_tree(tree: &ProvidedTree<HeldTree>) -> bool {
        tree.with(|provided| !matches!(provided.adapter.state, AdapterState::Inactive))
            .expect("the adapter is free")
    }

    /// The label the adapter's tree shows, if it holds one.
    fn shown(tree: &ProvidedTree<HeldTree>) -> Option<String> {
        tree.with(|provided| match &provided.adapter.state {
            AdapterState::Active(update) => update
                .nodes
                .first()
                .and_then(|(_, node)| node.label().map(str::to_owned)),
            AdapterState::Inactive | AdapterState::Placeholder => None,
        })
        .expect("the adapter is free")
    }

    /// The last service stops: the adapter's tree and the bridge's retained
    /// one are dropped, the root is told once, and nothing the owner posted
    /// before it heard is applied. A repeated report tells nobody.
    /// Red-check: let the stop only tell the root (the adapter keeps its
    /// tree and applies the update posted after the stop).
    #[test]
    fn the_last_service_stopping_ends_the_tree_and_tells_the_root_once() {
        let (tree, shared, heard) = heard_tree();
        let _ = shared.publish(whole("Submit"));
        ask(&tree);
        assert!(shared.is_active() && holds_tree(&tree));
        assert!(matches!(tree.apply(whole("Count 1")), Ok(Some(()))));

        tree.services_changed(false);
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

        tree.services_changed(false);
        assert_eq!(
            *heard.lock(),
            [true, false],
            "the root hears each change once"
        );
    }

    /// A touch-exploration hover the owner posted while the tree was live
    /// runs after the report of no service: it is answered, the root is not
    /// told, and the adapter holds nothing once it returns, so no frame
    /// builds semantics again. Red-check: let the activation tell the root
    /// whatever the services (the root hears `true` again and stays on).
    #[test]
    fn a_hover_posted_before_the_last_service_stopped_starts_nothing() {
        let (tree, shared, heard) = heard_tree();
        ask(&tree);
        tree.services_changed(false);

        let answered = tree
            .with(|provided| {
                question(provided);
                !matches!(provided.adapter.state, AdapterState::Inactive)
            })
            .expect("the posted hover finds the adapter free");
        assert!(answered, "the hover is answered from a placeholder");
        assert!(
            !holds_tree(&tree),
            "the placeholder is gone once it returned"
        );
        assert!(!shared.is_active());
        assert_eq!(*heard.lock(), [true, false]);
    }

    /// A question from no service at all (Assist structure, autofill
    /// compatibility) before any service was reported: answered, nothing
    /// told, nothing kept. Red-check: as above.
    #[test]
    fn a_question_with_no_service_enabled_tells_nobody_and_keeps_nothing() {
        let shared = Arc::new(BridgeShared::new());
        let heard: Heard = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&heard);
        shared.set_activation_listener(Arc::new(move |active| log.lock().push(active)));
        let _ = shared.publish(whole("Submit"));
        let tree = ProvidedTree::<HeldTree>::new(Arc::clone(&shared));

        ask(&tree);
        assert!(!holds_tree(&tree));
        assert!(!shared.is_active());
        assert!(heard.lock().is_empty());
    }

    /// The hover restarts the tree after the stop (order: no service, hover,
    /// fragment), and a fragment posted before reaches an adapter that holds
    /// nothing: dropped. With a service, an activation that had no tree to
    /// answer leaves a placeholder: a fragment is dropped and the whole tree
    /// asked again, and the whole tree then replaces the placeholder.
    /// Red-check: hand every update to the adapter (the placeholder takes the
    /// fragment and the adapter panics).
    #[test]
    fn a_fragment_never_reaches_a_placeholder() {
        let (tree, shared, heard) = heard_tree();
        ask(&tree);
        tree.services_changed(false);
        tree.with(question)
            .expect("the hover finds the adapter free");
        assert_eq!(tree.apply(fragment("Count 1")).map(|_| ()), Ok(()));
        assert!(!holds_tree(&tree));

        tree.services_changed(true);
        ask(&tree);
        assert!(holds_tree(&tree) && shown(&tree).is_none(), "a placeholder");
        assert!(matches!(tree.apply(fragment("Count 2")), Ok(None)));
        assert_eq!(
            *heard.lock(),
            [true, false, true, true],
            "the root is asked for the whole tree again"
        );
        assert!(matches!(tree.apply(whole("Count 3")), Ok(Some(()))));
        assert_eq!(shown(&tree).as_deref(), Some("Count 3"));
        assert!(matches!(tree.apply(fragment("Count 4")), Ok(Some(()))));
        assert!(shared.is_active());
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
        tree.services_changed(true);
        TOLD_TREE.with(|told| *told.borrow_mut() = Some(Rc::clone(&tree)));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (log, bridge) = (Arc::clone(&seen), Arc::clone(&shared));
        shared.set_activation_listener(Arc::new(move |active| {
            if active {
                return;
            }
            let reached = TOLD_TREE.with(|told| {
                told.borrow().as_ref().map(|tree| {
                    tree.with(|provided| !matches!(provided.adapter.state, AdapterState::Inactive))
                })
            });
            log.lock()
                .push((reached, bridge.is_active(), bridge.retained().is_some()));
        }));
        let _ = shared.publish(whole("Submit"));
        ask(&tree);

        tree.services_changed(false);
        TOLD_TREE.with(|told| told.borrow_mut().take());
        assert_eq!(*seen.lock(), [(Some(Ok(false)), false, false)]);
    }

    /// A report of no service that arrives while a call is inside the adapter
    /// waits for that call, and every call it nests, to return. Red-check:
    /// drop the pending stop when the adapter is borrowed (the tree outlives
    /// the report).
    #[test]
    fn a_stop_during_a_call_into_the_adapter_ends_the_tree_as_the_call_returns() {
        let (tree, shared, heard) = heard_tree();
        ask(&tree);
        tree.with(|provided| {
            tree.services_changed(false);
            assert!(
                !matches!(provided.adapter.state, AdapterState::Inactive),
                "not under the call"
            );
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
    }

    /// A service enabled again asks again: the root is told, the activation
    /// answers no stale tree, and the whole tree the root then builds once
    /// replaces the placeholder. Red-check: keep the retained tree at the
    /// stop (the activation answers `Submit`).
    #[test]
    fn a_service_starting_again_gets_the_whole_tree_built_for_it() {
        let (tree, shared, heard) = heard_tree();
        let _ = shared.publish(whole("Submit"));
        ask(&tree);
        tree.services_changed(false);

        tree.services_changed(true);
        ask(&tree);
        assert_eq!(*heard.lock(), [true, false, true]);
        assert!(holds_tree(&tree) && shown(&tree).is_none(), "a placeholder");

        let update = shared
            .publish(whole("Count 0"))
            .expect("a publish outside the activation is handed on");
        assert!(matches!(tree.apply(update), Ok(Some(()))));
        assert_eq!(tree.with(|provided| provided.adapter.applied), Ok(1));
    }

    /// A root whose listener committed the activation and then panicked
    /// (the bridge reads inactive) is still told when the last service stops,
    /// and a root that panicked on the stop is told again by the next report
    /// of none. Red-check: tell only an active bridge (neither is told).
    #[test]
    fn a_stop_reaches_a_root_that_failed_to_take_a_change_whole() {
        let (tree, shared, heard) = heard_tree();
        let log = Arc::clone(&heard);
        shared.set_activation_listener(Arc::new(move |active| {
            log.lock().push(active);
            let told = log.lock().len();
            assert!(told > 2, "the root's wake failed after it took the change");
        }));
        ask(&tree);
        assert!(
            !shared.is_active(),
            "the activation's panic left it inactive"
        );

        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.services_changed(false);
        }));
        assert!(unwound.is_err(), "the stop's listener panicked");
        assert!(!holds_tree(&tree), "the adapter's tree is gone");

        tree.services_changed(true);
        tree.services_changed(false);
        assert_eq!(*heard.lock(), [true, false, false]);
    }

    /// `FluiView` reports the manager's present answer, not the notice's
    /// argument, so a stale notice cannot end a running service's tree; it
    /// registers the listener before it asks, so a change in between is
    /// heard, reports again as the bridge arrives, and removes the listener
    /// with the other observers.
    #[test]
    fn fluiview_reports_what_the_manager_says_now() {
        let view = crate::owner::android::java_host::java_source("dev.flui.platform.FluiView");
        let at = |needle: &str| {
            view.find(needle)
                .unwrap_or_else(|| panic!("{needle:?} in FluiView"))
        };
        let _ = at(
            "nativeAccessibilityServices(handle, accessibility != null && accessibility.isEnabled());",
        );
        let registered = at("accessibility.addAccessibilityStateChangeListener(services);");
        let reported = at("        servicesChanged();\n    }");
        assert!(registered < reported, "registered, then asked once");
        let provided = at("setAccessibilityDelegate(new NodesDelegate());");
        let reported_to_the_bridge = at("            servicesChanged();\n        } else {");
        assert!(provided < reported_to_the_bridge);
        let _ = at("accessibility.removeAccessibilityStateChangeListener(services);");
        assert!(
            !view.contains("onAccessibilityStateChanged(boolean enabled) {\n                nativeAccessibilityServices"),
            "the notice's argument decides nothing"
        );
    }
}
