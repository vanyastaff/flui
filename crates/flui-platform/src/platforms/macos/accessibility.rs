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
//! # View focus state
//!
//! [`MacosAccessibility::update_view_focus_state`] is wired from the
//! window's key-status delegate path (`handle_focus_gained` /
//! `handle_focus_lost` in `window.rs`), so VoiceOver's notion of the
//! focused view tracks the key window — load-bearing in any multi-window
//! application.
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
    /// trait. `Option` because the adapter's lifetime is deliberately
    /// shorter than the capability's: [`Self::shutdown`] (window teardown,
    /// **before** the content view is released) or a same-thread [`Drop`]
    /// takes it out, and every later call through a still-live `Arc` clone
    /// is a guarded no-op instead of a dereference of a dead view.
    adapter: Mutex<Option<SubclassingAdapter>>,
    /// The thread that owns the subclass (the main thread, where the window
    /// constructor runs) — the only thread allowed to run the adapter's
    /// destructor, since unhooking dereferences the view.
    owner_thread: std::thread::ThreadId,
}

// SAFETY, per field: `shared` is `Arc<BridgeShared>`, `Send + Sync` by
// construction; `owner_thread` is a plain `ThreadId`. The
// `Mutex<Option<SubclassingAdapter>>` serializes every touch of the
// adapter, whose auto-trait opt-outs come from (a) the type-erased handler
// boxes, filled here only with `Activation`/`Action` over
// `Arc<BridgeShared>` — concrete `Send + Sync` types — and (b) the raw
// NSView-adjacent subclass state, which is main-thread-AFFINE AppKit
// state: the accessibility-protocol overrides run where AppKit delivers
// them (the main thread), and `update_if_active` hands the update to
// AccessKit's adapter, which serializes internally.
//
// The remaining affinity + lifetime obligations — the destructor unhooks
// the dynamic subclass, which dereferences the view and must happen on the
// main thread while the view is alive — are ENFORCED, not merely
// documented: the window's own teardown calls [`MacosAccessibility::shutdown`]
// before releasing the view, and `Self::drop` runs the destructor only on
// `owner_thread`, otherwise leaking (with a warning) rather than touching
// AppKit state cross-thread. Safe code can therefore move or drop the last
// `Arc` anywhere without reaching an unsound path.
unsafe impl Send for MacosAccessibility {}
// SAFETY: see `Send` above — all interior mutability is `Mutex`-guarded.
unsafe impl Sync for MacosAccessibility {}

impl Drop for MacosAccessibility {
    fn drop(&mut self) {
        let Some(adapter) = self.adapter.get_mut().take() else {
            return;
        };
        if std::thread::current().id() == self.owner_thread {
            drop(adapter);
        } else {
            // Unhooking the dynamic subclass dereferences the view and
            // must happen on the main thread; leaking the hook is safe
            // (the view is on its way down with its window) and sound, so
            // it is the only acceptable fallback. Reaching here at all
            // means the window's own teardown (which calls `shutdown`
            // first) was bypassed.
            tracing::warn!(
                "leaking an NSAccessibility subclass adapter dropped off the \
                 main thread (unhooking would touch AppKit state cross-thread)"
            );
            std::mem::forget(adapter);
        }
    }
}

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
            adapter: Mutex::new(Some(adapter)),
            owner_thread: std::thread::current().id(),
        }
    }

    /// Unhook and drop the adapter now, on the caller's (main) thread —
    /// the deterministic half of teardown, called by the window's own
    /// `Drop` **before** it releases the window (and with it the content
    /// view the subclass dereferences on unhook). Every later call through
    /// a still-live capability `Arc` is a no-op. Idempotent.
    pub(crate) fn shutdown(&self) {
        let adapter = self.adapter.lock().take();
        drop(adapter);
    }

    /// Tell VoiceOver whether the subclassed view is in the key window.
    ///
    /// Called from the window's `windowDidBecomeKey:` /
    /// `windowDidResignKey:` delegate path (`handle_focus_gained` /
    /// `handle_focus_lost`), on the main thread.
    pub fn update_view_focus_state(&self, is_focused: bool) {
        let events = {
            let mut adapter = self.adapter.lock();
            adapter
                .as_mut()
                .and_then(|adapter| adapter.update_view_focus_state(is_focused))
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
        // re-enter. A shut-down capability (adapter taken by teardown)
        // drops the update instead — see the `adapter` field doc.
        let events = {
            let mut adapter = self.adapter.lock();
            adapter
                .as_mut()
                .and_then(|adapter| adapter.update_if_active(move || update))
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
