//! AT-SPI accessibility for Linux, via `accesskit_unix`.
//!
//! Implements [`PlatformAccessibility`] by wrapping [`accesskit_unix::Adapter`],
//! which exposes an AccessKit tree over AT-SPI (the D-Bus protocol Orca and
//! every other Linux screen reader speak).
//!
//! # Why this is feature-gated
//!
//! `accesskit_unix` brings ~66 crates (zbus, atspi, an async executor). AT-SPI
//! *is* D-Bus, so there is no lighter path — but an application that does not
//! want a screen-reader bridge should not link a D-Bus stack. The `a11y`
//! feature is therefore off by default.
//!
//! # Nothing async reaches the frame path
//!
//! `Adapter`'s own API is blocking; it runs an executor internally on its own
//! thread. No future is awaited on the UI thread and no runtime is imposed on
//! the embedder, which is what keeps this compatible with FLUI's synchronous
//! pipeline.
//!
//! # The activation dance
//!
//! AT-SPI is lazy: nothing exists until an assistive technology asks. That maps
//! onto FLUI's own gate rather than needing a new one — activation turns
//! semantics assembly on, deactivation turns it off, so an application with no
//! screen reader attached never pays for a tree walk.
//!
//! The first request arrives before FLUI has assembled anything, so
//! [`ActivationHandler::request_initial_tree`] answers `None` when it has no
//! tree yet. That is not a failure: AccessKit treats it as "not ready", and the
//! next [`publish`](PlatformAccessibility::publish) delivers the tree once
//! assembly has produced one.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use parking_lot::Mutex;

use crate::traits::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};

/// State shared between the capability and the three handlers the adapter owns.
///
/// The handlers are moved into `Adapter::new` and thereafter live on the
/// adapter's own thread, so everything they touch has to be reachable from both
/// sides. This is that shared middle.
#[derive(Default)]
struct Shared {
    /// Whether assistive technology is currently attached.
    active: AtomicBool,
    /// The most recent tree, kept so a late activation can be answered
    /// immediately rather than showing an empty application until the next
    /// frame happens to dirty something.
    latest: Mutex<Option<TreeUpdate>>,
    activation_listener: Mutex<Option<AccessibilityActivationListener>>,
    action_listener: Mutex<Option<AccessibilityActionListener>>,
}

impl Shared {
    /// Clone the listener out of its lock, then call it.
    ///
    /// A listener re-entering this type (the composition root's natural
    /// response to "attached" is to publish) would deadlock against a held
    /// guard. Same discipline as the semantics owner's callback dispatch.
    fn notify_activation(&self, active: bool) {
        self.active.store(active, Ordering::SeqCst);
        let listener = self.activation_listener.lock().clone();
        if let Some(listener) = listener {
            listener(active);
        }
    }

    fn notify_action(&self, request: ActionRequest) {
        let listener = self.action_listener.lock().clone();
        if let Some(listener) = listener {
            listener(request);
        }
    }
}

struct Activation(Arc<Shared>);

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        // Order matters: mark active and tell the composition root *before*
        // reading `latest`. The listener typically enables semantics, and on a
        // cold start that is what will produce the first tree at all — reading
        // first would answer `None` even when a synchronous listener could
        // have supplied one.
        self.0.notify_activation(true);
        self.0.latest.lock().clone()
    }
}

struct Deactivation(Arc<Shared>);

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {
        self.0.notify_activation(false);
    }
}

struct Action(Arc<Shared>);

impl ActionHandler for Action {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.notify_action(request);
    }
}

/// AT-SPI accessibility for one window.
pub struct UnixAccessibility {
    shared: Arc<Shared>,
    /// The adapter needs `&mut` to publish, and the capability is shared behind
    /// an `Arc`, so the mutability lives here rather than in the trait.
    adapter: Mutex<accesskit_unix::Adapter>,
}

impl UnixAccessibility {
    /// Create the adapter and announce this window on the AT-SPI bus.
    ///
    /// Constructing does not require a screen reader to be running, and does
    /// not fail when no session bus is reachable — the adapter simply never
    /// activates, which is exactly the state a headless CI machine is in.
    #[must_use]
    pub fn new() -> Self {
        let shared = Arc::new(Shared::default());
        let adapter = accesskit_unix::Adapter::new(
            Activation(Arc::clone(&shared)),
            Action(Arc::clone(&shared)),
            Deactivation(Arc::clone(&shared)),
        );

        Self {
            shared,
            adapter: Mutex::new(adapter),
        }
    }
}

impl Default for UnixAccessibility {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for UnixAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnixAccessibility")
            .field("active", &self.shared.active.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl PlatformAccessibility for UnixAccessibility {
    fn publish(&self, update: &TreeUpdate) {
        // Retained even while inactive: a screen reader started later asks for
        // an initial tree, and answering it from here shows the real interface
        // immediately instead of an empty window until something next changes.
        *self.shared.latest.lock() = Some(update.clone());

        // `update_if_active` takes a factory, so the second clone happens only
        // when assistive technology is actually attached. The common case —
        // nobody listening — costs the store above and nothing more.
        self.adapter.lock().update_if_active(|| update.clone());
    }

    fn is_active(&self) -> bool {
        self.shared.active.load(Ordering::SeqCst)
    }

    fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        *self.shared.activation_listener.lock() = Some(listener);
    }

    fn set_action_listener(&self, listener: AccessibilityActionListener) {
        *self.shared.action_listener.lock() = Some(listener);
    }
}

#[cfg(test)]
mod tests {
    use accesskit::{Node, NodeId, Role, Tree, TreeId};

