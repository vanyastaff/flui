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
//! # What executes it
//!
//! A UIA client driving the generated counter on a real window reads this
//! adapter's tree, invokes the button and reads the new count back. That is a
//! manual check on a Windows desktop, not a CI job; CI only type-checks this module. The
//! retention and dispatch rules it relies on are executed on every host via
//! [`BridgeShared`]'s own tests.

// The adapter holds UIA objects that are not `Send`; the window claims
// `Send`/`Sync` by hand: the workspace lint `unsafe_code = "warn"` is opted out
// here, at the module boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::sync::Arc;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_windows::SubclassingAdapter;
use parking_lot::Mutex;
use windows::Win32::Foundation::HWND;

use super::bridge::{Action, Activation, BridgeShared};
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use flui_semantics::{
    platform::AccessibilityActionListener, platform::AccessibilityActivationListener,
    platform::PlatformAccessibility,
};

/// UIA accessibility for one window.
pub struct WindowsAccessibility {
    shared: Arc<BridgeShared>,
    /// The adapter needs `&mut` to publish, and the capability is shared
    /// behind an `Arc`, so the mutability lives here rather than in the
    /// trait. `Option` because the adapter's lifetime is deliberately
    /// shorter than the capability's: `detach` (window teardown)
    /// or a same-thread [`Drop`] takes it out, and every later call through
    /// a still-live `Arc` clone is a guarded no-op instead of a touch of
    /// torn-down subclass state.
    adapter: Mutex<Option<SubclassingAdapter>>,
    /// The thread that owns the subclass hook (the window's creating
    /// thread) — the only thread allowed to run the adapter's destructor.
    owner_thread: std::thread::ThreadId,
}

// SAFETY, per field: `shared` is `Arc<BridgeShared>`, itself `Send + Sync`
// by construction (atomics + `Mutex`es over `Send + Sync` payloads);
// `owner_thread` is a plain `ThreadId`. The `Mutex<Option<SubclassingAdapter>>`
// serializes every touch of the adapter, whose auto-trait opt-outs come
// from (a) the type-erased handler boxes, which this module only ever
// fills with `Activation`/`Action` over `Arc<BridgeShared>` — concrete
// `Send + Sync` types — and (b) the raw HWND-adjacent subclass state,
// which is thread-AFFINE Win32 state: the hook itself only runs on the
// window's owning thread (inside its message loop), and `update_if_active`
// delegates to the inner UIA adapter, which AccessKit documents as
// callable from any thread.
//
// The remaining affinity obligation — the destructor unhooks the subclass,
// which Win32 requires on the owning thread — is ENFORCED, not merely
// documented: `Self::drop` runs the adapter's destructor only when the
// current thread is `owner_thread`, and otherwise leaks it (with a warning)
// rather than unhooking cross-thread. Safe code can therefore move or drop
// the last `Arc` anywhere without reaching an unsound path.
unsafe impl Send for WindowsAccessibility {}
// SAFETY: see `Send` above — all interior mutability is `Mutex`-guarded.
unsafe impl Sync for WindowsAccessibility {}

impl Drop for WindowsAccessibility {
    fn drop(&mut self) {
        let Some(adapter) = self.adapter.get_mut().take() else {
            return;
        };
        if std::thread::current().id() == self.owner_thread {
            drop(adapter);
        } else {
            // Unhooking a Win32 subclass from a foreign thread races the
            // owner thread's message dispatch; leaking the hook is safe
            // (the window is on its way down with it) and sound, so it is
            // the only acceptable fallback.
            crate::owner::panic_boundary::retain_off_owner_resource(adapter, || {
                tracing::warn!(
                    "leaking a UIA subclass adapter dropped off its owner thread \
                     (unhooking cross-thread would race message dispatch)"
                );
            });
        }
    }
}

