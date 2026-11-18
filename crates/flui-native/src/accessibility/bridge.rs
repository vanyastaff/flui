//! State shared between a platform accessibility adapter and its handlers.
//!
//! Every OS adapter (AT-SPI on Linux, UIA on Windows, NSAccessibility on
//! macOS) has the same shape: the platform library owns handler objects that
//! live on its own thread(s), while the [`PlatformAccessibility`] capability
//! is called from the composition root. What both sides share — the
//! active flag, the retained self-contained update, and the two listeners —
//! is this struct, extracted so the retention and dispatch rules are written
//! (and unit-tested, on Linux, where tests actually execute) exactly once
//! instead of re-derived per OS. The Web's semantics host shares it too.
//!
//! [`PlatformAccessibility`]: flui_semantics::platform::PlatformAccessibility

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(target_arch = "wasm32"))]
use accesskit::{ActionHandler, ActivationHandler};
use accesskit::{ActionRequest, TreeUpdate};
use parking_lot::Mutex;

#[cfg(not(target_arch = "wasm32"))]
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use flui_semantics::{
    platform::AccessibilityActionListener, platform::AccessibilityActivationListener,
};

// Each handler runs the composition root's listener inside the adapter's
// own OS entry (a UIA vtable method, an NSAccessibility or UIAccessibility
// method, an AT-SPI call on the adapter's thread): a panic in it is
// contained here and answers as if no listener ran.

/// AccessKit's activation handler over a bridge's shared state.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Activation(pub(crate) Arc<BridgeShared>);

#[cfg(not(target_arch = "wasm32"))]
impl ActivationHandler for Activation {
    /// Marks the bridge active, tells the composition root, then answers the
    /// retained tree. A tree the listener publishes while it is told is the
    /// answer: the adapter is in the middle of this call, so the publish is
    /// retained and not handed to it ([`BridgeShared::publish`]).
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        os_entry(
            "AccessKit's activation",
            (),
            AfterPanic::Answer(|()| None),
            || {
                // Order matters: mark active and tell the composition root
                // *before* reading the retained tree. The listener typically
                // enables semantics, and on a cold start that is what will
                // produce the first tree at all; reading first would answer
                // `None` even when a synchronous listener supplied one.
                let _activating = self.0.activating();
                self.0.notify_activation(true);
                self.0.retained()
            },
        )
    }
}

/// AccessKit's deactivation handler, for the adapters whose platform says
/// when assistive technology leaves (AT-SPI, UIAccessibility).
#[cfg(any(target_os = "ios", linux_host))]
pub(crate) struct Deactivation(pub(crate) Arc<BridgeShared>);

#[cfg(any(target_os = "ios", linux_host))]
impl accesskit::DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {
        os_entry(
            "AccessKit's deactivation",
            (),
            AfterPanic::Answer(|()| ()),
            || self.0.deactivate(),
        );
    }
}

/// AccessKit's action handler over a bridge's shared state.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Action(pub(crate) Arc<BridgeShared>);

#[cfg(not(target_arch = "wasm32"))]
impl ActionHandler for Action {
    fn do_action(&mut self, request: ActionRequest) {
        os_entry(
            "an AccessKit action",
            (),
            AfterPanic::Answer(|()| ()),
            || self.0.notify_action(request),
        );
    }
}

/// Puts the active flag back when the listener told of a change panics, so
/// the flag never says the composition root heard what it did not.
struct RestoreOnUnwind<'a> {
    flag: &'a AtomicBool,
    previous: bool,
}

impl Drop for RestoreOnUnwind<'_> {
    fn drop(&mut self) {
        self.flag.store(self.previous, Ordering::SeqCst);
    }
}

