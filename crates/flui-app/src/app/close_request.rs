//! Per-window close-request veto — the application's answer to "may this
//! window close?" (issue #558's cancel-or-defer criterion).
//!
//! # The seam this fills
//!
//! [`flui_platform::traits::PlatformWindow::on_should_close`] is a real
//! per-window veto: the winit, Win32 and AppKit backends all consult it
//! synchronously when the *user* asks a window to close, and a `false`
//! answer stops the close before anything else in the arm runs — no
//! `on_close`, no UI runtime teardown, no exit-policy consultation. Until this
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
//! `closeEvent` all leave that wait unbounded.
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
//!   `LifecycleContext`-acquired handle yet — the widget
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
//! - **Resolving a deferred close on winit completes on the owner's next
//!   turn, not inside the call** (issue #919, fixed). `WinitWindow::close`
//!   posts the close to that backend's owner lane and the owner runs the
//!   same teardown a compositor close takes — `on_close`, window-map
//!   removal, callback clear, exit-policy consult — so the process exits
//!   when that was the last window. What this module inherits from it: on
//!   winit the presentation is torn down AFTER
//!   [`request_presentation_close`](crate::request_presentation_close)
//!   returns, on the owner turn; the headless double still runs `on_close`
//!   synchronously inside `close()`, so a test that asserts right after the
//!   call pins the double, not the winit contract.

use std::sync::{Arc, Weak};
use std::thread::ThreadId;

use flui_foundation::PresentationAddress;
use flui_foundation::panic::payload_text;
use flui_platform::traits::PlatformWindow;
pub use flui_view::CloseReason;
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
    /// Which UI runtime incarnation and which presentation within it is being
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

    /// Why the close was requested: by the user, by the application's own
    /// code quitting, or by the session ending.
    ///
    /// Not yet carried: every request reads as [`CloseReason::User`].
    #[must_use]
    pub fn reason(&self) -> CloseReason {
        CloseReason::User
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
    ///
    /// On winit the close resolved that way completes on the owner's next
    /// turn rather than inside the call — see
    /// [`request_presentation_close`](crate::request_presentation_close)'s
    /// backend section.
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
pub struct CloseRequestHandler(
    #[cfg_attr(
        all(target_arch = "wasm32", not(test)),
        expect(
            dead_code,
            reason = "reached through the loop-exit teardown (desktop/android/iOS); wasm32 has \
                      no loop-exit teardown at all"
        )
    )]
    Arc<dyn Fn(&CloseRequest) -> CloseResponse + Send + Sync>,
);

impl CloseRequestHandler {
    /// Wrap a callback as a registerable handler.
    pub fn new(handler: impl Fn(&CloseRequest) -> CloseResponse + Send + Sync + 'static) -> Self {
        Self(Arc::new(handler))
    }

    #[cfg_attr(
        all(target_arch = "wasm32", not(test)),
        expect(
            dead_code,
            reason = "reached through the loop-exit teardown (desktop/android/iOS); wasm32 has \
                      no loop-exit teardown at all"
        )
    )]
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
    /// another thread. Distinct from [`Self::NoHostedRuntime`]: the
    /// presentation is registered and this process hosts it, just on a
    /// different thread than the caller.
    #[error("a close must be requested from the owner thread")]
    WrongThread,
    /// The calling thread hosts no FLUI runtime at all, so it has no
    /// presentations to close.
    ///
    /// The common cause is calling from a worker thread after finishing
    /// the work a [`CloseResponse::KeepOpen`] answer deferred. Marshal
    /// back to the thread that ran the handler and call from there.
    ///
    /// Reported instead of [`Self::UnknownPresentation`] because the two
    /// are genuinely different failures and a caller can act on the
    /// difference. It is thread-scoped rather than process-scoped on
    /// purpose: a process may host several event loops on several threads
    /// (`AppRuntime` is thread-local by design), so "the owner thread" is
    /// not a process-wide fact this call could consult.
    #[error(
        "this thread hosts no FLUI runtime; request the close from the thread that runs the \
         presentation's event loop"
    )]
    NoHostedRuntime,
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
    /// answer by reaching into UI runtime state anyway.
    owner_thread: ThreadId,
}

/// Loop-scoped registry of per-presentation close-request handlers.
///
/// Held by [`AppRuntime`](super::runtime::AppRuntime) as an `Arc` rather
/// than inline, so the `on_should_close` closure each window registers can
/// hold its own clone and answer **without** re-entering the `APP_RUNTIME`
/// thread-local at all. That matters: a close request can arrive while a
/// UI runtime is checked out for dispatch, and a router reached through the
/// UI runtime would then have to fail closed on a bookkeeping detail the
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
    /// Private, and no guard ever escapes this type's own methods:
    /// every caller gets a value out, never a lock.
    entries: Mutex<Vec<PresentationCloseEntry>>,
}

/// An unpublished handler and weak native window, prepared outside all guards.
pub(crate) struct PreparedCloseRequest(PresentationCloseEntry);

