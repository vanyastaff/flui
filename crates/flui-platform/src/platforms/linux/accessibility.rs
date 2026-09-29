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

use std::sync::Arc;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use parking_lot::Mutex;

use crate::shared::accessibility_bridge::BridgeShared;
use crate::traits::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};

// The state shared between the capability and the three handlers the adapter
// owns lives in `crate::shared::accessibility_bridge`: the handlers are moved
// into `Adapter::new` and thereafter live on the adapter's own thread, so
// everything they touch has to be reachable from both sides — and the
// retention/dispatch rules are common to every OS adapter, so they are
// written and unit-tested once there rather than re-derived per OS.

struct Activation(Arc<BridgeShared>);

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        // Order matters: mark active and tell the composition root *before*
        // reading `latest`. The listener typically enables semantics, and on a
        // cold start that is what will produce the first tree at all — reading
        // first would answer `None` even when a synchronous listener could
        // have supplied one.
        self.0.notify_activation(true);
        self.0.retained()
    }
}

struct Deactivation(Arc<BridgeShared>);

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {
        self.0.notify_activation(false);
    }
}

struct Action(Arc<BridgeShared>);

impl ActionHandler for Action {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.notify_action(request);
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
}