/// The adapter-side shared state: activation, retention, listeners.
#[derive(Default)]
pub(crate) struct BridgeShared {
    /// Whether assistive technology is currently attached.
    active: AtomicBool,
    /// The most recent **self-contained** update (one carrying
    /// [`TreeUpdate::tree`] metadata — the producer's promise that it stands
    /// alone), kept so a late activation can be answered immediately rather
    /// than showing an empty application until the next frame happens to
    /// dirty something.
    ///
    /// Incremental updates (`tree: None`) are deliberately not retained:
    /// applied in isolation they describe a handful of changed nodes, and
    /// answering a fresh screen reader with one would present a fragment as
    /// the whole interface. The composition root re-publishes a full tree on
    /// every activation, so this retained answer is at worst one full
    /// publish behind and is corrected within a frame.
    latest: Mutex<Option<TreeUpdate>>,
    /// The threads running the adapter's activation ([`Activation`]), each
    /// with how deep it is in it. The adapter holds its own state across that
    /// call (a `RefCell` borrow under UIA and NSAccessibility, a mutex under
    /// AT-SPI), so a publish the listener makes there must not reach it. Each
    /// thread counts only its own entry, so activations of one bridge on two
    /// threads, ending in either order, leave each other's marks alone.
    #[cfg(not(target_arch = "wasm32"))]
    activations: Mutex<smallvec::SmallVec<[(std::thread::ThreadId, u32); 1]>>,
    activation_listener: Mutex<Option<AccessibilityActivationListener>>,
    action_listener: Mutex<Option<AccessibilityActionListener>>,
}

/// Marks an activation running on the current thread until it drops, panic
/// included.
#[cfg(not(target_arch = "wasm32"))]
struct Activating<'a> {
    activations: &'a Mutex<smallvec::SmallVec<[(std::thread::ThreadId, u32); 1]>>,
    thread: std::thread::ThreadId,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Activating<'_> {
    fn drop(&mut self) {
        let mut activations = self.activations.lock();
        if let Some(index) = activations
            .iter()
            .position(|(thread, _)| *thread == self.thread)
        {
            let depth = &mut activations[index].1;
            *depth = depth.saturating_sub(1);
            if *depth == 0 {
                activations.swap_remove(index);
            }
        }
    }
}

impl BridgeShared {
    /// Fresh state: inactive, nothing retained, no listeners.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Mark the attachment state and tell the composition root.
    ///
    /// The flag is stored **before** the listener runs: the listener's
    /// natural response to "attached" is to enable semantics and publish,
    /// and a publish that consulted a stale `false` would be dropped by the
    /// adapter it was meant to initialize.
    ///
    /// The listener is cloned out of its lock, then called — a listener
    /// re-entering the adapter (publishing is the expected re-entry) would
    /// deadlock against a held guard. Same discipline as the semantics
    /// owner's callback dispatch.
    ///
    /// A listener that panics leaves the flag as it was before the call: the
    /// composition root did not take in the change, and an adapter that
    /// asks again finds the state that makes it tell the root again.
    pub(crate) fn notify_activation(&self, active: bool) {
        let previous = self.active.swap(active, Ordering::SeqCst);
        let listener = self.activation_listener.lock().clone();
        if let Some(listener) = listener {
            let restore = RestoreOnUnwind {
                flag: &self.active,
                previous,
            };
            listener(active);
            #[expect(
                clippy::mem_forget,
                reason = "disarms the restore guard once the listener returned"
            )]
            std::mem::forget(restore);
        }
    }

    /// The last assistive technology left: the retained tree is dropped, and
    /// the composition root, which stops building the tree, is told once.
    ///
    /// Both are committed before the listener runs ([`Self::notify_activation`]).
    /// A notice that finds the bridge inactive (a repeated one, or one after an
    /// activation whose listener panicked, which the root never heard) tells
    /// nobody: the root hears each change of state once.
    #[cfg(any(
        target_os = "ios",
        target_os = "android",
        linux_host,
        all(test, not(target_arch = "wasm32"))
    ))]
    pub(crate) fn deactivate(&self) {
        let retained = self.latest.lock().take();
        drop(retained);
        if self.is_active() {
            self.notify_activation(false);
        }
    }

    /// Forward an inbound action request (clone-and-release, as above).
    pub(crate) fn notify_action(&self, request: ActionRequest) {
        let listener = self.action_listener.lock().clone();
        if let Some(listener) = listener {
            listener(request);
        }
    }

    /// Mark the adapter's activation running on this thread until the guard
    /// drops.
    #[cfg(not(target_arch = "wasm32"))]
    fn activating(&self) -> Activating<'_> {
        let thread = std::thread::current().id();
        let mut activations = self.activations.lock();
        match activations
            .iter_mut()
            .find(|(running, _)| *running == thread)
        {
            Some((_, depth)) => *depth = depth.saturating_add(1),
            None => activations.push((thread, 1)),
        }
        Activating {
            activations: &self.activations,
            thread,
        }
    }

    /// Whether the adapter's activation runs on the calling thread.
    #[cfg(not(target_arch = "wasm32"))]
    fn activating_here(&self) -> bool {
        let activations = self.activations.lock();
        !activations.is_empty() && {
            let thread = std::thread::current().id();
            activations.iter().any(|(running, _)| *running == thread)
        }
    }

    /// What of a published `update` the adapter is handed. A self-contained
    /// update (one carrying tree metadata) is retained for late activations;
    /// a fragment never is, since it must not become the answer to a fresh
    /// screen reader (see the `latest` field doc). The update comes back for
    /// the adapter, except when it is published on the thread running the
    /// adapter's activation, from inside the listener: the adapter holds its
    /// state across that call and would re-enter it, so the update goes no
    /// further, and the activation answers it from the retained tree. A
    /// fragment published there is dropped; the composition root publishes a
    /// whole tree when told of an activation.
    #[must_use = "the update the adapter is to apply"]
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            clippy::unnecessary_wraps,
            reason = "the web host has no activation to retain an update for; the other hosts do"
        )
    )]
    pub(crate) fn publish(&self, update: TreeUpdate) -> Option<TreeUpdate> {
        if update.tree.is_some() {
            *self.latest.lock() = Some(update.clone());
        }
        // The web host has no adapter that calls into an activation.
        #[cfg(not(target_arch = "wasm32"))]
        if self.activating_here() {
            return None;
        }
        Some(update)
    }

    /// The retained self-contained update, if any — the immediate answer to
    /// a late activation.
    pub(crate) fn retained(&self) -> Option<TreeUpdate> {
        self.latest.lock().clone()
    }

    /// Whether assistive technology is currently attached.
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Register the attach/detach listener, replacing any previous one.
    pub(crate) fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        let _prev = self.activation_listener.lock().replace(listener);
    }

    /// Register the inbound-action listener, replacing any previous one.
    pub(crate) fn set_action_listener(&self, listener: AccessibilityActionListener) {
        let _prev = self.action_listener.lock().replace(listener);
    }
}