    use super::*;

    fn tree_update(label: &str) -> TreeUpdate {
        let root = NodeId(1);
        let mut node = Node::new(Role::Button);
        node.set_label(label.to_string());
        TreeUpdate {
            nodes: vec![(root, node)],
            tree: Some(Tree::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        }
    }

    /// Constructing must not require a session bus. CI has none, and neither
    /// does a user running under a bare compositor — an application that
    /// panicked or hung there would be unusable for everyone, screen reader or
    /// not.
    #[test]
    fn constructing_without_a_session_bus_is_harmless() {
        let accessibility = UnixAccessibility::new();
        assert!(
            !accessibility.is_active(),
            "no assistive technology has attached"
        );
    }

    /// Publishing with nothing attached must be a no-op that still *retains*
    /// the tree, so a screen reader started afterwards is answered with the
    /// real interface rather than an empty one.
    #[test]
    fn publishing_while_inactive_retains_the_tree_for_a_later_activation() {
        let accessibility = UnixAccessibility::new();

        accessibility.publish(&tree_update("Submit"));

        let retained = accessibility.shared.latest.lock().clone();
        let retained = retained.expect("the tree is kept for a late activation");
        let (_, node) = retained.nodes.first().expect("one node");
        assert_eq!(node.label(), Some("Submit"));
    }

    /// The activation handler answers with the retained tree, which is the
    /// path a screen reader started after the application takes.
    #[test]
    fn activation_answers_with_the_retained_tree() {
        let accessibility = UnixAccessibility::new();
        accessibility.publish(&tree_update("Submit"));

        let mut activation = Activation(Arc::clone(&accessibility.shared));
        let initial = activation
            .request_initial_tree()
            .expect("a tree was published before activation");

        let (_, node) = initial.nodes.first().expect("one node");
        assert_eq!(node.label(), Some("Submit"));
        assert!(accessibility.is_active());
    }

    /// A cold activation has nothing to answer with. `None` means "not ready",
    /// not "no interface" — the next publish delivers the tree.
    #[test]
    fn a_cold_activation_answers_none_and_still_activates() {
        let accessibility = UnixAccessibility::new();

        let mut activation = Activation(Arc::clone(&accessibility.shared));
        assert!(activation.request_initial_tree().is_none());
        assert!(
            accessibility.is_active(),
            "activation is not conditional on having a tree"
        );
    }

    /// Attach and detach both reach the composition root's listener. A missed
    /// detach leaves semantics assembly running for a screen reader that is
    /// gone — a per-frame tree walk charged to nobody.
    #[test]
    fn activation_and_deactivation_reach_the_listener() {
        let accessibility = UnixAccessibility::new();
        let seen: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));

        let seen_in_listener = Arc::clone(&seen);
        accessibility.set_activation_listener(Arc::new(move |active| {
            seen_in_listener.lock().push(active);
        }));

        Activation(Arc::clone(&accessibility.shared)).request_initial_tree();
        Deactivation(Arc::clone(&accessibility.shared)).deactivate_accessibility();

        assert_eq!(*seen.lock(), vec![true, false]);
        assert!(!accessibility.is_active());
    }

    /// A listener that publishes from inside the activation callback must not
    /// deadlock — that is the composition root's natural response to "attached".
    #[test]
    fn a_listener_that_publishes_does_not_deadlock() {
        let accessibility = Arc::new(UnixAccessibility::new());
        let published = Arc::new(AtomicBool::new(false));

        let inner = Arc::clone(&accessibility);
        let published_flag = Arc::clone(&published);
        accessibility.set_activation_listener(Arc::new(move |active| {
            if active {
                inner.publish(&tree_update("Submit"));
                published_flag.store(true, Ordering::SeqCst);
            }
        }));

        let initial = Activation(Arc::clone(&accessibility.shared)).request_initial_tree();

        assert!(published.load(Ordering::SeqCst), "the listener completed");
        assert!(
            initial.is_some(),
            "and the tree it published answered this very activation"
        );
    }

    /// Inbound actions reach the composition root addressed by the stable node
    /// id, which is what closes the round trip without a translation table.
    #[test]
    fn an_action_reaches_the_listener_in_the_published_id_space() {
        let accessibility = UnixAccessibility::new();
        let seen: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));

        let seen_in_listener = Arc::clone(&seen);
        accessibility.set_action_listener(Arc::new(move |request: ActionRequest| {
            seen_in_listener.lock().push(request.target_node.0);
        }));

        Action(Arc::clone(&accessibility.shared)).do_action(ActionRequest {
            action: accesskit::Action::Click,
            target_tree: TreeId::ROOT,
            target_node: NodeId(1),
            data: None,
        });

        assert_eq!(*seen.lock(), vec![1]);
    }
}