impl WindowsAccessibility {
    /// Subclass `hwnd` and answer UIA clients for it.
    ///
    /// Must be called on the thread that owns `hwnd` (Win32 subclassing is
    /// thread-affine); the window constructor is that thread. Constructing
    /// with no assistive technology running is inert — the adapter answers
    /// `WM_GETOBJECT` only when a client actually asks.
    #[must_use]
    pub(crate) fn new(hwnd: HWND) -> Self {
        let shared = Arc::new(BridgeShared::new());
        let adapter = SubclassingAdapter::new(
            hwnd,
            Activation(Arc::clone(&shared)),
            Action(Arc::clone(&shared)),
        );

        Self {
            shared,
            adapter: Mutex::new(Some(adapter)),
            owner_thread: std::thread::current().id(),
        }
    }
}

impl crate::host::WindowAccessibility for WindowsAccessibility {
    /// Unhook and drop the adapter now, on the caller's (owner) thread —
    /// the deterministic half of teardown, called by the window's own
    /// `Drop` **before** it destroys the HWND, so the subclass is removed
    /// while the window still exists. Every later call through a still-live
    /// capability `Arc` is a no-op. Idempotent.
    fn detach(&self) {
        let adapter = self.adapter.lock().take();
        drop(adapter);
    }

    /// The subclassed procedure sees `WM_SETFOCUS` and `WM_KILLFOCUS` itself.
    fn focus_changed(&self, _: bool) {}
}

/// Each Win32 window subclasses its own procedure for UIA, on the thread
/// that created it, where the table's position makes the bridge.
impl crate::host::Accessibility<crate::owner::windows::WindowRef> for WindowsAccessibility {
    fn for_window(
        window: &crate::owner::windows::WindowRef,
    ) -> Arc<dyn crate::host::WindowAccessibility> {
        Arc::new(Self::new(window.hwnd()))
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
        if tracing::enabled!(tracing::Level::TRACE) {
            let nodes: Vec<_> = update
                .nodes
                .iter()
                .map(|(id, node)| (id.0, node.role(), node.children().len()))
                .collect();
            tracing::trace!(
                full = update.tree.is_some(),
                focus = update.focus.0,
                ?nodes,
                "publishing an accessibility update"
            );
        }
        // Same retention contract as the AT-SPI bridge: only self-contained
        // updates may answer a late activation — see `BridgeShared`.
        self.shared.retain_if_self_contained(&update);
        // AccessKit's contract: `QueuedEvents` must be raised OUTSIDE any
        // lock the tree state lives behind, because UIA event handlers can
        // re-enter. Hence the explicit guard drop before `raise`. A
        // shut-down capability (adapter taken by teardown) drops the update
        // instead — see the `adapter` field doc.
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

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;

    use accesskit::{ActionHandler, ActionRequest, ActivationHandler, NodeId, TreeId};

    use super::{Action, Activation, BridgeShared};

    /// AccessKit calls the handlers from its UIA vtable methods, which are
    /// `extern "system"`: a panic of the composition root's listener is
    /// contained by the handler, which answers as if no listener ran.
    /// Red-check: call the listener outside `os_entry` (the panic unwinds
    /// out of the handler, and under UIA aborts the process).
    #[test]
    fn a_panicking_listener_stays_inside_its_handler() {
        let shared = Arc::new(BridgeShared::new());
        shared.set_activation_listener(Arc::new(|_| panic!("activation listener")));
        shared.set_action_listener(Arc::new(|_| panic!("action listener")));

        let contained = catch_unwind(AssertUnwindSafe(|| {
            let initial = Activation(Arc::clone(&shared)).request_initial_tree();
            Action(Arc::clone(&shared)).do_action(ActionRequest {
                action: accesskit::Action::Click,
                target_tree: TreeId::ROOT,
                target_node: NodeId(1),
                data: None,
            });
            initial
        }));

        let initial = contained.expect("a listener's panic unwound out of its handler");
        assert!(initial.is_none(), "a failed activation answers no tree");
        assert!(
            shared.is_active(),
            "the activation reached the bridge before its listener failed"
        );
    }
}
