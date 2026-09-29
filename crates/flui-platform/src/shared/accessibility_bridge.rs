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
//! [`PlatformAccessibility`]: crate::traits::PlatformAccessibility

use std::sync::atomic::{AtomicBool, Ordering};

use accesskit::{ActionRequest, TreeUpdate};
use parking_lot::Mutex;

use crate::traits::{AccessibilityActionListener, AccessibilityActivationListener};

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
    pub(crate) fn notify_activation(&self, active: bool) {
        self.active.store(active, Ordering::SeqCst);
        let listener = self.activation_listener.lock().clone();
        if let Some(listener) = listener {
            listener(active);
        }
    }

    /// Forward an inbound action request (clone-and-release, as above).
    pub(crate) fn notify_action(&self, request: ActionRequest) {
        let listener = self.action_listener.lock().clone();
        if let Some(listener) = listener {
            listener(request);
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
}

impl std::fmt::Debug for BridgeShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeShared")
            .field("active", &self.is_active())
            .field("has_retained", &self.latest.lock().is_some())
            .finish_non_exhaustive()
    }
}
