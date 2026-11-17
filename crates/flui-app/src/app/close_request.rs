//! Per-window close-request veto — the application's answer to "may this
//! window close?".
//!
//! # The seam this fills
//!
//! [`flui_platform::WindowHandler::close_requested`] is a real
//! per-window veto: the Win32, AppKit, X11 and Wayland backends all consult it
//! synchronously when the *user* asks a window to close, and a
//! [`KeepOpen`](flui_platform::CloseDecision::KeepOpen) answer stops the
//! close before anything else in the arm runs — no `closed`, no UI runtime
//! teardown, no exit-policy consultation. Until this module existed every
//! `flui-app` window answered `Close`, so the mechanism shipped and no
//! application could reach it.
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
//! # A veto is recorded, never timed
//!
//! [`CloseResponse::KeepOpen`] keeps the window open and records the close
//! on the presentation's close guard, once per [`CloseReason`]: its widgets
//! find it as a `PendingClose` and resolve it, by carrying it out or by
//! staying open. The runtime arms no timer: the window stays open and fully
//! interactive, its close affordance still works, and the next request is
//! put to the handler afresh. A presentation without a guard records
//! nothing, and its veto is a complete answer to that one request.
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
//! The `deadline -> flush -> termination` leg is a different
//! question — how long teardown may take *after* a close is agreed — and
//! belongs with journaled state, not here.
//!
//! # Where a handler comes from, and where it is asked
//!
//! A handler is registered through
//! [`AppConfig::close_request_handler`](crate::AppConfig::close_request_handler),
//! the same embedder-facing route
//! [`FrameFailureHandler`](crate::FrameFailureHandler) and
//! [`ExitPolicy`](crate::ExitPolicy) take. A widget declares its own
//! unsaved work through the presentation's close guard, which this router
//! asks before the handler.
//!
//! No code path in the web or Android backend asks a window's handler
//! `close_requested`: neither system lets the application refuse a close,
//! so a handler registered there is never asked.
//!
//! # One thread
//!
//! The router, its entries and the handlers it calls live on the owner
//! thread that opened the windows (ADR-0323 §1): the router is held in an
//! `Rc` and every entry holds a `Weak` to its window, so neither can reach
//! another thread, and a handler may capture `Rc`, `Cell` and `RefCell`
//! state. No call checks its thread at run time; the types do.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_foundation::panic::payload_text;
use flui_platform::PlatformWindow;

use flui_platform::CommandRefusal;
pub use flui_view::CloseReason;

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
    reason: CloseReason,
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
    /// code quitting ([`AppHandle::request_quit`](crate::AppHandle::request_quit)),
    /// or by the session ending.
    #[must_use]
    pub fn reason(&self) -> CloseReason {
        self.reason
    }
}

/// The application's answer to a [`CloseRequest`].
///
/// `#[non_exhaustive]`: a future variant carrying a runtime-bounded flush
/// window (the `deadline -> flush -> termination` leg) would be an
/// additive change here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CloseResponse {
    /// Let the window close now. The close proceeds exactly as it would
    /// with no handler registered at all.
    Close,
    /// Keep the window open. The
    /// name here says what happens rather than what was refused.
    ///
    /// The close is recorded on the presentation's close guard as a
    /// `PendingClose` for the request's reason, once per reason, where its
    /// widgets find it (`CloseGuard::pending`) and resolve it: `discard_and_close`
    /// carries the close out, `stay_open` withdraws it. Nothing times out:
    /// the record waits for that decision. An application that answered
    /// this way in order to finish work may instead call
    /// [`request_presentation_close`](crate::request_presentation_close)
    /// when the work is done. The next close request is put to the handler
    /// afresh.
    ///
    /// A quit ([`CloseReason::Program`]) this answer refuses is deferred
    /// until the record is resolved; see
    /// [`AppHandle::request_quit`](crate::AppHandle::request_quit).
    KeepOpen,
}