impl PreparedCloseRequest {
    #[cfg(any(
        test,
        all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        )
    ))]
    pub(crate) fn new(
        address: PresentationAddress,
        window: &Arc<dyn PlatformWindow>,
        handler: Option<CloseRequestHandler>,
    ) -> Self {
        Self(PresentationCloseEntry {
            address,
            window: Arc::downgrade(window),
            handler,
            owner_thread: std::thread::current().id(),
        })
    }
}

/// Capacity and identity reservation held through joint native publication.
pub(crate) struct ClosePublication<'a>(parking_lot::MutexGuard<'a, Vec<PresentationCloseEntry>>);

impl ClosePublication<'_> {
    pub(crate) fn publish(mut self, prepared: PreparedCloseRequest) {
        self.0.push(prepared.0);
    }
}

impl std::fmt::Debug for CloseRequestRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloseRequestRouter")
            .field("registered", &self.entries.lock().len())
            .finish()
    }
}

impl CloseRequestRouter {
    pub(crate) fn publication(
        &self,
        prepared: &PreparedCloseRequest,
    ) -> Option<ClosePublication<'_>> {
        let mut entries = self.entries.lock();
        if entries
            .iter()
            .any(|entry| entry.address == prepared.0.address)
        {
            return None;
        }
        entries.reserve(1);
        Some(ClosePublication(entries))
    }
    /// An empty router.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Remove the entry and return its handler for retirement outside host borrows.
    pub(crate) fn take(&self, address: PresentationAddress) -> Option<CloseRequestHandler> {
        let removed = {
            let mut entries = self.entries.lock();
            let index = entries.iter().position(|entry| entry.address == address)?;
            entries.remove(index)
        };
        removed.handler
    }

    /// Remove every registration for loop exit or final native-owner retirement.
    ///
    /// Not reachable by UI runtime-by-UI runtime removal: an explicit platform quit,
    /// or a bootstrap that fails after a window is wired but before its
    /// UI runtime is installed, both leave this loop with registrations no
    /// per-UI runtime teardown ever names. Since a second `Platform::run` on the
    /// same thread reuses this `AppRuntime`, those would otherwise be
    /// consulted by the NEXT loop's windows.
    pub(crate) fn take_all(&self) -> Vec<CloseRequestHandler> {
        let removed = std::mem::take(&mut *self.entries.lock());
        removed
            .into_iter()
            .filter_map(|entry| entry.handler)
            .collect()
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
    ///
    /// `reason` is why the close was asked for. Not yet carried to the
    /// handler: [`CloseRequest::reason`] reads [`CloseReason::User`].
    #[cfg_attr(
        all(target_arch = "wasm32", not(test)),
        expect(
            dead_code,
            reason = "reached through the loop-exit teardown (desktop/android/iOS); wasm32 has \
                      no loop-exit teardown at all"
        )
    )]
    pub(crate) fn consult(
        &self,
        address: PresentationAddress,
        reason: CloseReason,
    ) -> CloseResponse {
        let _ = reason;
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
        answered.unwrap_or_else(|payload| {
            tracing::error!(
                ?address,
                panic = payload_text(&*payload).unwrap_or("<non-string panic payload>"),
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
    ///
    /// **Completes on every backend whose `close()` performs a real close**
    /// (headless, winit, Win32, AppKit). On winit the teardown — `on_close`,
    /// window-map removal, exit-policy consult — runs on the owner's next
    /// turn, not inside this call (issue #919's fix); the headless double
    /// runs it synchronously. See
    /// [`request_presentation_close`](crate::request_presentation_close)
    /// for the per-backend statement.
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

    use flui_foundation::{PresentationId, UiRuntimeId};

    use super::*;
    use crate::app::window_test_support::TestWindow;

    fn address(ui_runtime: usize, presentation: usize) -> PresentationAddress {
        PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(ui_runtime),
            presentation_id: PresentationId::new(presentation),
        }
    }

    fn window(id: u64) -> Arc<dyn PlatformWindow> {
        Arc::new(TestWindow::new().with_id(id)) as Arc<dyn PlatformWindow>
    }

    fn register(
        router: &CloseRequestRouter,
        address: PresentationAddress,
        window: &Arc<dyn PlatformWindow>,
        handler: Option<CloseRequestHandler>,
    ) {
        let prepared = PreparedCloseRequest::new(address, window, handler);
        router
            .publication(&prepared)
            .expect("fresh test registration")
            .publish(prepared);
    }

    /// Two presentations, two answers, one router: the addressing that lets
    /// a document window refuse a close while the preferences window beside
    /// it closes normally. The sibling's handler must not even be consulted.
    fn one_presentations_veto_does_not_reach_its_sibling() {
        let router = CloseRequestRouter::new();
        let keeps_open = address(1, 1);
        let closes = address(1, 2);
        let other_ui_runtime = address(2, 1);

        let keeps_open_asked = Arc::new(AtomicUsize::new(0));
        let asked = Arc::clone(&keeps_open_asked);
        register(
            &router,
            keeps_open,
            &window(1),
            Some(CloseRequestHandler::new(move |_| {
                asked.fetch_add(1, Ordering::SeqCst);
                CloseResponse::KeepOpen
            })),
        );
        register(
            &router,
            closes,
            &window(2),
            Some(CloseRequestHandler::new(|_| CloseResponse::Close)),
        );

        assert_eq!(
            router.consult(closes, CloseReason::User),
            CloseResponse::Close
        );
        assert_eq!(
            keeps_open_asked.load(Ordering::SeqCst),
            0,
            "a sibling's close request must not consult this presentation's handler at all"
        );
        assert_eq!(
            router.consult(keeps_open, CloseReason::User),
            CloseResponse::KeepOpen
        );

        // A same-numbered presentation in a different ui_runtime is a different
        // window, not this one -- the reason the address is a pair.
        assert_eq!(
            router.consult(other_ui_runtime, CloseReason::User),
            CloseResponse::Close
        );
    }

    /// A panicking handler cannot be read as consent to discard unsaved
    /// work, so it vetoes -- the same conservative answer
    /// `WindowCallbacks::dispatch_should_close` gives a reentrant query --
    /// and stays registered, so a handler that stops panicking works again.
    fn a_panicking_handler_vetoes_and_stays_registered() {
        let router = CloseRequestRouter::new();
        let a = address(1, 1);
        let panics = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let panics_in_handler = Arc::clone(&panics);

        register(
            &router,
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

        // A quiet hook is exactly why the payload has to be carried into
        // the `tracing` field: with the default output suppressed, the
        // reported field is the ONLY evidence of what went wrong, so the
        // capture below is asserting the sole surviving diagnostic.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let (vetoed, log) =
            flui_testing::log_capture::capture(|| router.consult(a, CloseReason::User));
        std::panic::set_hook(previous_hook);
        assert_eq!(vetoed, CloseResponse::KeepOpen);

        assert!(
            !log.is_empty(),
            "vacuous-pass guard: the containment must have logged something"
        );
        let reported = log
            .records()
            .iter()
            .find_map(|record| record.field("panic"))
            .expect("the containment must report the panic payload, not discard it");
        assert!(
            reported.contains("deliberate handler panic under test"),
            "the reported payload must be the handler's own message, got {reported:?}"
        );

        panics.store(false, Ordering::SeqCst);
        assert_eq!(
            router.consult(a, CloseReason::User),
            CloseResponse::Close,
            "the handler must still be registered after containing its panic"
        );
    }

    /// The handler reads why the close was asked for: a close the
    /// application's own quit asks for reaches it as `Program`, not as the
    /// user's.
    #[test]
    #[ignore = "contract: a close request carries the reason it was asked for"]
    fn a_close_request_carries_its_reason() {
        let router = CloseRequestRouter::new();
        let a = address(1, 1);
        let reasons = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&reasons);
        register(
            &router,
            a,
            &window(1),
            Some(CloseRequestHandler::new(move |request| {
                seen.lock().push(request.reason());
                CloseResponse::KeepOpen
            })),
        );

        router.consult(a, CloseReason::Program);
        router.consult(a, CloseReason::User);

        assert_eq!(
            *reasons.lock(),
            [CloseReason::Program, CloseReason::User],
            "each request reaches the handler with the reason it was asked for"
        );
    }

    #[test]
    fn close_request_matrix() {
        crate::table_test::run_table(
            "close_request_matrix",
            &[
                (
                    "one_presentations_veto_does_not_reach_its_sibling",
                    one_presentations_veto_does_not_reach_its_sibling as fn(),
                ),
                (
                    "retiring_a_handler_preserves_its_reentrant_registration",
                    retiring_a_handler_preserves_its_reentrant_registration as fn(),
                ),
                (
                    "a_panicking_handler_vetoes_and_stays_registered",
                    a_panicking_handler_vetoes_and_stays_registered as fn(),
                ),
            ],
        );
    }

    struct RegisterOnDrop {
        router: Weak<CloseRequestRouter>,
        window: Arc<dyn PlatformWindow>,
        address: PresentationAddress,
    }

    impl Drop for RegisterOnDrop {
        fn drop(&mut self) {
            let router = self.router.upgrade().expect("router still owned");
            register(
                &router,
                self.address,
                &self.window,
                Some(CloseRequestHandler::new(|_| CloseResponse::KeepOpen)),
            );
        }
    }

    fn retiring_a_handler_preserves_its_reentrant_registration() {
        for whole_runtime in [false, true] {
            let router = Arc::new(CloseRequestRouter::new());
            let old = address(1, 1);
            let next = address(2, 2);
            let capture = RegisterOnDrop {
                router: Arc::downgrade(&router),
                window: window(2),
                address: next,
            };
            register(
                &router,
                old,
                &window(1),
                Some(CloseRequestHandler::new(move |_| {
                    let _ = &capture;
                    CloseResponse::Close
                })),
            );
            if whole_runtime {
                drop(router.take_all());
            } else {
                drop(router.take(old));
            }
            assert_eq!(
                router.consult(next, CloseReason::User),
                CloseResponse::KeepOpen,
                "a handler registered during retirement survives; whole_runtime={whole_runtime}"
            );
        }
    }
}
