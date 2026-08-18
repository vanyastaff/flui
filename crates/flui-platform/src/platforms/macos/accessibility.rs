//! NSAccessibility for macOS, via `accesskit_macos`.
//!
//! Implements [`PlatformAccessibility`] by wrapping
//! [`accesskit_macos::SubclassingAdapter`], which dynamically subclasses the
//! window's content `NSView` to implement the `NSAccessibility` protocol
//! VoiceOver queries. Subclassing is chosen for the same reason as on
//! Windows: it needs no edits to the backend's own view class, keeping the
//! integration additive to a backend this repository can only type-check.
//!
//! # The activation model differs from AT-SPI
//!
//! Like UIA, NSAccessibility has no attach/detach session: the activation
//! handler fires the first time VoiceOver (or any client) asks for the
//! tree, and nothing ever fires a deactivation. Once asked, semantics
//! assembly stays enabled for the window's lifetime — the platform's shape,
//! not an oversight.
//!
//! # Known gap: view focus state
//!
//! [`MacosAccessibility::update_view_focus_state`] exists and forwards to
//! the adapter, but no backend seam calls it yet — the AppKit backend has
//! no per-window key-state delegate wired through to this capability.
//! VoiceOver still queries the tree; what suffers until it is wired is
//! focus-follows announcement fidelity when the window gains or loses key
//! status.
//!
//! # Honesty note: type-checked, never executed
//!
//! Like the whole AppKit backend, this module is covered only by the
//! `cross-typecheck` gate (clippy, no link, no tests). The retention and
//! dispatch rules it relies on are executed on Linux via
//! [`BridgeShared`]'s own tests; the adapter shell around them has not run
//! on a real macOS machine yet.

use std::sync::Arc;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_macos::SubclassingAdapter;
use parking_lot::Mutex;

use crate::shared::accessibility_bridge::BridgeShared;
use crate::traits::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};

struct Activation(Arc<BridgeShared>);

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        // Order matters: mark active and tell the composition root *before*
        // reading the retained tree — see the Windows twin for the cold-start
        // rationale.
        self.0.notify_activation(true);
        self.0.retained()
    }
}

struct Action(Arc<BridgeShared>);

impl ActionHandler for Action {
    fn do_action(&mut self, request: ActionRequest) {
        self.0.notify_action(request);
    }
}

/// NSAccessibility for one window.
pub struct MacosAccessibility {
    shared: Arc<BridgeShared>,
    /// The adapter needs `&mut` to publish, and the capability is shared
    /// behind an `Arc`, so the mutability lives here rather than in the
    /// trait.
    adapter: Mutex<SubclassingAdapter>,
}

// SAFETY, per field: `shared` is `Arc<BridgeShared>`, `Send + Sync` by
// construction. The `Mutex<SubclassingAdapter>` serializes every touch of
// the adapter, whose auto-trait opt-outs come from (a) the type-erased
// handler boxes, filled here only with `Activation`/`Action` over
// `Arc<BridgeShared>` — concrete `Send + Sync` types — and (b) the raw
// NSView-adjacent subclass state, which is main-thread-AFFINE AppKit state
// the same way `MacOSWindow`'s own `unsafe impl`s document: the
// accessibility-protocol overrides run where AppKit delivers them (the
// main thread), and `update_if_active` hands the update to AccessKit's
// adapter, which serializes internally.
//
// NOT claimed — the same gap `MacOSWindow` documents: dropping this value
// unhooks the dynamic subclass, which AppKit wants done on the main
// thread. The window owns this capability, so drop follows window
// teardown; a teardown path that drops the last `Arc` elsewhere inherits
// the identical, already-documented affinity obligation.
unsafe impl Send for MacosAccessibility {}
// SAFETY: see `Send` above — all interior mutability is `Mutex`-guarded.
unsafe impl Sync for MacosAccessibility {}

impl MacosAccessibility {
    /// Subclass `view` (a live `NSView*`) and answer VoiceOver for it.
    ///
    /// # Safety
    ///
    /// `view` must be a valid, live `NSView` pointer, and it must outlive
    /// the returned value — the adapter unhooks its dynamic subclass on
    /// drop, which dereferences the view. The window constructor satisfies
    /// both: it passes the content view it just installed, and the window
    /// owns this capability so drop order follows window teardown. Must be
    /// called on the main thread (AppKit affinity), which the window
    /// constructor also is.
    #[must_use]
    pub unsafe fn new(view: *mut std::ffi::c_void) -> Self {
        let shared = Arc::new(BridgeShared::new());
        // SAFETY: forwarded contract — see this function's own `# Safety`.
        let adapter = unsafe {
            SubclassingAdapter::new(
                view,
                Activation(Arc::clone(&shared)),
                Action(Arc::clone(&shared)),
            )
        };

        Self {
            shared,
            adapter: Mutex::new(adapter),
        }
    }

    /// Tell VoiceOver whether the subclassed view is in the key window.
    ///
    /// Not yet called from any backend seam — see the module's known-gap
    /// note. Kept public so the wiring lands as a one-line delegate call.
    pub fn update_view_focus_state(&self, is_focused: bool) {
        let events = {
            let mut adapter = self.adapter.lock();
            adapter.update_view_focus_state(is_focused)
        };
        if let Some(events) = events {
            events.raise();
        }
    }
}

impl std::fmt::Debug for MacosAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacosAccessibility")
            .field("active", &self.shared.is_active())
            .finish_non_exhaustive()
    }
}

impl PlatformAccessibility for MacosAccessibility {
    fn publish(&self, update: TreeUpdate) {
        // Same retention contract as the AT-SPI bridge: only self-contained
        // updates may answer a late activation — see `BridgeShared`.
        self.shared.retain_if_self_contained(&update);
        // AccessKit's contract: `QueuedEvents` are raised OUTSIDE the
        // adapter lock, because NSAccessibility notification handlers can
        // re-enter.
        let events = {
            let mut adapter = self.adapter.lock();
            adapter.update_if_active(move || update)
        };
        if let Some(events) = events {
            events.raise();
        }
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
