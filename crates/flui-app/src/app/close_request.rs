//! Per-window close-request veto — the application's answer to "may this
//! window close?" (issue #558's cancel-or-defer criterion).
//!
//! # The seam this fills
//!
//! [`flui_platform::traits::PlatformWindow::on_should_close`] is a real
//! per-window veto: the winit, Win32 and AppKit backends all consult it
//! synchronously when the *user* asks a window to close, and a `false`
//! answer stops the close before anything else in the arm runs — no
//! `on_close`, no realm teardown, no exit-policy consultation. Until this
//! module existed every `flui-app` registration hard-coded `true`, so the
//! mechanism shipped and no application could reach it.
//!
//! # Presentation-addressed, because the question is per window
//!
//! A multi-window application must be able to answer differently for each
//! window: a document window with unsaved work says "not yet" while the
//! preferences window beside it closes normally. [`PresentationAddress`]
//! is this workspace's window identity (ADR-0037 §2), so it is what a
//! handler is registered against and what [`CloseRequest::address`] hands
//! back — a single handler shared by two presentations can still
//! discriminate.
//!
//! # Not the keep-alive veto
//!
//! [`ExitPolicy`](crate::ExitPolicy) plus
//! [`ServiceLifetime::KeepsAppAlive`](crate::ServiceLifetime::KeepsAppAlive)
//! answer a *different*, strictly later question: now that every window is
//! gone, may the **process** exit? The two never compete, and the order is
//! fixed by causality rather than by a choice made here — see
//! [`CloseRequestRouter::consult`]'s own doc.
//!
//! # A veto is stateless, which is what makes it finite
//!
//! [`CloseResponse::KeepOpen`] is a complete answer to *this* request. The
//! runtime records nothing, arms no timer, and owes nothing: the window
//! stays open and fully interactive, its close affordance still works, and
//! the next request is put to the handler afresh. There is therefore no
//! deferral that can be forgotten and no bound that has to be enforced.
//!
//! A wall-clock deadline was considered and refused on merit rather than
//! on cost. The canonical use of a close veto is unsaved work, where the
//! application raises a Save / Discard / Cancel prompt and waits for a
//! *human*; a timer that fires mid-decision would destroy exactly the data
//! the veto exists to protect. No reference does this — AppKit's
//! `windowShouldClose:`, Win32's `WM_CLOSE`, GTK's `delete-event`, Qt's
//! `closeEvent` and Flutter's `PopScope` all leave that wait unbounded.
//! What the application owes instead is the means to finish the close, and
//! it is handed that up front:
//! [`request_presentation_close`](crate::request_presentation_close) closes
//! the window once the work is done.
//!
//! Issue #558's own `deadline -> flush -> termination` leg is a different
//! question — how long teardown may take *after* a close is agreed — and
//! belongs with the journaled-state slice, not here.
//!
//! # Not in this slice (stated, not silently assumed)
//!
//! - **A widget-tier capability.** A handler is registered through
//!   [`AppConfig::with_close_request_handler`](crate::AppConfig::with_close_request_handler),
//!   the same embedder-facing route
//!   [`FrameFailureHandler`](crate::FrameFailureHandler) and
//!   [`ExitPolicy`](crate::ExitPolicy) take. There is deliberately no
//!   `BuildContext`-acquired handle yet, so no token joins
//!   `scripts/check-frame-capability-scope.sh` in this change — the widget
//!   that would consume one (a `PopScope`-shaped "this subtree has unsaved
//!   work" declaration) does not exist either, and shipping half of that
//!   pair is how a seam ends up unreachable. Same deliberate remainder
//!   [`TaskSpawner`](crate::TaskSpawner) carries in
//!   [`lifecycle`](super::lifecycle).
//! - **Backends that never consult the seam.** The web and Android
//!   backends implement the callback *setter* (through the shared
//!   `impl_window_callback_setters!` macro) but no code path in either
//!   calls `dispatch_should_close`, so a handler registered there is inert.
//!   That is a property of those backends, not of this module.

use std::sync::{Arc, Weak};
use std::thread::ThreadId;

use flui_foundation::{PresentationAddress, RealmId};
use flui_platform::traits::PlatformWindow;
use parking_lot::Mutex;