/// An embedder-registered callback deciding whether one presentation's
/// window may close.
///
/// Register via
/// [`AppConfig::close_request_handler`](crate::AppConfig::close_request_handler);
/// each window opened with that config registers it for its own
/// presentation.
///
/// # Contract
///
/// Invoked **synchronously on the owner thread**, from inside the
/// platform's close-request handling; it never leaves that thread, so it
/// may capture `Rc`, `Cell` and `RefCell` state. `Fn`, not `FnMut`: the
/// router takes the handler out of its table before calling it, so a
/// handler may call back into this seam (closing another window, closing
/// its own, opening one) and finds the table free.
///
/// A handler that **panics** is answered with [`CloseResponse::KeepOpen`],
/// the same conservative veto a host's window delivery applies to a close
/// request raised inside a handler method: a panic is no trustworthy
/// answer, and closing on a wrong answer destroys data. The panic is
/// contained and logged at error level; the handler stays registered and a
/// later request reaches it normally.
#[derive(Clone)]
pub struct CloseRequestHandler(Rc<dyn Fn(&CloseRequest) -> CloseResponse>);

impl CloseRequestHandler {
    /// Wrap a callback as a registerable handler.
    pub fn new(handler: impl Fn(&CloseRequest) -> CloseResponse + 'static) -> Self {
        Self(Rc::new(handler))
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
#[derive(Debug, thiserror::Error, PartialEq)]
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
    /// The window does not close: its window system lets the application
    /// close none of its windows (Android's one window, whose `closes`
    /// capability is `Absent`).
    #[error("the window at {address:?} refused to close")]
    Refused {
        /// The address that was asked for.
        address: PresentationAddress,
        /// Why admitting [`WindowCommand::Close`](flui_platform::WindowCommand::Close)
        /// failed.
        #[source]
        error: flui_platform::AdmissionError,
    },
    /// The window took the close, and its window system failed it or the
    /// window was already gone.
    #[error("the window at {address:?} failed to close")]
    Failed {
        /// The address that was asked for.
        address: PresentationAddress,
        /// The window's answer to the admitted close.
        #[source]
        error: flui_platform::ApplyError,
    },
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
    /// alive past the teardown ordering a Wayland crash at quit
    /// established.
    window: std::rc::Weak<dyn PlatformWindow>,
    handler: Option<CloseRequestHandler>,
    /// The presentation's close guard: a close one of its holds refuses is
    /// recorded there and answered `KeepOpen` before the handler is asked.
    guard: Option<flui_view::__runtime::CloseGuardSource>,
}

/// Loop-scoped registry of per-presentation close-request handlers.
///
/// Held by the loop's native bindings in an `Rc`, so the close-request
/// route each window is opened with holds its own clone and answers
/// **without** re-entering the `APP_RUNTIME` thread-local at all. That
/// matters: a close request can arrive while a UI runtime is checked out
/// for dispatch, and a router reached through the UI runtime would then
/// have to fail closed on a bookkeeping detail the application never asked
/// about.
///
/// Deliberately not a second window authority:
/// [`WindowRegistry`](super::window_registry::WindowRegistry) owns the
/// native-key-to-[`PresentationAddress`] map and holds no window value,
/// while this keys on the address itself and carries the handler plus a
/// *weak* window. Nothing here names, accepts, or resolves a native window
/// key — the address is the only identity this module knows.
///
/// The table is borrowed only inside this type's methods, and no method
/// runs code it does not own while it holds the borrow: a handler, a
/// guard's signal, a window's close and the drop of a removed entry all
/// run after the borrow ends. A handler that re-enters the router finds the
/// table free.
#[derive(Default)]
pub(crate) struct CloseRequestRouter {
    entries: RefCell<Vec<PresentationCloseEntry>>,
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
        window: &std::rc::Rc<dyn PlatformWindow>,
        handler: Option<CloseRequestHandler>,
    ) -> Self {
        Self(PresentationCloseEntry {
            address,
            window: std::rc::Rc::downgrade(window),
            handler,
            guard: None,
        })
    }

    /// Answer the window's close requests with the presentation's close guard
    /// first: a close its holds refuse is recorded and kept open.
    #[cfg(any(
        test,
        all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        )
    ))]
    pub(crate) fn guard(mut self, guard: flui_view::__runtime::CloseGuardSource) -> Self {
        self.0.guard = Some(guard);
        self
    }
}