impl std::fmt::Debug for BridgeShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeShared")
            .field("active", &self.is_active())
            .field("has_retained", &self.latest.lock().is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::BridgeShared;

    /// The composition root never took in an activation whose listener
    /// panicked, so the bridge does not report itself active, and the next
    /// activation is told again.
    #[test]
    fn an_activation_whose_listener_panicked_leaves_the_bridge_inactive() {
        let shared = BridgeShared::new();
        shared.set_activation_listener(Arc::new(|_| panic!("the listener failed")));
        let unwound = catch_unwind(AssertUnwindSafe(|| shared.notify_activation(true)));
        assert!(
            unwound.is_err(),
            "the listener's panic unwinds to the entry"
        );
        assert!(
            !shared.is_active(),
            "the root never heard of the activation"
        );

        let heard = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&heard);
        shared.set_activation_listener(Arc::new(move |active| {
            if active {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        }));
        shared.notify_activation(true);
        assert!(shared.is_active());
        assert_eq!(
            heard.load(Ordering::SeqCst),
            1,
            "the next activation is told"
        );
    }
}

/// The activation as AccessKit's adapters run it, which the web host has none
/// of.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod activation_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use accesskit::{ActivationHandler, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
    use parking_lot::Mutex;

    use super::{Activation, BridgeShared};

    /// A whole tree of one button labelled `label`.
    fn tree(label: &str) -> TreeUpdate {
        let root = NodeId(1);
        let mut node = Node::new(Role::Button);
        node.set_label(label.to_string());
        TreeUpdate {
            nodes: vec![(root, node)],
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        }
    }

    /// The label of the root of `update`.
    fn label(update: &TreeUpdate) -> Option<&str> {
        update.nodes.first().and_then(|(_, node)| node.label())
    }

    /// The adapters hold their own state across the activation (a `RefCell`
    /// borrow under UIA and NSAccessibility, a mutex under AT-SPI), so a tree
    /// the listener publishes from inside it is the activation's answer and
    /// never reaches the adapter there; once the activation returned, a
    /// publish reaches the adapter again. A fragment published there answers
    /// nothing. Red-check: let `publish` hand the update on during the
    /// activation (the listener sees it come back, as the adapter would).
    #[test]
    fn a_tree_published_from_inside_the_activation_is_its_answer() {
        let shared = Arc::new(BridgeShared::new());
        let _ = shared.publish(tree("stale"));
        let passed_on = Arc::new(Mutex::new(Vec::new()));
        let inside = Arc::clone(&shared);
        let log = Arc::clone(&passed_on);
        shared.set_activation_listener(Arc::new(move |active| {
            if !active {
                return;
            }
            let mut fragment = tree("fragment");
            fragment.tree = None;
            for update in [tree("Submit"), fragment] {
                if let Some(update) = inside.publish(update) {
                    log.lock().push(update);
                }
            }
        }));

        let answer = Activation(Arc::clone(&shared))
            .request_initial_tree()
            .expect("the activation answers a tree");
        assert_eq!(label(&answer), Some("Submit"), "the tree published inside");
        assert!(
            passed_on.lock().is_empty(),
            "nothing published inside the activation reached the adapter"
        );
        assert!(shared.is_active());

        let after = shared
            .publish(tree("later"))
            .expect("a publish after the activation reaches the adapter");
        assert_eq!(label(&after), Some("later"));
    }

    /// The activation marks only its own thread: a publish another thread
    /// makes while it runs reaches the adapter, which serializes it itself.
    #[test]
    fn a_publish_on_another_thread_during_the_activation_reaches_the_adapter() {
        let shared = Arc::new(BridgeShared::new());
        let inside = Arc::clone(&shared);
        let passed_on = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&passed_on);
        shared.set_activation_listener(Arc::new(move |_| {
            let elsewhere = Arc::clone(&inside);
            let counted = Arc::clone(&counted);
            std::thread::spawn(move || {
                if elsewhere.publish(tree("elsewhere")).is_some() {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
            })
            .join()
            .expect("the publishing thread");
        }));
        let _ = Activation(Arc::clone(&shared)).request_initial_tree();
        assert_eq!(passed_on.load(Ordering::SeqCst), 1);
    }

    /// Two activations of one bridge on two threads overlap and the first
    /// ends first: the second's publish from inside its listener is still its
    /// answer, and once both ended no thread is left marked. Red-check: one
    /// slot for the activating thread, restored on drop, and the second's
    /// publish reaches the adapter while it is held.
    #[test]
    fn overlapping_activations_on_two_threads_keep_each_others_marks() {
        use std::sync::{Barrier, mpsc};

        let shared = Arc::new(BridgeShared::new());
        let both_inside = Arc::new(Barrier::new(2));
        let (first_ended, first_has_ended) = mpsc::channel::<()>();
        let first_has_ended = Mutex::new(first_has_ended);
        let reached_adapter = Arc::new(Mutex::new(Vec::new()));
        let (inside, log) = (Arc::clone(&shared), Arc::clone(&reached_adapter));
        shared.set_activation_listener(Arc::new(move |active| {
            if !active {
                return;
            }
            both_inside.wait();
            if std::thread::current().name() == Some("second") {
                first_has_ended
                    .lock()
                    .recv()
                    .expect("the first activation ends");
                log.lock().push(inside.publish(tree("second")).is_some());
            }
        }));
        let activate = |name: &str, ended: Option<mpsc::Sender<()>>| {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name(name.to_owned())
                .spawn(move || {
                    let answer = Activation(shared).request_initial_tree();
                    if let Some(ended) = ended {
                        let _ = ended.send(());
                    }
                    answer
                })
                .expect("an activating thread")
        };
        let first = activate("first", Some(first_ended));
        let second = activate("second", None);
        let _ = first.join().expect("the first activation");
        let answer = second.join().expect("the second activation");

        assert_eq!(
            *reached_adapter.lock(),
            [false],
            "the second activation's publish stayed out of its adapter"
        );
        assert_eq!(answer.as_ref().and_then(label), Some("second"));
        assert!(
            shared.activations.lock().is_empty(),
            "no thread left marked"
        );
    }
}