// ============================================================================
// The question, and the answer
// ============================================================================

/// One platform close request, addressed to the presentation being asked.
///
/// Handed to a [`CloseRequestHandler`] synchronously on the UI thread, from
/// inside the platform's own close-request handling and *before* anything
/// irreversible has happened: the native window is still open, the
/// presentation is still installed, and nothing has been torn down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CloseRequest {
    address: PresentationAddress,
}

impl CloseRequest {
    /// Which realm incarnation and which presentation within it is being
    /// asked to close.
    ///
    /// Store this if the answer is [`CloseResponse::KeepOpen`]: it is the
    /// address
    /// [`request_presentation_close`](crate::request_presentation_close)
    /// takes to finish the close later.
    #[must_use]
    pub fn address(&self) -> PresentationAddress {
        self.address
    }
}

/// The application's answer to a [`CloseRequest`].
///
/// `#[non_exhaustive]`: a future variant carrying a runtime-bounded flush
/// window (issue #558's `deadline -> flush -> termination` leg) would be an
/// additive change here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CloseResponse {
    /// Let the window close now. The close proceeds exactly as it would
    /// with no handler registered at all.
    Close,
    /// Keep the window open. Issue #558 calls this outcome *Cancel*; the
    /// name here says what happens rather than what was refused.
    ///
    /// This fully answers the request — the runtime keeps no pending
    /// obligation, so nothing has to time out. An application that
    /// answered this way in order to finish work calls
    /// [`request_presentation_close`](crate::request_presentation_close)
    /// when the work is done; one that answered because the user said
    /// "don't quit" does nothing further, and the next close request is
    /// put to the handler afresh.
    KeepOpen,
}

/// An embedder-registered callback deciding whether one presentation's
/// window may close.
///
/// Register via
/// [`AppConfig::with_close_request_handler`](crate::AppConfig::with_close_request_handler);
/// each window opened with that config registers it for its own
/// presentation.
///
/// # Contract
///
/// Invoked **synchronously on the UI (owner) thread**, from inside the
/// platform's close-request handling. `Fn`, not `FnMut`: the runtime clones
/// the callback out of its lock before calling it, so a handler may freely
/// call back into this seam (closing another window, say) without
/// deadlocking; put any state it needs to mutate behind interior
/// mutability.
///
/// Two failure modes are answered with [`CloseResponse::KeepOpen`] — the
/// same conservative veto `WindowCallbacks::dispatch_should_close` already
/// applies to a reentrant query — because neither can produce a trustworthy
/// answer and closing on a wrong answer destroys data:
///
/// - a handler that **panics** (contained here, logged at error level; the
///   handler stays registered and a later request reaches it normally);
/// - an invocation arriving on a **thread other than the one that
///   registered the handler**, which would mean a backend broke
///   [`PlatformWindow`]'s own same-thread callback contract.
#[derive(Clone)]
pub struct CloseRequestHandler(Arc<dyn Fn(&CloseRequest) -> CloseResponse + Send + Sync>);

impl CloseRequestHandler {
    /// Wrap a callback as a registerable handler.
    pub fn new(handler: impl Fn(&CloseRequest) -> CloseResponse + Send + Sync + 'static) -> Self {
        Self(Arc::new(handler))
    }

    fn call(&self, request: &CloseRequest) -> CloseResponse {
        (self.0)(request)
    }
}

impl std::fmt::Debug for CloseRequestHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The wrapped closure is opaque; identity is all Debug can say.
        f.debug_tuple("CloseRequestHandler").finish()
    }
}

/// Why a programmatic close could not be delivered.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum CloseRequestError {
    /// No presentation is registered at this address — it was never
    /// installed, or its window has already closed.
    #[error("no presentation is registered at {address:?}")]
    UnknownPresentation {
        /// The address that was asked for.
        address: PresentationAddress,
    },
    /// The presentation is registered but its native window has already
    /// been destroyed.
    #[error("the native window for {address:?} is already gone")]
    WindowGone {
        /// The address that was asked for.
        address: PresentationAddress,
    },
    /// Closing a window is an owner-thread operation; this call arrived on
    /// another thread.
    #[error("a close must be requested from the owner thread")]
    WrongThread,
}

// ============================================================================
// The router
// ============================================================================

