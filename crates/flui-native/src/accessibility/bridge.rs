//! State shared between a platform accessibility adapter and its handlers.
//!
//! Every OS adapter (AT-SPI on Linux, UIA on Windows, NSAccessibility on
//! macOS) has the same shape: the platform library owns handler objects that
//! live on its own thread(s), while the [`PlatformAccessibility`] capability
//! is called from the composition root. What both sides share — the
//! active flag, the retained self-contained update, and the two listeners —
//! is this struct, extracted so the retention and dispatch rules are written
//! (and unit-tested, on Linux, where tests actually execute) exactly once
//! instead of re-derived per OS.
//!
//! [`PlatformAccessibility`]: flui_semantics::platform::PlatformAccessibility

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, NodeId, TreeId, TreeUpdate};
use parking_lot::Mutex;

use crate::owner::panic_boundary::{AfterPanic, os_entry};
use flui_semantics::{
    platform::AccessibilityActionListener, platform::AccessibilityActivationListener,
    platform::AssistiveFocusListener,
};

// Each handler runs the composition root's listener inside the adapter's
// own OS entry (a UIA vtable method, an NSAccessibility or UIAccessibility
// method, an AT-SPI call on the adapter's thread): a panic in it is
// contained here and answers as if no listener ran.

/// AccessKit's activation handler over a bridge's shared state.
pub(crate) struct Activation(pub(crate) Arc<BridgeShared>);

impl ActivationHandler for Activation {
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
                // `None` even when a synchronous listener could have supplied
                // one.
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
            || self.0.notify_activation(false),
        );
    }
}

/// AccessKit's action handler over a bridge's shared state.
pub(crate) struct Action(pub(crate) Arc<BridgeShared>);

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
    activation_listener: Mutex<Option<AccessibilityActivationListener>>,
    action_listener: Mutex<Option<AccessibilityActionListener>>,
    assistive_focus_listener: Mutex<Option<AssistiveFocusListener>>,
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
            std::mem::forget(restore);
        }
    }

    /// Forward an inbound action request (clone-and-release, as above).
    pub(crate) fn notify_action(&self, request: ActionRequest) {
        let listener = self.action_listener.lock().clone();
        if let Some(listener) = listener {
            listener(request);
        }
    }

    /// Tell the composition root where the assistive technology's cursor
    /// moved (clone-and-release, as above). A node of a tree other than the
    /// one FLUI publishes, `TreeId::ROOT`, is off FLUI's tree.
    #[cfg_attr(
        not(any(target_os = "ios", test)),
        expect(dead_code, reason = "only iOS's adapter reports the cursor")
    )]
    pub(crate) fn notify_assistive_focus(&self, node: Option<(NodeId, TreeId)>) {
        let node = node.and_then(|(node, tree)| (tree == TreeId::ROOT).then_some(node));
        let listener = self.assistive_focus_listener.lock().clone();
        if let Some(listener) = listener {
            listener(node);
        }
    }

    /// Retain `update` for late activations **iff it is self-contained**
    /// (carries tree metadata). A fragment must never become the answer to
    /// a fresh screen reader — see the `latest` field doc.
    pub(crate) fn retain_if_self_contained(&self, update: &TreeUpdate) {
        if update.tree.is_some() {
            *self.latest.lock() = Some(update.clone());
        }
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

    /// Register the assistive-technology cursor listener, replacing any previous one.
    #[cfg_attr(
        not(any(target_os = "ios", test)),
        expect(dead_code, reason = "only iOS's adapter reports the cursor")
    )]
    pub(crate) fn set_assistive_focus_listener(&self, listener: AssistiveFocusListener) {
        let _prev = self.assistive_focus_listener.lock().replace(listener);
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

    /// The cursor reaches the composition root as a node of FLUI's tree; a
    /// node of another tree is off it.
    #[test]
    fn the_assistive_cursor_reaches_the_listener_on_flui_s_tree_only() {
        use accesskit::{NodeId, TreeId, Uuid};
        use parking_lot::Mutex;

        let shared = BridgeShared::new();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&heard);
        shared.set_assistive_focus_listener(Arc::new(move |node| record.lock().push(node)));

        let foreign = TreeId(Uuid::from_u128(7));
        shared.notify_assistive_focus(Some((NodeId(3), TreeId::ROOT)));
        shared.notify_assistive_focus(Some((NodeId(3), foreign)));
        shared.notify_assistive_focus(None);
        assert_eq!(*heard.lock(), [Some(NodeId(3)), None, None]);
    }
}
