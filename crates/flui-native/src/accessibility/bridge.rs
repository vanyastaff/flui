//! State shared between a platform accessibility adapter and its handlers.
//!
//! Every OS adapter (AT-SPI on Linux, UIA on Windows, NSAccessibility on
//! macOS) has the same shape: the platform library owns handler objects that
//! live on its own thread(s), while the [`PlatformAccessibility`] capability
//! is called from the composition root. What both sides share — the
//! active flag, the retained self-contained update, and the one inbound handler —
//! is this struct, extracted so the retention and dispatch rules are written
//! (and unit-tested, on Linux, where tests actually execute) exactly once
//! instead of re-derived per OS. The Web's semantics host shares it too.
//!
//! [`PlatformAccessibility`]: flui_semantics::platform::PlatformAccessibility

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(target_arch = "wasm32"))]
use accesskit::{ActionHandler, ActivationHandler};
use accesskit::{ActionRequest, NodeId, TreeId, TreeUpdate};
use parking_lot::Mutex;

#[cfg(not(target_arch = "wasm32"))]
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use flui_semantics::platform::{
    AccessibilityAlreadyAttached, AccessibilityAttachment, AccessibilityCapabilities,
    AccessibilityHandler,
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

/// The adapter-side shared state: activation, retention and one handler.
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
    handler: Mutex<Option<Arc<dyn AccessibilityHandler>>>,
}

impl BridgeShared {
    /// Fresh state: inactive, nothing retained, no handler.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Attach the composition root and snapshot the adapter in one lock hold.
    pub(crate) fn attach(
        &self,
        handler: Arc<dyn AccessibilityHandler>,
        capabilities: AccessibilityCapabilities,
    ) -> Result<AccessibilityAttachment, AccessibilityAlreadyAttached> {
        let mut slot = self.handler.lock();
        if slot.is_some() {
            return Err(AccessibilityAlreadyAttached);
        }
        *slot = Some(handler);
        Ok(AccessibilityAttachment::new(self.is_active(), capabilities))
    }

    /// Remove the handler under the lock and destroy it after releasing it.
    pub(crate) fn detach_handler(&self) {
        let handler = self.handler.lock().take();
        drop(handler);
    }

    /// Clone the live handler, releasing its slot before user code runs.
    fn handler(&self) -> Option<Arc<dyn AccessibilityHandler>> {
        self.handler.lock().clone()
    }