/// One registered presentation: how to ask it, and how to close it.
struct PresentationCloseEntry {
    address: PresentationAddress,
    /// Weak on purpose: this router is loop-scoped and outlives any single
    /// window. A strong reference would pin a closed window's native
    /// resources — and, through the callbacks it owns, its GPU surface —
    /// alive past the teardown ordering issue #713's Wayland crash
    /// established.
    window: Weak<dyn PlatformWindow>,
    handler: Option<CloseRequestHandler>,
    /// The thread that registered this entry. Every `PlatformWindow`
    /// callback must be invoked on the thread that registered it (see the
    /// contract at the top of `PlatformWindow`'s callback section), so a
    /// mismatch means a backend broke that contract — never something to
    /// answer by reaching into realm state anyway.
    owner_thread: ThreadId,
}

/// Loop-scoped registry of per-presentation close-request handlers.
///
/// Held by [`AppRuntime`](super::runtime::AppRuntime) as an `Arc` rather
/// than inline, so the `on_should_close` closure each window registers can
/// hold its own clone and answer **without** re-entering the `APP_RUNTIME`
/// thread-local at all. That matters: a close request can arrive while a
/// realm is checked out for dispatch, and a router reached through the
/// realm would then have to fail closed on a bookkeeping detail the
/// application never asked about.
///
/// Deliberately not a second window authority:
/// [`WindowRegistry`](super::window_registry::WindowRegistry) owns the
/// native-key-to-[`PresentationAddress`] map and holds no window value,
/// while this keys on the address itself and carries the handler plus a
/// *weak* window. Nothing here names, accepts, or resolves a native window
/// key — the address is the only identity this module knows.
#[derive(Default)]
pub(crate) struct CloseRequestRouter {
    /// Private, and no guard ever escapes this type's own methods (SP-6):
    /// every caller gets a value out, never a lock.
    entries: Mutex<Vec<PresentationCloseEntry>>,
}

impl std::fmt::Debug for CloseRequestRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloseRequestRouter")
            .field("registered", &self.entries.lock().len())
            .finish()
    }
}

impl CloseRequestRouter {
    /// An empty router.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register `address` so it can be asked about, and closed
    /// programmatically. Called once per presentation, from the bootstrap
    /// that installed it, whether or not `handler` is `Some` — a window
    /// with no handler still has to be closable through
    /// [`Self::request_close`].
    ///
    /// Replaces any entry at the same address. `PresentationAddress` is
    /// generational, so this can only ever be a genuine re-registration of
    /// the same live presentation, never a stale incarnation colliding with
    /// a fresh one.
    pub(crate) fn register(
        &self,
        address: PresentationAddress,
        window: &Arc<dyn PlatformWindow>,
        handler: Option<CloseRequestHandler>,
    ) {
        let entry = PresentationCloseEntry {
            address,
            window: Arc::downgrade(window),
            handler,
            owner_thread: std::thread::current().id(),
        };
        let mut entries = self.entries.lock();
        match entries.iter_mut().find(|e| e.address == address) {
            Some(existing) => *existing = entry,
            None => entries.push(entry),
        }
    }

    /// Drop the entry for exactly this presentation.
    pub(crate) fn forget(&self, address: PresentationAddress) {
        self.entries.lock().retain(|e| e.address != address);
    }

    /// Drop every entry belonging to `realm` — the realm-wide uninstall
    /// counterpart of [`Self::forget`].
    pub(crate) fn forget_realm(&self, realm: RealmId) {
        self.entries.lock().retain(|e| e.address.realm_id != realm);
    }