/// Capacity and identity reservation held through joint native publication.
pub(crate) struct ClosePublication<'a>(std::cell::RefMut<'a, Vec<PresentationCloseEntry>>);

impl ClosePublication<'_> {
    pub(crate) fn publish(mut self, prepared: PreparedCloseRequest) {
        self.0.push(prepared.0);
    }
}

impl std::fmt::Debug for CloseRequestRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Formatting may run while a publication holds the table: it then
        // says so instead of counting.
        let mut debug = f.debug_struct("CloseRequestRouter");
        match self.entries.try_borrow() {
            Ok(entries) => debug.field("registered", &entries.len()),
            Err(_) => debug.field("registered", &"<publishing>"),
        };
        debug.finish()
    }
}

impl CloseRequestRouter {
    pub(crate) fn publication(
        &self,
        prepared: &PreparedCloseRequest,
    ) -> Option<ClosePublication<'_>> {
        let mut entries = self.entries.borrow_mut();
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
            let mut entries = self.entries.borrow_mut();
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
        let removed = self.entries.take();
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
    /// (before `dispatch_close`, before the window leaves its tracking
    /// map); it consults the
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
    /// `reason` is why the close was asked for. The presentation's close
    /// guard is asked first, with it; a close the guard refuses is kept open
    /// without asking the handler. Otherwise the handler reads it as
    /// [`CloseRequest::reason`], and its [`CloseResponse::KeepOpen`] records
    /// the close on the guard. A handler that panics keeps the window open
    /// and records nothing: a panic is no answer to wait on.
    pub(crate) fn consult(
        &self,
        address: PresentationAddress,
        reason: CloseReason,
    ) -> CloseResponse {
        let Some((handler, guard)) = self.answerers(address) else {
            return CloseResponse::Close;
        };

        // The guard comes first: a close its holds refuse is recorded there
        // and carried out once the last hold is released, without asking the
        // handler again.
        if guard.as_ref().is_some_and(|guard| guard.refuses(reason)) {
            return CloseResponse::KeepOpen;
        }
        let Some(handler) = handler else {
            return CloseResponse::Close;
        };

        let request = CloseRequest { address, reason };
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a close handler's panic keeps the window open and is reported"
        )]
        let answered =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler.call(&request)));
        if let (Ok(CloseResponse::KeepOpen), Some(guard)) = (&answered, &guard) {
            guard.record(reason);
        }
        answered.unwrap_or_else(|payload| {
            tracing::error!(
                ?address,
                panic = payload_text(&*payload).unwrap_or("<non-string panic payload>"),
                "close-request handler panicked; vetoing the close (a panicking handler cannot \
                 be read as consent to discard unsaved work)"
            );
            flui_foundation::panic::discard_payload(payload);
            CloseResponse::KeepOpen
        })
    }

    /// Ask the application whether the session may end at the window at
    /// `address` (ADR-0260 §1): its handler first, with
    /// [`CloseReason::SessionEnd`], so a handler can still start the save or
    /// put up the decision; then its guard. A handler's
    /// [`CloseResponse::KeepOpen`] records the close on the guard; a hold for
    /// work never refuses a session end, a hold for a decision does. A handler
    /// that panics keeps the window open and records nothing.
    pub(crate) fn consult_session_end(&self, address: PresentationAddress) -> CloseResponse {
        let reason = CloseReason::SessionEnd;
        let Some((handler, guard)) = self.answerers(address) else {
            return CloseResponse::Close;
        };
        if let Some(handler) = handler {
            let request = CloseRequest { address, reason };
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: a close handler's panic keeps the window open and is reported"
            )]
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler.call(&request)))
            {
                Ok(CloseResponse::KeepOpen) => {
                    if let Some(guard) = &guard {
                        guard.record(reason);
                    }
                    return CloseResponse::KeepOpen;
                }
                Ok(_) => {}
                Err(payload) => {
                    tracing::error!(
                        ?address,
                        panic = payload_text(&*payload).unwrap_or("<non-string panic payload>"),
                        "close-request handler panicked on a session end; refusing it"
                    );
                    // Kept, not dropped: a payload whose destructor panics
                    // must not overturn the refusal on its way out.
                    flui_foundation::panic::discard_payload(payload);
                    return CloseResponse::KeepOpen;
                }
            }
        }
        if guard.is_some_and(|guard| guard.refuses(reason)) {
            CloseResponse::KeepOpen
        } else {
            CloseResponse::Close
        }
    }

    /// The handler and the guard registered at `address`, cloned out of the
    /// table so that asking either runs with the table free: application
    /// code may re-enter this router (closing a sibling window, closing its
    /// own, registering a handler). `None` for an unregistered address.
    fn answerers(
        &self,
        address: PresentationAddress,
    ) -> Option<(
        Option<CloseRequestHandler>,
        Option<flui_view::__runtime::CloseGuardSource>,
    )> {
        self.entries
            .borrow()
            .iter()
            .find(|entry| entry.address == address)
            .map(|entry| (entry.handler.clone(), entry.guard.clone()))
    }

    /// The guard registered at `address`, cloned out of the table: what it
    /// is asked signals the owner.
    fn guard_of(
        &self,
        address: PresentationAddress,
    ) -> Option<flui_view::__runtime::CloseGuardSource> {
        self.answerers(address).and_then(|(_, guard)| guard)
    }

    /// Every registered presentation, in registration order.
    pub(crate) fn addresses(&self) -> Vec<PresentationAddress> {
        self.entries
            .borrow()
            .iter()
            .map(|entry| entry.address)
            .collect()
    }

    /// Withdraw the close recorded for `reason` on the guard of the
    /// presentation at `address`, if one is: the platform cancelled it, as a
    /// cancelled log-off does. Closes recorded for other reasons stay.
    pub(crate) fn withdraw(&self, address: PresentationAddress, reason: CloseReason) {
        if let Some(guard) = self.guard_of(address) {
            guard.withdraw(reason);
        }
    }

    #[cfg(any(
        test,
        all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        )
    ))]
    /// Whether the guard of the presentation at `address` has a close for
    /// `reason` recorded and waiting. `false` for a presentation without a
    /// guard and for one no longer registered.
    pub(crate) fn is_recorded(&self, address: PresentationAddress, reason: CloseReason) -> bool {
        self.guard_of(address)
            .is_some_and(|guard| guard.is_recorded(reason))
    }

    /// Close the window at `address` programmatically, bypassing the
    /// handler.
    ///
    /// This is how a [`CloseResponse::KeepOpen`] answer is finished: the
    /// application saved its work and now wants the close it deferred.
    ///
    /// **Completes on every backend whose `WindowCommand::Close` performs a
    /// real close** (headless, Win32, AppKit, X11, Wayland); a window that
    /// refuses it (Android) answers [`CloseRequestError::Refused`]. The
    /// headless double runs `on_close` synchronously inside the command. See
    /// [`request_presentation_close`](crate::request_presentation_close)
    /// for the per-backend statement.
    /// Bypassing the handler is the point — every backend's own
    /// `WindowCommand::Close` bypasses its native close-request path for
    /// the same reason (AppKit's `-close` does not send
    /// `windowShouldClose:`; Win32's `DestroyWindow` does not send
    /// `WM_CLOSE`), because asking again would either loop forever or
    /// require the application to track "I am the one closing this".
    pub(crate) fn request_close(
        &self,
        address: PresentationAddress,
    ) -> Result<(), CloseRequestError> {
        let window = {
            let entries = self.entries.borrow();
            let entry = entries
                .iter()
                .find(|e| e.address == address)
                .ok_or(CloseRequestError::UnknownPresentation { address })?;
            entry
                .window
                .upgrade()
                .ok_or(CloseRequestError::WindowGone { address })?
        };
        // With the table free: closing fires the window's own `on_close`,
        // which re-enters flui-app (`close_this_window`) and may re-enter
        // this router.
        flui_platform::WindowCommand::Close
            .admit_and_apply(&*window)
            .map_err(|refusal| match refusal {
                CommandRefusal::Admission(error) => CloseRequestError::Refused { address, error },
                CommandRefusal::Apply(error) => CloseRequestError::Failed { address, error },
            })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use flui_foundation::{PresentationId, UiRuntimeId};

    use super::*;
    use crate::app::window_test_support::TestWindow;

    // What replaced the run-time thread checks: neither the router nor a
    // handler can reach another thread.
    static_assertions::assert_not_impl_any!(CloseRequestRouter: Send, Sync);
    static_assertions::assert_not_impl_any!(CloseRequestHandler: Send, Sync);

    fn address(ui_runtime: usize, presentation: usize) -> PresentationAddress {
        PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(ui_runtime),
            presentation_id: PresentationId::new(presentation),
        }
    }

    fn window(id: u64) -> Rc<dyn PlatformWindow> {
        Rc::new(
            TestWindow::new()
                .with_id(flui_platform::WindowId::try_from_raw(id).expect("a nonzero window id")),
        ) as Rc<dyn PlatformWindow>
    }

    fn register(
        router: &CloseRequestRouter,
        address: PresentationAddress,
        window: &Rc<dyn PlatformWindow>,
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

        let keeps_open_asked = Rc::new(Cell::new(0_usize));
        let asked = Rc::clone(&keeps_open_asked);
        register(
            &router,
            keeps_open,
            &window(1),
            Some(CloseRequestHandler::new(move |_| {
                asked.set(asked.get() + 1);
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
            keeps_open_asked.get(),
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
    /// a host's window delivery gives a close request raised inside a handler method --
    /// and stays registered, so a handler that stops panicking works again.
    fn a_panicking_handler_vetoes_and_stays_registered() {
        let router = CloseRequestRouter::new();
        let a = address(1, 1);
        let panics = Rc::new(Cell::new(true));
        let panics_in_handler = Rc::clone(&panics);

        register(
            &router,
            a,
            &window(1),
            Some(CloseRequestHandler::new(move |_| {
                assert!(
                    !panics_in_handler.get(),
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

        panics.set(false);
        assert_eq!(
            router.consult(a, CloseReason::User),
            CloseResponse::Close,
            "the handler must still be registered after containing its panic"
        );
    }

    /// The handler reads why the close was asked for: a close the
    /// application's own quit asks for reaches it as `Program`, not as the
    /// user's.
    fn a_close_request_carries_its_reason() {
        let router = CloseRequestRouter::new();
        let a = address(1, 1);
        let reasons = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&reasons);
        register(
            &router,
            a,
            &window(1),
            Some(CloseRequestHandler::new(move |request| {
                seen.borrow_mut().push(request.reason());
                CloseResponse::KeepOpen
            })),
        );

        router.consult(a, CloseReason::Program);
        router.consult(a, CloseReason::User);

        assert_eq!(
            *reasons.borrow(),
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
                (
                    "a_window_that_cannot_close_says_so",
                    a_window_that_cannot_close_says_so as fn(),
                ),
                (
                    "a_held_guard_keeps_the_close_before_the_handler_is_asked",
                    a_held_guard_keeps_the_close_before_the_handler_is_asked as fn(),
                ),
                (
                    "a_close_request_carries_its_reason",
                    a_close_request_carries_its_reason as fn(),
                ),
                (
                    "a_kept_open_close_is_recorded_for_its_reason",
                    a_kept_open_close_is_recorded_for_its_reason as fn(),
                ),
            ],
        );
    }

    /// A close the presentation's guard holds is kept open before the
    /// handler is asked, and recorded on the guard; once the hold is
    /// released, the handler answers again.
    fn a_held_guard_keeps_the_close_before_the_handler_is_asked() {
        let router = CloseRequestRouter::new();
        let at = address(1, 1);
        let asked = Rc::new(Cell::new(0_usize));
        let counted = Rc::clone(&asked);
        let source = flui_view::__runtime::CloseGuardSource::new(
            &[CloseReason::User],
            flui_view::__runtime::OwnerNotify::new(|_| Ok(())),
        );
        let prepared = PreparedCloseRequest::new(
            at,
            &window(1),
            Some(CloseRequestHandler::new(move |_| {
                counted.set(counted.get() + 1);
                CloseResponse::Close
            })),
        )
        .guard(source.clone());
        router
            .publication(&prepared)
            .expect("fresh test registration")
            .publish(prepared);
        let guard = source.guard();
        let hold = guard.hold();

        assert_eq!(
            router.consult(at, CloseReason::User),
            CloseResponse::KeepOpen
        );
        assert_eq!(asked.get(), 0, "the guard answers first");
        assert_eq!(
            guard.pending().map(|pending| pending.reason()),
            Some(CloseReason::User),
            "the refused close is recorded on the guard"
        );

        drop(hold);
        assert_eq!(
            source.settle().close,
            Some(CloseReason::User),
            "releasing the hold makes the recorded close due"
        );
        assert_eq!(router.consult(at, CloseReason::User), CloseResponse::Close);
        assert_eq!(asked.get(), 1);
    }

    /// A handler's `KeepOpen` records the close on the guard for the
    /// request's reason, once however often it is asked; a handler that
    /// panics records nothing.
    fn a_kept_open_close_is_recorded_for_its_reason() {
        let router = CloseRequestRouter::new();
        let at = address(1, 1);
        let panics = Rc::new(Cell::new(true));
        let panicking = Rc::clone(&panics);
        let source = flui_view::__runtime::CloseGuardSource::new(
            &[CloseReason::User, CloseReason::Program],
            flui_view::__runtime::OwnerNotify::new(|_| Ok(())),
        );
        let prepared = PreparedCloseRequest::new(
            at,
            &window(1),
            Some(CloseRequestHandler::new(move |_| {
                assert!(!panicking.get(), "deliberate handler panic under test");
                CloseResponse::KeepOpen
            })),
        )
        .guard(source.clone());
        router
            .publication(&prepared)
            .expect("fresh test registration")
            .publish(prepared);

        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panicked = router.consult(at, CloseReason::Program);
        std::panic::set_hook(previous_hook);
        assert_eq!(panicked, CloseResponse::KeepOpen);
        assert!(
            !router.is_recorded(at, CloseReason::Program),
            "a panic is no answer to wait on"
        );

        panics.set(false);
        for _ in 0..2 {
            assert_eq!(
                router.consult(at, CloseReason::Program),
                CloseResponse::KeepOpen
            );
        }
        assert!(router.is_recorded(at, CloseReason::Program));
        assert!(!router.is_recorded(at, CloseReason::User));
        let pending = source
            .guard()
            .pending()
            .expect("the kept-open close is recorded");
        assert_eq!(pending.reason(), CloseReason::Program);
        pending.stay_open().expect("the record is the current one");
        assert!(
            source.guard().pending().is_none(),
            "asking twice recorded one close"
        );
        assert_eq!(source.settle().declined, [CloseReason::Program]);
    }

    /// A window whose window system lets the application close none of its
    /// windows (Android's) refuses the programmatic close, and the caller
    /// hears why instead of an `Ok` for a window that stays.
    fn a_window_that_cannot_close_says_so() {
        let router = CloseRequestRouter::new();
        let at = address(1, 1);
        // The double honours no command, `Close` included.
        let window = window(1);
        register(&router, at, &window, None);
        let refused = router.request_close(at);
        assert!(
            matches!(
                refused,
                Err(CloseRequestError::Refused {
                    address,
                    error: flui_platform::AdmissionError::Unsupported(_),
                }) if address == at
            ),
            "{refused:?}"
        );
    }

    /// Registers a handler on the router when the handler capturing it is
    /// dropped. Weak: a handler is held by the router it registers on.
    struct RegisterOnDrop {
        router: std::rc::Weak<CloseRequestRouter>,
        window_id: u64,
        address: PresentationAddress,
    }

    impl Drop for RegisterOnDrop {
        fn drop(&mut self) {
            let router = self.router.upgrade().expect("router still owned");
            register(
                &router,
                self.address,
                &window(self.window_id),
                Some(CloseRequestHandler::new(|_| CloseResponse::KeepOpen)),
            );
        }
    }

    fn retiring_a_handler_preserves_its_reentrant_registration() {
        for whole_runtime in [false, true] {
            let router = Rc::new(CloseRequestRouter::new());
            let old = address(1, 1);
            let next = address(2, 2);
            let capture = RegisterOnDrop {
                router: Rc::downgrade(&router),
                window_id: 2,
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