    /// Mark the attachment state and tell the composition root.
    ///
    /// The flag is stored **before** the handler runs: its natural response
    /// to "attached" is to enable semantics and publish, and a publish that
    /// consulted a stale `false` would be dropped by the adapter it was meant
    /// to initialize. A handler that panics restores the previous flag.
    pub(crate) fn notify_activation(&self, active: bool) {
        let previous = self.active.swap(active, Ordering::SeqCst);
        if let Some(handler) = self.handler() {
            let restore = RestoreOnUnwind {
                flag: &self.active,
                previous,
            };
            handler.activation_changed(active);
            #[expect(
                clippy::mem_forget,
                reason = "disarms the restore guard once the handler returned"
            )]
            std::mem::forget(restore);
        }
    }

    /// Forward an inbound action request (clone-and-release, as above).
    pub(crate) fn notify_action(&self, request: ActionRequest) {
        if let Some(handler) = self.handler() {
            handler.action_requested(request);
        }
    }

    /// Tell the composition root where the assistive technology's cursor
    /// moved (clone-and-release, as above). A node of a tree other than the
    /// one FLUI publishes, `TreeId::ROOT`, is off FLUI's tree.
    #[cfg_attr(
        not(any(target_os = "ios", target_os = "android", test)),
        expect(
            dead_code,
            reason = "only the iOS and Android adapters report the cursor"
        )
    )]
    pub(crate) fn notify_assistive_focus(&self, node: Option<(NodeId, TreeId)>) {
        let node = node.and_then(|(node, tree)| (tree == TreeId::ROOT).then_some(node));
        if let Some(handler) = self.handler() {
            handler.assistive_focus_changed(node);
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

    use accesskit::{Action, ActionRequest, NodeId, TreeId};
    use flui_semantics::platform::{
        AccessibilityCapabilities, AccessibilityHandler, AssistiveFocusSupport, RouteChangeSupport,
    };
    use parking_lot::Mutex;

    use super::BridgeShared;

    #[derive(Default)]
    struct EventHandler {
        activations: AtomicUsize,
        actions: AtomicUsize,
        cursor: Mutex<Vec<Option<NodeId>>>,
        panic_activation: bool,
    }

    impl AccessibilityHandler for EventHandler {
        fn activation_changed(&self, _active: bool) {
            assert!(!self.panic_activation, "the activation handler failed");
            self.activations.fetch_add(1, Ordering::SeqCst);
        }

        fn action_requested(&self, _request: ActionRequest) {
            self.actions.fetch_add(1, Ordering::SeqCst);
        }

        fn assistive_focus_changed(&self, node: Option<NodeId>) {
            self.cursor.lock().push(node);
        }
    }

    fn request() -> ActionRequest {
        ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: NodeId(1),
            data: None,
        }
    }

    /// The composition root never took in an activation whose handler
    /// panicked, so the bridge does not report itself active, and the next
    /// attachment hears the next activation.
    #[test]
    fn an_activation_whose_listener_panicked_leaves_the_bridge_inactive() {
        let shared = BridgeShared::new();
        shared
            .attach(
                Arc::new(EventHandler {
                    panic_activation: true,
                    ..EventHandler::default()
                }),
                AccessibilityCapabilities::NONE,
            )
            .expect("BUG: the first handler attaches");
        let unwound = catch_unwind(AssertUnwindSafe(|| shared.notify_activation(true)));
        assert!(unwound.is_err(), "the handler's panic unwinds to the entry");
        assert!(
            !shared.is_active(),
            "the root never heard of the activation"
        );

        shared.detach_handler();
        let heard = Arc::new(EventHandler::default());
        shared
            .attach(heard.clone(), AccessibilityCapabilities::NONE)
            .expect("BUG: a detached handler can be replaced");
        shared.notify_activation(true);
        assert!(shared.is_active());
        assert_eq!(heard.activations.load(Ordering::SeqCst), 1);
    }

    /// The cursor reaches the composition root as a node of FLUI's tree; a
    /// node of another tree is off it.
    #[test]
    fn the_assistive_cursor_reaches_the_listener_on_flui_s_tree_only() {
        use accesskit::Uuid;

        let shared = BridgeShared::new();
        let heard = Arc::new(EventHandler::default());
        shared
            .attach(heard.clone(), AccessibilityCapabilities::NONE)
            .expect("BUG: the first handler attaches");

        let foreign = TreeId(Uuid::from_u128(7));
        shared.notify_assistive_focus(Some((NodeId(3), TreeId::ROOT)));
        shared.notify_assistive_focus(Some((NodeId(3), foreign)));
        shared.notify_assistive_focus(None);
        assert_eq!(*heard.cursor.lock(), [Some(NodeId(3)), None, None]);
    }

    #[test]
    fn a_second_attachment_does_not_replace_the_live_handler() {
        let shared = BridgeShared::new();
        let first = Arc::new(EventHandler::default());
        let second = Arc::new(EventHandler::default());
        shared
            .attach(first.clone(), AccessibilityCapabilities::NONE)
            .expect("BUG: the first handler attaches");
        shared
            .attach(second, AccessibilityCapabilities::NONE)
            .expect_err("a live handler cannot be replaced");

        shared.notify_activation(true);

        assert_eq!(first.activations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn detaching_stops_every_inbound_event_and_releases_the_handler() {
        let shared = BridgeShared::new();
        let handler = Arc::new(EventHandler::default());
        shared
            .attach(handler.clone(), AccessibilityCapabilities::NONE)
            .expect("BUG: the first handler attaches");
        shared.detach_handler();
        assert_eq!(Arc::strong_count(&handler), 1);

        shared.notify_activation(true);
        shared.notify_action(request());
        shared.notify_assistive_focus(Some((NodeId(3), TreeId::ROOT)));

        assert_eq!(handler.activations.load(Ordering::SeqCst), 0);
        assert_eq!(handler.actions.load(Ordering::SeqCst), 0);
        assert!(handler.cursor.lock().is_empty());
    }

    #[test]
    fn attachment_snapshots_an_already_active_adapter_and_its_capabilities() {
        let shared = BridgeShared::new();
        shared.notify_activation(true);
        let capabilities = AccessibilityCapabilities::new(
            AssistiveFocusSupport::Reported,
            RouteChangeSupport::Native,
        );

        let attachment = shared
            .attach(Arc::new(EventHandler::default()), capabilities)
            .expect("BUG: the first handler attaches");

        assert!(attachment.is_active());
        assert_eq!(attachment.capabilities(), capabilities);
    }
}