    /// Ask the application whether the window at `address` may close.
    ///
    /// # Ordering against the keep-alive (process-exit) veto
    ///
    /// This runs **first**, and the ordering is causal rather than chosen.
    /// A backend consults this at the top of its close-request handling
    /// (winit: `WindowEvent::CloseRequested`, before `dispatch_close`,
    /// before the window leaves its tracking map); it consults the
    /// exit-policy hook — which is where `ServiceLifetime::KeepsAppAlive`
    /// vetoes — only once a close has already happened *and* left no
    /// window behind. So a [`CloseResponse::KeepOpen`] here means the exit
    /// question is never reached at all, and a running keep-alive service
    /// can never hold a window open: it defers the process exit that
    /// follows the last window closing, which is a different event.
    ///
    /// The reverse order is not merely worse, it is incoherent — "may the
    /// process exit now that the last window is gone" cannot be asked
    /// about a window that is still open.
    ///
    /// An unregistered address answers [`CloseResponse::Close`], matching
    /// the platform seam's own "no callback means close is allowed".
    pub(crate) fn consult(&self, address: PresentationAddress) -> CloseResponse {
        // Clone the handler out from under the lock before invoking it
        // (ADR-0039): application code may re-enter this router — closing a
        // sibling window, registering a handler — and this `Mutex` is not
        // reentrant.
        let Some((handler, owner_thread)) = ({
            let entries = self.entries.lock();
            entries
                .iter()
                .find(|e| e.address == address)
                .and_then(|e| e.handler.clone().map(|h| (h, e.owner_thread)))
        }) else {
            return CloseResponse::Close;
        };

        let current = std::thread::current().id();
        if current != owner_thread {
            tracing::error!(
                ?address,
                ?owner_thread,
                ?current,
                "close-request handler reached on a non-owner thread; vetoing the close rather \
                 than answering from the wrong thread"
            );
            return CloseResponse::KeepOpen;
        }

        let request = CloseRequest { address };
        let answered =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler.call(&request)));
        answered.unwrap_or_else(|_| {
            tracing::error!(
                ?address,
                "close-request handler panicked; vetoing the close (a panicking handler cannot \
                 be read as consent to discard unsaved work)"
            );
            CloseResponse::KeepOpen
        })
    }

    /// Close the window at `address` programmatically, bypassing the
    /// handler.
    ///
    /// This is how a [`CloseResponse::KeepOpen`] answer is finished: the
    /// application saved its work and now wants the close it deferred.
    /// Bypassing the handler is the point — every backend's own
    /// `PlatformWindow::close` bypasses its native close-request path for
    /// the same reason (AppKit's `-close` does not send
    /// `windowShouldClose:`; Win32's `DestroyWindow` does not send
    /// `WM_CLOSE`), because asking again would either loop forever or
    /// require the application to track "I am the one closing this".
    pub(crate) fn request_close(
        &self,
        address: PresentationAddress,
    ) -> Result<(), CloseRequestError> {
        let window = {
            let entries = self.entries.lock();
            let entry = entries
                .iter()
                .find(|e| e.address == address)
                .ok_or(CloseRequestError::UnknownPresentation { address })?;
            if std::thread::current().id() != entry.owner_thread {
                return Err(CloseRequestError::WrongThread);
            }
            entry
                .window
                .upgrade()
                .ok_or(CloseRequestError::WindowGone { address })?
        };
        // Outside the lock: `close()` fires the window's own `on_close`,
        // which re-enters flui-app (`close_this_window`) and may re-enter
        // this router.
        window.close();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_foundation::{PresentationId, RealmId};

    use super::*;
    use crate::app::window_test_support::TestWindow;

    fn address(realm: usize, presentation: usize) -> PresentationAddress {
        PresentationAddress {
            realm_id: RealmId::new(realm),
            presentation_id: PresentationId::new(presentation),
        }
    }

    fn window(id: u64) -> Arc<dyn PlatformWindow> {
        Arc::new(TestWindow::new().with_id(id)) as Arc<dyn PlatformWindow>
    }

    /// The pre-wiring default has to survive: an address nobody registered
    /// a handler for closes, exactly as the platform seam's own "no
    /// callback means close is allowed" does. Getting this backwards would
    /// make every window in every FLUI application unclosable.
    #[test]
    fn an_address_with_no_handler_closes() {
        let router = CloseRequestRouter::new();
        let a = address(1, 1);

        assert_eq!(
            router.consult(a),
            CloseResponse::Close,
            "an address that was never registered must not veto"
        );

        router.register(a, &window(1), None);
        assert_eq!(
            router.consult(a),
            CloseResponse::Close,
            "a registered address with no handler must not veto either -- registration exists \
             for the programmatic-close route, and must not change the answer on its own"
        );
    }

    /// The whole point of the seam: a handler answering `KeepOpen` is what
    /// the caller sees.
    #[test]
    fn a_handler_that_keeps_the_window_open_is_reported_verbatim() {
        let router = CloseRequestRouter::new();
        let a = address(1, 1);
        let asked = Arc::new(AtomicUsize::new(0));
        let asked_in_handler = Arc::clone(&asked);

        router.register(
            a,
            &window(1),
            Some(CloseRequestHandler::new(move |request| {
                assert_eq!(
                    request.address(),
                    a,
                    "the request must carry the address it was registered against, so one \
                     handler shared by several presentations can discriminate"
                );
                asked_in_handler.fetch_add(1, Ordering::SeqCst);
                CloseResponse::KeepOpen
            })),
        );

        assert_eq!(router.consult(a), CloseResponse::KeepOpen);
        assert_eq!(asked.load(Ordering::SeqCst), 1, "asked exactly once");

        // Stateless veto: nothing was recorded, so a second request reaches
        // the handler afresh rather than being answered from a latched
        // "already vetoed" flag.
        assert_eq!(router.consult(a), CloseResponse::KeepOpen);
        assert_eq!(asked.load(Ordering::SeqCst), 2);
    }

    /// Two presentations, two answers, one router: the addressing that lets
    /// a document window refuse a close while the preferences window beside
    /// it closes normally. The sibling's handler must not even be consulted.
    #[test]
    fn one_presentations_veto_does_not_reach_its_sibling() {
        let router = CloseRequestRouter::new();
        let keeps_open = address(1, 1);
        let closes = address(1, 2);
        let other_realm = address(2, 1);

        let keeps_open_asked = Arc::new(AtomicUsize::new(0));
        let asked = Arc::clone(&keeps_open_asked);
        router.register(
            keeps_open,
            &window(1),
            Some(CloseRequestHandler::new(move |_| {
                asked.fetch_add(1, Ordering::SeqCst);
                CloseResponse::KeepOpen
            })),
        );
        router.register(
            closes,
            &window(2),
            Some(CloseRequestHandler::new(|_| CloseResponse::Close)),
        );

        assert_eq!(router.consult(closes), CloseResponse::Close);
        assert_eq!(
            keeps_open_asked.load(Ordering::SeqCst),
            0,
            "a sibling's close request must not consult this presentation's handler at all"
        );
        assert_eq!(router.consult(keeps_open), CloseResponse::KeepOpen);

        // A same-numbered presentation in a different realm is a different
        // window, not this one -- the reason the address is a pair.
        assert_eq!(router.consult(other_realm), CloseResponse::Close);
    }

    /// A panicking handler cannot be read as consent to discard unsaved
    /// work, so it vetoes -- the same conservative answer
    /// `WindowCallbacks::dispatch_should_close` gives a reentrant query --
    /// and stays registered, so a handler that stops panicking works again.
    #[test]
    fn a_panicking_handler_vetoes_and_stays_registered() {
        let router = CloseRequestRouter::new();
        let a = address(1, 1);
        let panics = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let panics_in_handler = Arc::clone(&panics);

        router.register(
            a,
            &window(1),
            Some(CloseRequestHandler::new(move |_| {
                assert!(
                    !panics_in_handler.load(Ordering::SeqCst),
                    "deliberate handler panic under test"
                );
                CloseResponse::Close
            })),
        );

        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let vetoed = router.consult(a);
        std::panic::set_hook(previous_hook);
        assert_eq!(vetoed, CloseResponse::KeepOpen);

        panics.store(false, Ordering::SeqCst);
        assert_eq!(
            router.consult(a),
            CloseResponse::Close,
            "the handler must still be registered after containing its panic"
        );
    }

    /// `PlatformWindow` requires every callback to be invoked on the thread
    /// that registered it. A backend that broke that contract must not be
    /// answered by running application code on the wrong thread: refuse,
    /// loudly, and veto.
    #[test]
    fn a_query_from_another_thread_vetoes_without_calling_the_handler() {
        let router = Arc::new(CloseRequestRouter::new());
        let a = address(1, 1);
        let asked = Arc::new(AtomicUsize::new(0));
        let asked_in_handler = Arc::clone(&asked);

        router.register(
            a,
            &window(1),
            Some(CloseRequestHandler::new(move |_| {
                asked_in_handler.fetch_add(1, Ordering::SeqCst);
                CloseResponse::Close
            })),
        );

        let router_for_thread = Arc::clone(&router);
        let answer = std::thread::spawn(move || router_for_thread.consult(a))
            .join()
            .expect("the consulting thread must not panic");

        assert_eq!(answer, CloseResponse::KeepOpen);
        assert_eq!(
            asked.load(Ordering::SeqCst),
            0,
            "the handler must not run on a thread other than the one that registered it"
        );
        assert_eq!(
            router.consult(a),
            CloseResponse::Close,
            "the owner thread still gets a real answer"
        );
    }

    /// A handler is cloned out from under the router's lock before it runs
    /// (ADR-0039), so application code may close a sibling window, or query
    /// one, from inside its own answer without deadlocking.
    #[test]
    fn a_handler_may_re_enter_the_router() {
        let router = Arc::new(CloseRequestRouter::new());
        let asking = address(1, 1);
        let sibling = address(1, 2);

        router.register(
            sibling,
            &window(2),
            Some(CloseRequestHandler::new(|_| CloseResponse::KeepOpen)),
        );
        let reentrant = Arc::clone(&router);
        router.register(
            asking,
            &window(1),
            Some(CloseRequestHandler::new(move |_| {
                // Would deadlock on a non-reentrant lock held across the call.
                assert_eq!(reentrant.consult(sibling), CloseResponse::KeepOpen);
                reentrant.forget(sibling);
                CloseResponse::Close
            })),
        );

        assert_eq!(router.consult(asking), CloseResponse::Close);
        assert_eq!(
            router.consult(sibling),
            CloseResponse::Close,
            "the handler's own `forget` took effect"
        );
    }

    /// Teardown drops exactly the right entries: a realm uninstall must not
    /// take a sibling realm's windows with it.
    #[test]
    fn forget_and_forget_realm_drop_only_what_they_name() {
        let router = CloseRequestRouter::new();
        let keep_open = CloseRequestHandler::new(|_| CloseResponse::KeepOpen);
        let realm_one_first = address(1, 1);
        let realm_one_second = address(1, 2);
        let realm_two = address(2, 1);
        for (index, addr) in [realm_one_first, realm_one_second, realm_two]
            .into_iter()
            .enumerate()
        {
            router.register(addr, &window(index as u64), Some(keep_open.clone()));
        }

        router.forget(realm_one_first);
        assert_eq!(router.consult(realm_one_first), CloseResponse::Close);
        assert_eq!(router.consult(realm_one_second), CloseResponse::KeepOpen);
        assert_eq!(router.consult(realm_two), CloseResponse::KeepOpen);

        router.forget_realm(RealmId::new(1));
        assert_eq!(router.consult(realm_one_second), CloseResponse::Close);
        assert_eq!(
            router.consult(realm_two),
            CloseResponse::KeepOpen,
            "a realm uninstall must not drop a sibling realm's entries"
        );
    }

    /// The programmatic-close route reports why it could not deliver
    /// instead of failing silently -- an application that vetoed a close is
    /// relying on this call to finish it.
    #[test]
    fn request_close_reports_a_typed_reason_when_it_cannot_deliver() {
        let router = CloseRequestRouter::new();
        let unknown = address(1, 1);
        assert_eq!(
            router.request_close(unknown),
            Err(CloseRequestError::UnknownPresentation { address: unknown })
        );

        let registered = address(1, 2);
        let live = window(2);
        router.register(registered, &live, None);
        assert_eq!(router.request_close(registered), Ok(()));

        drop(live);
        assert_eq!(
            router.request_close(registered),
            Err(CloseRequestError::WindowGone {
                address: registered
            }),
            "the router holds the window weakly, so a destroyed window is reported, never \
             resurrected"
        );
    }

    /// Closing a window is an owner-thread operation.
    #[test]
    fn request_close_refuses_a_foreign_thread() {
        let router = Arc::new(CloseRequestRouter::new());
        let a = address(1, 1);
        let live = window(1);
        router.register(a, &live, None);

        let router_for_thread = Arc::clone(&router);
        let refused = std::thread::spawn(move || router_for_thread.request_close(a))
            .join()
            .expect("the requesting thread must not panic");
        assert_eq!(refused, Err(CloseRequestError::WrongThread));
    }
}
