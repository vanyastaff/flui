//! UIA accessibility for Windows, via `accesskit_windows`.
//!
//! Implements [`PlatformAccessibility`] by wrapping
//! [`accesskit_windows::SubclassingAdapter`], which subclasses the window
//! procedure to answer `WM_GETOBJECT` — the message UI Automation clients
//! (Narrator, NVDA, JAWS) send to discover a window's accessibility tree.
//! The subclassing variant is chosen deliberately: it needs no edits to the
//! backend's own `window_proc`, so the integration stays additive to a
//! backend this repository can only type-check (see the honesty note below).
//!
//! # The activation model differs from AT-SPI
//!
//! UIA has no attach/detach session: the adapter's activation handler fires
//! the first time any client asks for the tree, and **nothing ever fires a
//! deactivation** — there is no "the screen reader left" signal to stop
//! semantics assembly on. Once a client has asked, assembly stays enabled
//! for the window's lifetime. That is the platform's shape, not an
//! oversight; AT-SPI's explicit deactivation is the outlier.
//!
//! # Honesty note: type-checked, never executed
//!
//! Like the whole Win32 backend, this module is covered only by the
//! `cross-typecheck` gate (clippy, no link, no tests). The retention and
//! dispatch rules it relies on are executed on Linux via
//! [`BridgeShared`]'s own tests; the adapter shell around them has not run
//! on a real Windows machine yet.

use std::sync::Arc;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_windows::SubclassingAdapter;
use parking_lot::Mutex;
use windows::Win32::Foundation::HWND;

use crate::shared::accessibility_bridge::BridgeShared;
use crate::traits::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};

struct Activation(Arc<BridgeShared>);

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        // Order matters: mark active and tell the composition root *before*
        // reading the retained tree. The listener typically enables
        // semantics, and on a cold start that is what will produce the first
        // tree at all — reading first would answer `None` even when a
        // synchronous listener could have supplied one.
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

/// UIA accessibility for one window.
pub struct WindowsAccessibility {
    shared: Arc<BridgeShared>,
    /// The adapter needs `&mut` to publish, and the capability is shared
    /// behind an `Arc`, so the mutability lives here rather than in the
    /// trait.
    adapter: Mutex<SubclassingAdapter>,
}

// SAFETY, per field: `shared` is `Arc<BridgeShared>`, itself `Send + Sync`
// by construction (atomics + `Mutex`es over `Send + Sync` payloads). The
// `Mutex<SubclassingAdapter>` serializes every touch of the adapter, whose
// auto-trait opt-outs come from (a) the type-erased handler boxes, which
// this module only ever fills with `Activation`/`Action` over
// `Arc<BridgeShared>` — concrete `Send + Sync` types — and (b) the raw
// HWND-adjacent subclass state, which is thread-AFFINE Win32 state the
// same way `WindowsWindow`'s own `unsafe impl`s document: the subclass
// hook itself only runs on the window's owning thread (inside its message
// loop), and `update_if_active` delegates to the inner UIA adapter, which
// AccessKit documents as callable from any thread.
//
// NOT claimed — the same gap `WindowsWindow` documents: dropping this
// value unhooks the subclass, which Win32 wants done on the owning
// thread. The window owns this capability, so drop follows window
// teardown; a teardown path that drops the last `Arc` on a foreign thread
// inherits the identical, already-documented affinity obligation.
unsafe impl Send for WindowsAccessibility {}
// SAFETY: see `Send` above — all interior mutability is `Mutex`-guarded.
unsafe impl Sync for WindowsAccessibility {}

impl WindowsAccessibility {
    /// Subclass `hwnd` and answer UIA clients for it.
    ///
    /// Must be called on the thread that owns `hwnd` (Win32 subclassing is
    /// thread-affine); the window constructor is that thread. Constructing
    /// with no assistive technology running is inert — the adapter answers
    /// `WM_GETOBJECT` only when a client actually asks.
    #[must_use]
    pub fn new(hwnd: HWND) -> Self {
        let shared = Arc::new(BridgeShared::new());
        let adapter = SubclassingAdapter::new(
            hwnd,
            Activation(Arc::clone(&shared)),
            Action(Arc::clone(&shared)),
        );

        Self {
            shared,
            adapter: Mutex::new(adapter),
        }
    }
}

impl std::fmt::Debug for WindowsAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsAccessibility")
            .field("active", &self.shared.is_active())
            .finish_non_exhaustive()
    }
}

impl PlatformAccessibility for WindowsAccessibility {
    fn publish(&self, update: TreeUpdate) {
        // Same retention contract as the AT-SPI bridge: only self-contained
        // updates may answer a late activation — see `BridgeShared`.
        self.shared.retain_if_self_contained(&update);
        // AccessKit's contract: `QueuedEvents` must be raised OUTSIDE any
        // lock the tree state lives behind, because UIA event handlers can
        // re-enter. Hence the explicit guard drop before `raise`.
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
