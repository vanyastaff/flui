//! AT-SPI accessibility for Linux, via `accesskit_unix`.
//!
//! Implements [`PlatformAccessibility`] by wrapping [`accesskit_unix::Adapter`],
//! which exposes an AccessKit tree over AT-SPI (the D-Bus protocol Orca and
//! every other Linux screen reader speak).
//!
//! # Where it builds
//!
//! `accesskit_unix` brings ~66 crates (zbus, atspi, an async executor). AT-SPI
//! *is* D-Bus, so there is no lighter path, and every Linux host reads the
//! Settings portal and logind over D-Bus anyway. The adapter therefore builds
//! on Linux always and has no feature of its own (ADR-0151 §15).
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

use std::sync::Arc;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use parking_lot::Mutex;

use crate::accessibility::bridge::BridgeShared;
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use flui_semantics::{
    platform::AccessibilityActionListener, platform::AccessibilityActivationListener,
    platform::PlatformAccessibility,
};

// The state shared between the capability and the three handlers the adapter
// owns lives in `crate::accessibility::bridge`: the handlers are moved
// into `Adapter::new` and thereafter live on the adapter's own thread, so
// everything they touch has to be reachable from both sides — and the
// retention/dispatch rules are common to every OS adapter, so they are
// written and unit-tested once there rather than re-derived per OS.

struct Activation(Arc<BridgeShared>);

// Each handler runs the composition root's listener on the adapter's thread,
// inside AccessKit's own AT-SPI entry: a panic in it is contained there and
// answers as if no listener ran.

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        os_entry(
            "AccessKit's activation",
            (),
            AfterPanic::Answer(|()| None),
            || {
                // Order matters: mark active and tell the composition root *before*
                // reading `latest`. The listener typically enables semantics, and on a
                // cold start that is what will produce the first tree at all — reading
                // first would answer `None` even when a synchronous listener could
                // have supplied one.
                self.0.notify_activation(true);
                self.0.retained()
            },
        )
    }
}

struct Deactivation(Arc<BridgeShared>);

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {
        os_entry(
            "AccessKit's deactivation",
            (),
            AfterPanic::Answer(|()| ()),
            || {
                self.0.notify_activation(false);
            },
        );
    }
}

struct Action(Arc<BridgeShared>);

impl ActionHandler for Action {
    fn do_action(&mut self, request: ActionRequest) {
        os_entry(
            "an AccessKit action",
            (),
            AfterPanic::Answer(|()| ()),
            || {
                self.0.notify_action(request);
            },
        );
    }
}

/// AT-SPI accessibility for one window.
pub struct UnixAccessibility {
    shared: Arc<BridgeShared>,
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
        let shared = Arc::new(BridgeShared::new());
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

/// The adapter announces the window on the bus, not on the window: it leaves
/// the bus when its last handle drops, and nothing of the window needs undoing.
impl crate::host::WindowAccessibility for UnixAccessibility {
    fn detach(&self) {}

    /// Tell the bus whether the window is the one the window manager or
    /// compositor activated: a screen reader announces the window it moves
    /// to and reads the focus only in the active one. Before an assistive
    /// technology attaches, the adapter keeps it for its first tree.
    fn focus_changed(&self, focused: bool) {
        self.adapter.lock().update_window_focus_state(focused);
    }
}

/// Each window of a Linux host announces itself on the AT-SPI bus. The
/// adapter needs nothing of the window itself, so one implementation serves
/// the windows of every Linux host, whatever their reference.
impl<W: ?Sized> crate::host::Accessibility<W> for UnixAccessibility {
    fn for_window(_: &W) -> Arc<dyn crate::host::WindowAccessibility> {
        Arc::new(Self::new())
    }
}

impl std::fmt::Debug for UnixAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnixAccessibility")
            .field("active", &self.shared.is_active())
            .finish_non_exhaustive()
    }
}

impl PlatformAccessibility for UnixAccessibility {
    fn publish(&self, update: TreeUpdate) {
        // Self-contained updates are retained even while inactive: a screen
        // reader started later asks for an initial tree, and answering it
        // from here shows the real interface immediately instead of an empty
        // window until something next changes. Incremental updates are not
        // retained — see `BridgeShared` for why a fragment must never answer
        // an activation.
        //
        // Exactly one clone, and it buys that retention. `update_if_active`
        // takes a factory, so the owned value moves into the adapter only when
        // something is attached; with nobody listening the closure never runs
        // and the value is simply dropped.
        self.shared.retain_if_self_contained(&update);
        self.adapter.lock().update_if_active(move || update);
    }

    fn is_active(&self) -> bool {
        self.shared.is_active()
    }

    fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        self.shared.set_activation_listener(listener);
    }

    fn set_action_listener(&self, listener: AccessibilityActionListener) {
        self.shared.set_action_listener(listener);
    }
}

#[cfg(test)]
mod tests {
    use accesskit::{Node, NodeId, Role, TreeId, TreeInfo};

    use super::*;

    fn tree_update(label: &str) -> TreeUpdate {
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

    /// The activation handler answers with the retained tree, which is the
    /// path a screen reader started after the application takes.
    #[test]
    fn activation_answers_with_the_retained_tree() {
        let accessibility = UnixAccessibility::new();
        accessibility.publish(tree_update("Submit"));

        let mut activation = Activation(Arc::clone(&accessibility.shared));
        let initial = activation
            .request_initial_tree()
            .expect("a tree was published before activation");

        let (_, node) = initial.nodes.first().expect("one node");
        assert_eq!(node.label(), Some("Submit"));
        assert!(accessibility.is_active());
    }

    /// The listeners are the composition root's code, run inside AccessKit's
    /// AT-SPI entry: a panic in one is contained by the handler, which answers
    /// as if no listener ran, and the bridge stays usable. Red-check: call the
    /// listener outside `os_entry`.
    #[test]
    fn a_panicking_listener_stays_inside_its_handler() {
        let accessibility = UnixAccessibility::new();
        accessibility.publish(tree_update("Submit"));
        accessibility.set_activation_listener(Arc::new(|_| panic!("activation listener")));
        accessibility.set_action_listener(Arc::new(|_| panic!("action listener")));

        let contained = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let initial = Activation(Arc::clone(&accessibility.shared)).request_initial_tree();
            Action(Arc::clone(&accessibility.shared)).do_action(ActionRequest {
                action: accesskit::Action::Click,
                target_tree: TreeId::ROOT,
                target_node: NodeId(1),
                data: None,
            });
            Deactivation(Arc::clone(&accessibility.shared)).deactivate_accessibility();
            initial
        }));

        let initial = contained.expect("a listener's panic unwound out of its handler");
        assert!(initial.is_none(), "a failed activation answers no tree");
        assert!(
            !accessibility.is_active(),
            "the deactivation after the failures still reached the bridge"
        );
    }
}
