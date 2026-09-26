#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use std::sync::Arc;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use crate::app::close_request::CloseRequestHandler;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use super::host::{
    APP_RUNTIME, runtime_needs_redraw_handle, runtime_wake_callback, with_owner_platform,
};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use super::realm_dispatch::{
    PlatformToUi, RealmDispatcher, RealmTask, close_this_window, dispatch_platform_realm,
    install_presentation_alongside, install_realm_alongside,
};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use crate::app::AppWindowError;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use crate::app::runtime::WindowPolicy;

/// A native window-open failure, retained whole as the typed error's
/// `source` — the same shape `main_window.rs` gives the primary window's.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn native_error(source: flui_platform::OpenWindowError) -> AppWindowError {
    AppWindowError::Native {
        source: Arc::new(source),
    }
}

/// A realm/presentation install failure, retained whole as
/// [`AppWindowError::Mount`]'s `source`.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn mount_error(source: impl std::error::Error + Send + Sync + 'static) -> AppWindowError {
    AppWindowError::Mount {
        source: Arc::new(source),
    }
}
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use crate::app::{AppConfig, FrameFailureDetail};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use flui_view::View;

// ============================================================================
// Multi-window embedder seam (issue #555's `WindowPolicy`)
// ============================================================================

/// Opens an additional top-level window while the platform loop
/// `run_app`/`run_app_with_config` started is already running — the
/// embedder-facing seam issue #555's [`WindowPolicy`] governs which
/// realm/presentation topology the new window becomes. Must be called from
/// the owner thread while a loop is live (mirrors `bootstrap_desktop`'s own
/// `OwnerPlatform` access constraint: reachable only from inside, or after,
/// `on_ready` — e.g. from a `window.on_input`/`window.on_should_close`
/// callback the FIRST window already registered).
///
/// # [`WindowPolicy::SeparateRealms`]
///
/// Opens a fully independent second realm: its own `UiRealm`, its own
/// `GlobalKeyScope`, its own `UpdateScheduler` — installed via
/// `install_realm_alongside`, never `install_platform_realm`'s displacing
/// legacy path. `two_realms_via_separate_windows_policy_share_nothing` pins
/// the "share nothing but `SharedEngineServices`" guarantee this policy
/// claims. Input/close/should-close/focus/visibility/resize dispatch are
/// wired and addressed to this new realm exactly like the FIRST window's
/// own dispatch.
///
/// # [`WindowPolicy::SharedRealm`]
///
/// Installs a second PRESENTATION into the FIRST realm hosted on this
/// thread (via `install_presentation_alongside`) — real forest membership,
/// a real `WindowRegistry` mapping, real addressed
/// input/close/should-close/focus/visibility dispatch.
/// `one_realm_two_windows_policy_routes_by_presentation` pins that this
/// really is forest-membership routing, not a second realm in disguise.
///
/// # Completion and ownership
///
/// `Ready` installs inline. `Pending` reserves loop liveness before native
/// creation and is polled on window-independent owner turns. Worker completion
/// wakes that loop through its stamped `PlatformProxy`; no realm scheduler or
/// visible window is needed. Unwoken futures are not polled on unrelated turns.
/// An accepted separate-realm request survives closure of its originating realm.
/// Shared-realm requests capture the exact target identity at admission and fail
/// if that realm disappears; they never retarget another realm.
///
/// Polling and installation run outside runtime borrows. Installation waits for
/// any active realm checkout to finish. Quit and loop replacement invalidate
/// outstanding requests, including a batch currently being polled. Resolved but
/// uninstalled windows are closed and reservations released on every failure.
/// Asynchronous creation failures are traced. A failed physical wake releases
/// the reservation and marks the request failed; owner-local cleanup requires a
/// subsequent successful owner turn or shutdown. OS posting failure cannot
/// guarantee progress and is not retried in a busy loop.
///
/// # Rendering limitation
///
/// These windows have real registry membership and addressed native event
/// routing, but no mounted widget content or per-window renderer/frame callback.
/// Rendering secondary windows and exposing a resident root factory remain
/// separate work; this completion transport does not provide those features.
///
/// # Errors
///
/// Window creation ([`AppWindowError::Native`]) and (`SeparateRealms` only)
/// `UiRealm` construction ([`AppWindowError::Mount`]) surface as `Err`
/// exactly like `bootstrap_desktop`'s own first-window failures — this call
/// does not tear down or exit the loop on failure, unlike a first-window
/// bootstrap failure (which propagates out of `Platform::run` and ends the
/// loop): the caller decides what a failed secondary-window open means for
/// their app. A call from a thread with no running loop is
/// [`AppWindowError::NoOwnerLoop`]; one made while the application is
/// quitting is [`AppWindowError::AdmissionClosed`]; `SharedRealm` with no
/// realm hosted on this thread yet is [`AppWindowError::UnsupportedPolicy`].
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub fn open_secondary_window(
    config: AppConfig,
    policy: WindowPolicy,
) -> Result<(), AppWindowError> {
    open_secondary_window_impl(config, policy).map(|_| ())
}

/// Opens an additional top-level window with mounted widget content and its
/// own renderer — the content-bearing companion of
/// [`open_secondary_window`]'s bare shell.
///
/// Must be called from the owner thread while the platform loop is live.
/// The window becomes a real presentation target: it owns its own render
/// lane, drives its own frame pump, and accepts input routed to its own
/// realm — the same contract `run_app`'s primary window satisfies. The
/// `root` widget is mounted as the new window's root, sharing nothing
/// widget-visible with any sibling presentation on this host.
///
/// # Policy
///
/// `WindowPolicy::SeparateRealms` is the only policy that admits content:
/// each such window owns its own `UiRealm`, its own widget tree, its own
/// raster lane. `WindowPolicy::SharedRealm` currently refuses content at
/// admission with an `Err` — the realm's single-raster-lane contract would
/// have to be relaxed before a presentation inside one shared realm could
/// own its own renderer, and that relaxation is deliberately not smuggled
/// in through this API.
///
/// # Errors
///
/// The same as [`open_secondary_window`], plus the renderer-initialization
/// failures `install_desktop_window` can produce: GPU init
/// ([`AppWindowError::Renderer`]), mount ([`AppWindowError::Mount`]).
/// `SharedRealm` is refused as [`AppWindowError::UnsupportedPolicy`].
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub fn open_window<V>(
    config: AppConfig,
    policy: WindowPolicy,
    root: V,
) -> Result<(), AppWindowError>
where
    V: View + Clone + 'static,
{
    open_window_with_content_impl(config, policy, root).map(|_| ())
}

// Same cfg as `open_secondary_window`/`open_secondary_window_impl`/
// `finish_open_secondary_window` themselves (desktop-only) -- both statics
// exist only to serve that family, and `PENDING_SECONDARY_WINDOW_COMPLETIONS`
// names `WindowPolicy`, which is imported under this exact cfg expression
// (see this module's own `use crate::app::runtime::WindowPolicy` above). A
// narrower cfg here (e.g. `not(target_os = "ios")` alone) leaves this type
// annotation referencing an import that does not exist on android/wasm32,
// a hard compile error there, not merely dead code.
/// Configuration retained until a secondary window is installed.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
struct SecondaryWindowInstallConfig {
    loop_identity: Arc<()>,
    policy: WindowPolicy,
    shared_with: Option<flui_foundation::RealmId>,
    reservation: WindowReservation,
    close_request_handler: Option<CloseRequestHandler>,
    frame_failure_detail: FrameFailureDetail,
}

/// A resolved `Pending`-arm window waiting for installation.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
struct PendingCompletion {
    config: SecondaryWindowInstallConfig,
    window: Arc<dyn flui_platform::traits::HostWindow>,
    /// The install continuation. `None` runs the bare-shell
    /// [`finish_open_secondary_window`]; `Some` runs the closure, which
    /// captures everything the content-install path owns (root widget,
    /// worker reload handle, etc.).
    ///
    /// `FnOnce` because a real install consumes the closure's captured
    /// root exactly once. Boxed so `PendingCompletion` stays
    /// non-generic — every caller of the drain loop reads the same type,
    /// and the generic parameter lives only at the `open_window` call
    /// site that produced this record.
    install: Option<SecondaryWindowInstall>,
}

/// A window the open path resolved synchronously: its realm dispatcher and
/// the platform window it drives. `None` from the open functions means the
/// window was accepted but its creation is deferred to a later owner turn.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) type OpenedWindow = (
    RealmDispatcher,
    Arc<dyn flui_platform::traits::PlatformWindow>,
);

/// The boxed install continuation a [`PendingCompletion`] carries: given the
/// install configuration and the opened window, mount the content and return
/// the realm dispatcher and the window it now drives.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
type SecondaryWindowInstall = Box<
    dyn FnOnce(
        SecondaryWindowInstallConfig,
        Arc<dyn flui_platform::traits::HostWindow>,
    ) -> Result<
        (
            RealmDispatcher,
            Arc<dyn flui_platform::traits::PlatformWindow>,
        ),
        AppWindowError,
    >,
>;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
struct WindowReservation(Arc<PendingRequestState>);
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
enum PendingPhase {
    Waiting,
    Installing,
    Settled,
    Failed,
}
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
struct PendingRequestState {
    state: parking_lot::Mutex<(PendingPhase, bool)>,
    count: Arc<std::sync::atomic::AtomicUsize>,
    platform: flui_platform::SharedPlatform,
}
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
impl PendingRequestState {
    fn release(&self) {
        let previous = self.count.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        debug_assert!(
            previous > 0,
            "BUG: pending window reservation released twice"
        );
        self.platform.request_exit_policy_reevaluation();
    }
    fn fail(&self) -> bool {
        let failed = {
            let mut state = self.state.lock();
            if state.0 == PendingPhase::Waiting {
                state.0 = PendingPhase::Failed;
                true
            } else {
                false
            }
        };
        if failed {
            self.release();
        }
        failed
    }
    fn settle(&self) {
        let release = {
            let mut state = self.state.lock();
            let release = matches!(state.0, PendingPhase::Waiting | PendingPhase::Installing);
            state.0 = PendingPhase::Settled;
            release
        };
        if release {
            self.release();
        }
    }
    fn begin_install(&self) -> bool {
        let mut state = self.state.lock();
        if state.0 != PendingPhase::Waiting {
            return false;
        }
        state.0 = PendingPhase::Installing;
        true
    }
    fn mark_ready(&self) -> bool {
        let mut state = self.state.lock();
        if state.0 != PendingPhase::Waiting {
            return false;
        }
        state.1 = true;
        true
    }
    fn take_ready(&self) -> bool {
        let mut state = self.state.lock();
        state.0 == PendingPhase::Waiting && std::mem::take(&mut state.1)
    }
    fn failed(&self) -> bool {
        self.state.lock().0 == PendingPhase::Failed
    }
}
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
impl Drop for WindowReservation {
    fn drop(&mut self) {
        self.0.settle();
    }
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn reserve_window() -> Result<WindowReservation, AppWindowError> {
    let platform = with_owner_platform(flui_platform::OwnerPlatform::shared)
        .ok_or(AppWindowError::NoOwnerLoop)?;
    let count = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().pending_window_reservations));
    count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    Ok(WindowReservation(Arc::new(PendingRequestState {
        state: parking_lot::Mutex::new((PendingPhase::Waiting, true)),
        count,
        platform,
    })))
}
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
struct PendingOpen {
    request_id: Arc<()>,
    config: SecondaryWindowInstallConfig,
    pending: flui_platform::PendingWindow,
    waker: std::task::Waker,
}
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
struct PendingOpenWake {
    proxy: flui_platform::PlatformProxy,
    request: Arc<PendingRequestState>,
}
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
impl std::task::Wake for PendingOpenWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if !self.request.mark_ready() {
            return;
        }
        if let Err(error) = self.proxy.wake() {
            let report = matches!(
                &error,
                flui_platform::ProxySendError::WakeFailed { .. }
                    | flui_platform::ProxySendError::Unsupported { .. }
            );
            if self.request.fail() && report {
                tracing::error!(%error, "pending window wake failed; request cancelled, native cleanup awaits an owner turn or shutdown");
            }
        }
    }
}

thread_local! {
    #[cfg(all(not(target_os = "android"), not(target_os = "ios"), not(target_arch = "wasm32")))]
    static POLLING_PENDING_WINDOWS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Loop-owned requests. Each finite drain removes settled entries and
    /// returns only unresolved requests to this registry.
    #[cfg(all(not(target_os = "android"), not(target_os = "ios"), not(target_arch = "wasm32")))]
    static PENDING_SECONDARY_WINDOW_OPENS: std::cell::RefCell<Vec<PendingOpen>> =
        const { std::cell::RefCell::new(Vec::new()) };

    /// Resolved windows awaiting installation after realm checkout clears.
    /// Polling never installs inline; both policies share this completion path.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    static PENDING_SECONDARY_WINDOW_COMPLETIONS: std::cell::RefCell<Vec<PendingCompletion>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn discard_pending_open(mut request: PendingOpen) {
    if let Some(Ok(window)) = request.pending.try_take() {
        window.close();
    }
    // An unresolved handle is disclaimed; a delivered handle is explicitly
    // closed above, since dropping an Arc alone need not destroy a native window.
}

/// Cancel only uninstalled secondary windows; user/native cleanup runs outside TLS borrows.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn cancel_pending_secondary_windows() {
    let tokens =
        PENDING_SECONDARY_WINDOW_OPENS.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    let completions =
        PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    let mut panic = None;
    for token in tokens {
        let error =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| discard_pending_open(token)))
                .err();
        crate::app::lifecycle_state::preserve_first_lifecycle_panic(
            &mut panic,
            error,
            "pending open cancellation",
        );
    }
    for completion in completions {
        let error =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion.window.close()))
                .err();
        crate::app::lifecycle_state::preserve_first_lifecycle_panic(
            &mut panic,
            error,
            "uninstalled window close",
        );
    }
    if let Some(payload) = panic {
        std::panic::resume_unwind(payload);
    }
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn secondary_install_admitted(identity: &Arc<()>) -> bool {
    APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        state.quit_notification == crate::app::runtime::QuitNotification::Active
            && Arc::ptr_eq(identity, &state.loop_identity)
    })
}

/// Applies every `open_secondary_window` `Pending`-arm completion queued by
/// [`spawn_pending_secondary_window_completion`]'s own future, in request
/// order. Call only from a point where this thread's dispatch/hot-restart-
/// visit checkout state is already clear (`dispatched_realm_id` and
/// `iterating_all_realms` both settled back to their idle values) — the same
/// discipline [`crate::app::runtime::AppRuntime::drain_pending_realm_mutations`]
/// requires of its own callers, and for the identical reason:
/// `finish_open_secondary_window` calls `install_presentation_alongside`/
/// `install_realm_alongside`, both of which need to actually apply rather
/// than defer (`SharedRealm`'s `install_presentation_alongside` has no
/// defer-to-idle queue of its own, so calling this before the checkout
/// clears would just reproduce the same `DispatchInFlight` refusal one level
/// up).
///
/// A completion's own failure is traced (`tracing::error!`), never
/// propagated: by the time this runs there is no synchronous caller left for
/// either policy to return an `Err` to.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn drain_pending_secondary_window_completions() {
    use std::future::Future;
    if APP_RUNTIME.with(|slot| {
        slot.borrow().quit_notification != crate::app::runtime::QuitNotification::Active
    }) {
        cancel_pending_secondary_windows();
        return;
    }
    if APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        state.dispatched_realm_id.is_some() || state.iterating_all_realms
    }) || POLLING_PENDING_WINDOWS.with(|active| active.replace(true))
    {
        return;
    }
    struct PollGuard;
    impl Drop for PollGuard {
        fn drop(&mut self) {
            POLLING_PENDING_WINDOWS.with(|active| active.set(false));
        }
    }
    let _guard = PollGuard;
    let pending =
        PENDING_SECONDARY_WINDOW_OPENS.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    let mut first_panic = None;
    for mut request in pending {
        if !secondary_install_admitted(&request.config.loop_identity)
            || request.config.reservation.0.failed()
        {
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                discard_pending_open(request);
            }))
            .err();
            crate::app::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first_panic,
                error,
                "failed pending window cleanup",
            );
            continue;
        }
        if !request.config.reservation.0.take_ready() {
            PENDING_SECONDARY_WINDOW_OPENS.with(|queue| queue.borrow_mut().push(request));
            continue;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            std::pin::Pin::new(&mut request.pending)
                .poll(&mut std::task::Context::from_waker(&request.waker))
        }));
        match result {
            Ok(std::task::Poll::Pending)
                if secondary_install_admitted(&request.config.loop_identity)
                    && !request.config.reservation.0.failed() =>
            {
                PENDING_SECONDARY_WINDOW_OPENS.with(|queue| queue.borrow_mut().push(request));
            }
            Ok(std::task::Poll::Pending) => {}
            Ok(std::task::Poll::Ready(Ok(window))) => {
                PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| {
                    queue.borrow_mut().push(PendingCompletion {
                        config: request.config,
                        window,
                        install: None,
                    });
                });
            }
            Ok(std::task::Poll::Ready(Err(error))) => {
                tracing::error!(%error, "pending secondary window creation failed");
            }
            Err(payload) => crate::app::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first_panic,
                Some(payload),
                "pending window poll",
            ),
        }
    }
    let completed =
        PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    for completion in completed {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // `Some` — a content-bearing secondary window from
            // `open_window`'s deferred-install path: the closure owns the
            // root widget and the worker reload handle. `None` — the bare
            // shell that predates content support: no root, no renderer,
            // just the realm install and the platform callbacks.
            let outcome = match completion.install {
                Some(install) => install(completion.config, completion.window),
                None => finish_open_secondary_window(completion.config, completion.window),
            };
            if let Err(error) = outcome {
                tracing::error!(%error, "installing resolved secondary window failed");
            }
        }));
        crate::app::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first_panic,
            result.err(),
            "pending window installation",
        );
    }
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
}

/// [`open_secondary_window`]'s real body, additionally returning the exact
/// native window it opened and wired — the public function discards it
/// (embedders address the window only through dispatched events, never a
/// held handle); this module's own tests need it to drive a REAL close
/// (`window.close()`) instead of reaching for the internal
/// `close_this_window`/`uninstall_platform_realm` primitives directly, which
/// would prove the primitives work without proving THIS function's own
/// `on_close` wiring calls them.
///
/// Returns `Ok(None)` for the `Pending` arm (see
/// [`spawn_pending_secondary_window_completion`]): `OwnerPlatform::
/// open_window` resolves synchronously only inside `on_ready`, or on a
/// backend with no owner lane at all (headless); the real winit backend
/// defers any call after `on_ready` — exactly the calling convention this
/// function documents as its own intended use (from a callback the FIRST
/// window already registered) — so a caller reachable ONLY through that
/// convention must handle `None` as "accepted, completing asynchronously",
/// not assume `Some` unconditionally.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn open_secondary_window_impl(
    config: AppConfig,
    policy: WindowPolicy,
) -> Result<Option<OpenedWindow>, AppWindowError> {
    use flui_platform::{WindowOpen, WindowOptions};
    let loop_identity = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity));
    if !secondary_install_admitted(&loop_identity) {
        return Err(AppWindowError::AdmissionClosed);
    }

    let shared_with = if policy == WindowPolicy::SharedRealm {
        Some(
            APP_RUNTIME
                .with(|slot| slot.borrow().realms.iter().next().map(|(id, _)| *id))
                .ok_or(AppWindowError::UnsupportedPolicy {
                    reason: "SharedRealm requires a realm already hosted on this thread to \
                             share with",
                })?,
        )
    } else {
        None
    };
    let reservation = reserve_window()?;
    // Reveal at open (`From<&AppConfig>`'s default): a bare secondary
    // window has no renderer and no frame loop, so nothing here would
    // ever call `reveal_after_first_frame` — see
    // `desktop::rendered_window_options` for the path that defers.
    let options: WindowOptions = (&config).into();
    let open = with_owner_platform(|owner| owner.open_window(options))
        .ok_or(AppWindowError::NoOwnerLoop)?
        .map_err(native_error)?;

    let install_config = SecondaryWindowInstallConfig {
        loop_identity,
        policy,
        shared_with,
        reservation,
        close_request_handler: config.close_request_handler.clone(),
        frame_failure_detail: config.frame_failure_detail,
    };

    match open {
        WindowOpen::Ready(window) => finish_open_secondary_window(install_config, window).map(Some),
        WindowOpen::Pending(pending) => {
            spawn_pending_secondary_window_completion(install_config, pending)?;
            Ok(None)
        }
    }
}

/// The content-bearing companion of [`open_secondary_window_impl`]:
/// identical admission and reservation protocol, but the resolved window
/// is routed to a full `install_desktop_window`-style mount instead of
/// the bare-shell `finish_open_secondary_window` — the realm owns a widget
/// tree, a GPU raster lane, and a frame pump.
///
/// `SharedRealm` is refused at admission, before any window creation
/// work runs: the realm's raster lane and content renderer are per-realm,
/// not per-presentation today, so a shared realm presenting N windows
/// with content would need a lane-per-presentation redesign before this
/// arm could be honoured.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn open_window_with_content_impl<V>(
    config: AppConfig,
    policy: WindowPolicy,
    root: V,
) -> Result<Option<OpenedWindow>, AppWindowError>
where
    V: View + Clone + 'static,
{
    use flui_platform::{WindowOpen, WindowOptions};

    if policy == WindowPolicy::SharedRealm {
        return Err(AppWindowError::UnsupportedPolicy {
            reason: "open_window with content requires WindowPolicy::SeparateRealms; SharedRealm \
                     would imply a lane-per-presentation raster contract the realm does not \
                     provide today",
        });
    }

    let loop_identity = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity));
    if !secondary_install_admitted(&loop_identity) {
        return Err(AppWindowError::AdmissionClosed);
    }

    let reservation = reserve_window()?;
    // Deferred reveal: the content install below goes through
    // `install_desktop_window`, which performs the reveal.
    let options: WindowOptions = super::desktop::rendered_window_options(&config);
    let open = with_owner_platform(|owner| owner.open_window(options))
        .ok_or(AppWindowError::NoOwnerLoop)?
        .map_err(native_error)?;

    match open {
        WindowOpen::Ready(window) => {
            // The install consumes nothing from this function's stack, so
            // the closure's captured values are the whole state: the
            // `RealmSlot` install is deferred past whatever
            // `dispatch_platform_realm` currently owns the realm checkout
            // (this function may itself run inside one — e.g. a widget
            // `on_pressed`), and the deferred completion drains after the
            // dispatch loop on the realm's own turn. Routing through
            // `PENDING_SECONDARY_WINDOW_COMPLETIONS` is what lets
            // `install_desktop_window`'s internals — which run
            // `install_realm_alongside` synchronously against the
            // registry — observe an idle checkout rather than one held by
            // the caller's own dispatch.
            let install_config = SecondaryWindowInstallConfig {
                loop_identity: Arc::clone(&loop_identity),
                policy,
                shared_with: None,
                reservation,
                close_request_handler: config.close_request_handler.clone(),
                frame_failure_detail: config.frame_failure_detail,
            };
            let reload = crate::app::hot_reload::WorkerReload::from_config(&config);
            let host = APP_RUNTIME.with(|slot| slot.borrow().main_host_lifecycle);
            let config_for_install = config.clone();
            PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| {
                queue.borrow_mut().push(PendingCompletion {
                    config: install_config,
                    window: Arc::clone(&window),
                    install: Some(Box::new(move |install_slot, window| {
                        let _ = install_slot; // fields already captured separately
                        super::desktop::install_desktop_window(
                            root,
                            &config_for_install,
                            reload,
                            Arc::clone(&window),
                            host,
                        )
                        .map(|rendered| {
                            (
                                RealmDispatcher {
                                    owner_thread: std::thread::current().id(),
                                    address: rendered.address,
                                },
                                window as Arc<dyn flui_platform::traits::PlatformWindow>,
                            )
                        })
                    })),
                });
            });
            // Wake the loop so the next owner turn drains the completion.
            // The reservation itself does not wake anything: the loop's
            // drain runs on its regular wake points (see
            // `drain_pending_secondary_window_completions`'s own callers),
            // but an idle loop with no other work would never reach one.
            // Poking the runtime's wake handle is the ordinary "I just
            // queued background work" signal and matches what
            // `UiRealm::request_redraw` already does from
            // `request_redraw_for`.
            with_owner_platform(|owner| owner.proxy().wake());
            Ok(None)
        }
        WindowOpen::Pending(_) => Err(AppWindowError::UnsupportedPolicy {
            reason: "open_window with content does not yet support deferred window creation \
                     (the `Pending` arm); call on the owner thread, where windows resolve Ready",
        }),
    }
}

/// Registers a loop-owned pending request and requests its first owner poll.
/// No realm or frame is required. Synchronous posting failure returns an error
/// and removes the request; later errors are traced because the accepting caller
/// has already returned. A future resident controller may expose completion to
/// callers, but this existing public entry point remains fire-and-report.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn spawn_pending_secondary_window_completion(
    config: SecondaryWindowInstallConfig,
    pending: flui_platform::PendingWindow,
) -> Result<(), AppWindowError> {
    let proxy = with_owner_platform(flui_platform::OwnerPlatform::proxy)
        .ok_or(AppWindowError::NoOwnerLoop)?;
    let waker = std::task::Waker::from(Arc::new(PendingOpenWake {
        proxy: proxy.clone(),
        request: Arc::clone(&config.reservation.0),
    }));
    let request_id = Arc::new(());
    PENDING_SECONDARY_WINDOW_OPENS.with(|queue| {
        queue.borrow_mut().push(PendingOpen {
            request_id: Arc::clone(&request_id),
            config,
            pending,
            waker,
        });
    });
    if let Err(error) = proxy.wake() {
        let rejected = PENDING_SECONDARY_WINDOW_OPENS.with(|queue| {
            let mut queue = queue.borrow_mut();
            queue
                .iter()
                .position(|request| Arc::ptr_eq(&request.request_id, &request_id))
                .map(|position| queue.remove(position))
        });
        drop(rejected);
        return Err(AppWindowError::Native {
            source: Arc::new(error),
        });
    }
    Ok(())
}

/// [`open_secondary_window_impl`]'s shared completion path — installs the
/// realm/presentation topology [`WindowPolicy`] governs and wires every
/// per-window callback, for a `window` that already exists (whether
/// obtained synchronously, `WindowOpen::Ready`, or asynchronously through
/// [`spawn_pending_secondary_window_completion`]'s own `Pending` resolution)
/// — so both arms install and wire a window identically, and neither
/// duplicates the other's bookkeeping.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn finish_open_secondary_window(
    config: SecondaryWindowInstallConfig,
    host: Arc<dyn flui_platform::traits::HostWindow>,
) -> Result<
    (
        RealmDispatcher,
        Arc<dyn flui_platform::traits::PlatformWindow>,
    ),
    AppWindowError,
> {
    let window: Arc<dyn flui_platform::traits::PlatformWindow> = Arc::clone(&host) as _;
    struct Uninstalled(Option<Arc<dyn flui_platform::PlatformWindow>>);
    impl Drop for Uninstalled {
        fn drop(&mut self) {
            if let Some(window) = self.0.take()
                && let Err(payload) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| window.close()))
            {
                std::mem::forget(payload);
            }
        }
    }
    let mut uninstalled = Uninstalled(Some(Arc::clone(&window)));
    use flui_platform::traits::{DispatchEventResult, PlatformInput};
    if !secondary_install_admitted(&config.loop_identity) || !config.reservation.0.begin_install() {
        return Err(AppWindowError::AdmissionClosed);
    }

    let SecondaryWindowInstallConfig {
        loop_identity: _,
        policy,
        shared_with,
        reservation: _reservation,
        close_request_handler,
        frame_failure_detail,
    } = config;

    let realm_dispatch = match policy {
        WindowPolicy::SharedRealm => {
            // Failure detail is realm-scoped. A secondary presentation
            // inherits the already-hosted realm's policy; its window config
            // must not mutate that policy for existing siblings.
            let shared_with = APP_RUNTIME
                .with(|slot| {
                    let state = slot.borrow();
                    let realm_id = shared_with?;
                    let realm = state.realms.get(&realm_id)?.realm.as_ref()?;
                    Some(RealmDispatcher {
                        owner_thread: state.owner_thread?,
                        address: flui_foundation::PresentationAddress {
                            realm_id,
                            presentation_id: realm.presentation_id(),
                        },
                    })
                })
                .ok_or(AppWindowError::UnsupportedPolicy {
                    reason: "WindowPolicy::SharedRealm requires an already-hosted realm to share \
                             with; none is installed on this thread",
                })?;
            install_presentation_alongside(
                shared_with,
                super::presentation_window(Arc::clone(&host)),
            )
            .map_err(mount_error)?
        }
        WindowPolicy::SeparateRealms => {
            let scale_factor = window.scale_factor() as f32;
            let wake = runtime_wake_callback();
            let ui_realm = crate::app::ui_realm::UiRealm::new(
                Arc::clone(&wake),
                super::presentation_window(Arc::clone(&host)),
                scale_factor,
                runtime_needs_redraw_handle(),
            )
            .map_err(mount_error)?;
            ui_realm.set_frame_failure_detail(frame_failure_detail);
            // No frame-failure handler is installed here. Under
            // `open_secondary_window`'s current contract this realm has no
            // root widget or renderer, so secondary handler ownership is
            // blocked on the documented secondary-window rendering contract.
            install_realm_alongside(ui_realm, &window).map_err(mount_error)?
        }
    };

    tracing::warn!(
        ?policy,
        ?realm_dispatch,
        "open_secondary_window: installed a live, addressed window with no widget content and no \
         renderer -- see this function's own doc for the two named, scoped-out gaps"
    );

    // Wire this presentation into the close-request seam (issue #558)
    // through the same single implementation `run_desktop`'s primary window
    // uses. Unlike the frame-failure handler noted above, this one IS
    // threaded down from the caller's `AppConfig` -- a secondary window
    // that could not refuse its own close would leave the veto reachable
    // for exactly one window per process, and a window's answer is
    // addressed to its OWN presentation, so it can never affect a
    // sibling's.
    super::install_close_request_wiring(realm_dispatch.address, &window, close_request_handler);

    window.on_input(Box::new(move |input: PlatformInput| {
        let _ =
            dispatch_platform_realm(realm_dispatch, RealmTask::Event(PlatformToUi::Input(input)));
        DispatchEventResult::resolved(false, true)
    }));

    window.on_resize(Box::new(move |size, scale_factor| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::Resized { size, scale_factor }),
        );
    }));

    // Window close -> close THIS window's own presentation, exactly like
    // `run_desktop`'s primary window (see `close_this_window`'s own doc):
    // `SeparateRealms` reduces to a full uninstall of this new, independent
    // realm (its sole presentation); `SharedRealm` removes just this
    // presentation from the shared realm's forest while the primary (and
    // any other sibling) survives untouched -- never a blind
    // `uninstall_platform_realm`, which would tear down the WHOLE shared
    // realm out from under a still-open sibling window.
    //
    // No `on_quit` registration here — that is a single platform-level
    // callback slot the FIRST window's bootstrap already owns
    // (`Platform::on_quit`/`SharedPlatform::on_quit` replace, never stack);
    // registering a second one here would silently steal the first window's
    // Detached-lifecycle notification on process quit instead of adding to
    // it. The loop-owned quit callback visits every installed realm once,
    // including this window's realm, after any active dispatch restores it.
    window.on_close(Box::new(move || {
        tracing::info!(?realm_dispatch, "Secondary window closed");
        close_this_window(realm_dispatch);
    }));
    // No `on_should_close` registration here: `install_close_request_wiring`
    // above installed it, together with the router entry it consults.
    window.on_active_status_change(Box::new(move |focused| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowFocus(focused)),
        );
    }));
    window.on_execution_state_change(Box::new(move |state| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowExecution(state)),
        );
    }));
    window.on_visibility_status_change(Box::new(move |visible| {
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmTask::Event(PlatformToUi::WindowVisibility(visible)),
        );
    }));
    let execution = window.execution_state();
    let focused = window.is_focused();
    let visible = window.is_visible();
    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmTask::Frame(Box::new(move |realm| {
            realm.synchronize_window_snapshot(
                realm_dispatch.address.presentation_id,
                execution,
                focused,
                visible,
            );
        })),
    );

    let _ = dispatch_platform_realm(
        realm_dispatch,
        RealmTask::Event(PlatformToUi::SynchronizeLifecycle),
    );

    uninstalled.0 = None;
    Ok((realm_dispatch, window))
}

#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod quit_notification_tests {
    use super::super::host::{
        OwnerHostClearGuard, install_owner_platform, install_platform_quit_hook,
    };
    use super::super::realm_dispatch::{install_platform_realm, teardown_platform_realm};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn pending_secondary_open_is_accepted_without_a_driver_realm() {
        let _clear = OwnerHostClearGuard::arm();
        let platform = flui_platform::HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let owner_turns = platform.owner_turns();
        let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
        platform
            .run(Box::new(|owner| {
                install_owner_platform(owner).expect("install owner wake transport");
                assert!(APP_RUNTIME.with(|slot| slot.borrow().realms.iter().next().is_none()));
                assert!(
                    open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
                        .expect("a loop-owned pending open must not require a driver realm")
                        .is_none()
                );
                Ok(())
            }))
            .expect("headless run");
        owner_turns.drive(); // Initial poll installs a real PendingWindow waker.
        let window =
            std::thread::spawn(move || deferred.resolve_next().expect("resolve accepted request"))
                .join()
                .expect("worker completion");
        assert!(APP_RUNTIME.with(|slot| slot.borrow().realms.iter().next().is_none()));
        owner_turns.drive(); // The worker waker parked a window-independent owner turn.
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            1
        );
        assert_eq!(
            APP_RUNTIME.with(|slot| slot
                .borrow()
                .pending_window_reservations
                .load(Ordering::Acquire)),
            0
        );
        window.close();
        teardown_platform_realm();
    }

    #[test]
    fn failed_worker_post_releases_reservation_and_closes_delivered_window_on_next_turn() {
        let _clear = OwnerHostClearGuard::arm();
        let platform = flui_platform::HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let turns = platform.owner_turns();
        let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
        platform
            .run(Box::new(|owner| {
                install_owner_platform(owner).expect("install");
                open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
                    .expect("accept");
                Ok(())
            }))
            .expect("run");
        turns.drive();
        turns.fail_next_wake();
        let window = std::thread::spawn(move || deferred.resolve_next().expect("resolve"))
            .join()
            .expect("worker");
        let closed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&closed);
        window.on_close(Box::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(
            APP_RUNTIME.with(|slot| slot
                .borrow()
                .pending_window_reservations
                .load(Ordering::Acquire)),
            0
        );
        assert!(
            PENDING_SECONDARY_WINDOW_OPENS.with(|queue| queue.borrow()[0]
                .config
                .reservation
                .0
                .failed())
        );
        with_owner_platform(|owner| owner.proxy().wake())
            .expect("owner")
            .expect("later successful wake");
        turns.drive();
        assert_eq!(
            closed.load(Ordering::SeqCst),
            1,
            "delivered uninstalled window explicitly closes"
        );
        assert_eq!(
            APP_RUNTIME.with(|slot| slot.borrow().realms.iter().count()),
            0
        );
        assert!(PENDING_SECONDARY_WINDOW_OPENS.with(|queue| queue.borrow().is_empty()));
    }

    #[test]
    fn explicit_quit_closes_delivered_but_unpolled_pending_window() {
        let _clear = OwnerHostClearGuard::arm();
        let platform = flui_platform::HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let turns = platform.owner_turns();
        let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
        platform
            .run(Box::new(|owner| {
                install_owner_platform(owner).expect("install");
                install_platform_quit_hook();
                open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
                    .expect("accept");
                Ok(())
            }))
            .expect("run");
        turns.drive();
        let window = std::thread::spawn(move || deferred.resolve_next().expect("resolve"))
            .join()
            .expect("worker");
        let closed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&closed);
        window.on_close(Box::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        let proxy = with_owner_platform(flui_platform::OwnerPlatform::proxy).expect("owner");
        proxy.request_quit().expect("quit");
        turns.drive();
        assert_eq!(closed.load(Ordering::SeqCst), 1);
        assert_eq!(
            APP_RUNTIME.with(|slot| slot
                .borrow()
                .pending_window_reservations
                .load(Ordering::Acquire)),
            0
        );
        assert!(PENDING_SECONDARY_WINDOW_OPENS.with(|queue| queue.borrow().is_empty()));
    }

    #[test]
    fn shared_pending_target_loss_does_not_retarget_surviving_realm() {
        let _clear = OwnerHostClearGuard::arm();
        let platform = flui_platform::HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let turns = platform.owner_turns();
        let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
        platform
            .run(Box::new(|owner| {
                install_owner_platform(owner).expect("install");
                let window_a = crate::app::window_test_support::headless_test_window();
                let target =
                    install_platform_realm(crate::app::ui_realm::UiRealm::for_test(), &window_a);
                open_secondary_window_impl(AppConfig::default(), WindowPolicy::SharedRealm)
                    .expect("accept");
                let window_b = crate::app::window_test_support::headless_test_window();
                install_realm_alongside(crate::app::ui_realm::UiRealm::for_test(), &window_b)
                    .expect("other realm");
                close_this_window(target);
                Ok(())
            }))
            .expect("run");
        turns.drive();
        let window = std::thread::spawn(move || deferred.resolve_next().expect("resolve"))
            .join()
            .expect("worker");
        let closed = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&closed);
        window.on_close(Box::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        turns.drive();
        assert_eq!(closed.load(Ordering::SeqCst), 1);
        APP_RUNTIME.with(|slot| {
            let runtime = slot.borrow();
            assert_eq!(runtime.realms.iter().count(), 1);
            let (_, slot) = runtime.realms.iter().next().expect("survivor");
            assert_eq!(
                slot.realm.as_ref().expect("restored").presentation_count(),
                1
            );
            assert_eq!(
                runtime.pending_window_reservations.load(Ordering::Acquire),
                0
            );
        });
        teardown_platform_realm();
    }

    #[test]
    fn pending_request_readiness_and_failure_install_race_release_once() {
        let platform: Box<dyn flui_platform::Platform> =
            Box::new(flui_platform::HeadlessPlatform::new());
        platform
            .run(Box::new(|owner| {
                for _ in 0..64 {
                    let count = Arc::new(AtomicUsize::new(1));
                    let request = Arc::new(PendingRequestState {
                        state: parking_lot::Mutex::new((PendingPhase::Waiting, true)),
                        count: Arc::clone(&count),
                        platform: owner.shared(),
                    });
                    assert!(request.take_ready());
                    assert!(!request.take_ready(), "unrelated turns cannot repoll");
                    assert!(request.mark_ready());
                    assert!(request.take_ready(), "wake during poll is retained");
                    let barrier = Arc::new(std::sync::Barrier::new(2));
                    let worker_request = Arc::clone(&request);
                    let worker_barrier = Arc::clone(&barrier);
                    let worker = std::thread::spawn(move || {
                        worker_barrier.wait();
                        worker_request.fail()
                    });
                    barrier.wait();
                    let installed = request.begin_install();
                    let failed = worker.join().expect("failure worker");
                    assert_ne!(installed, failed, "one linearized winner");
                    assert_eq!(count.load(Ordering::Acquire), usize::from(installed));
                    assert!(!request.mark_ready());
                    request.settle();
                    request.settle();
                    assert!(!request.fail());
                    assert_eq!(count.load(Ordering::Acquire), 0);
                }
                Ok(())
            }))
            .expect("headless run");
    }

    fn completion_config(policy: WindowPolicy) -> SecondaryWindowInstallConfig {
        SecondaryWindowInstallConfig {
            loop_identity: APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity)),
            policy,
            shared_with: APP_RUNTIME
                .with(|slot| slot.borrow().realms.iter().next().map(|(id, _)| *id)),
            reservation: reserve_window().expect("reserve test completion"),
            close_request_handler: None,
            frame_failure_detail: FrameFailureDetail::Redacted,
        }
    }

    #[test]
    fn quit_notification_closes_queued_completions_before_the_dispatch_tail_can_install_them() {
        for policy in [WindowPolicy::SeparateRealms, WindowPolicy::SharedRealm] {
            let _clear = OwnerHostClearGuard::arm();
            let platform = flui_platform::HeadlessPlatform::new();
            let reevaluation = platform.exit_reevaluation();
            let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
            platform
                .run(Box::new(move |owner| {
                    let shared = owner.shared();
                    install_owner_platform(owner).expect("install owner wake transport");
                    let primary = install_platform_realm(
                        crate::app::ui_realm::UiRealm::for_test(),
                        &crate::app::window_test_support::headless_test_window(),
                    );
                    install_platform_quit_hook();
                    shared.set_exit_policy_hook(Box::new(|| true));
                    let closed = Arc::new(AtomicUsize::new(0));
                    let observed = Arc::clone(&closed);
                    let window = crate::app::window_test_support::headless_test_host_window();
                    window.on_close(Box::new(move || {
                        APP_RUNTIME.with(|slot| {
                            assert!(
                                slot.try_borrow_mut().is_ok(),
                                "native cleanup must run outside runtime borrow"
                            );
                        });
                        PENDING_SECONDARY_WINDOW_COMPLETIONS
                            .with(|queue| assert!(queue.try_borrow_mut().is_ok()));
                        observed.fetch_add(1, Ordering::SeqCst);
                    }));
                    let config = completion_config(policy);
                    dispatch_platform_realm(
                        primary,
                        RealmTask::Frame(Box::new(move |_| {
                            PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| {
                                queue.borrow_mut().push(PendingCompletion {
                                    config,
                                    window,
                                    install: None,
                                });
                            });
                            shared.request_exit_policy_reevaluation();
                            assert!(reevaluation.drive());
                        })),
                    )
                    .expect("dispatch");
                    assert_eq!(closed.load(Ordering::SeqCst), 1);
                    assert!(
                        PENDING_SECONDARY_WINDOW_COMPLETIONS
                            .with(|queue| queue.borrow().is_empty())
                    );
                    APP_RUNTIME.with(|slot| {
                        let state = slot.borrow();
                        assert_eq!(state.realms.iter().count(), 1);
                        let (_, installed) = state.realms.iter().next().expect("primary");
                        assert_eq!(
                            installed
                                .realm
                                .as_ref()
                                .expect("restored")
                                .presentation_count(),
                            1
                        );
                    });
                    teardown_platform_realm();
                    Ok(())
                }))
                .expect("headless run");
        }
    }

    #[test]
    fn quit_notification_cancels_unresolved_open_without_a_late_realm_install() {
        let _clear = OwnerHostClearGuard::arm();
        let platform = flui_platform::HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();
        let reevaluation = platform.exit_reevaluation();
        let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
        platform
            .run(Box::new(move |owner| {
                let shared = owner.shared();
                install_owner_platform(owner).expect("install owner wake transport");
                let primary = install_platform_realm(
                    crate::app::ui_realm::UiRealm::for_test(),
                    &crate::app::window_test_support::headless_test_window(),
                );
                install_platform_quit_hook();
                shared.set_exit_policy_hook(Box::new(|| true));
                assert!(
                    open_secondary_window_impl(AppConfig::default(), WindowPolicy::SeparateRealms)
                        .expect("pending accepted")
                        .is_none()
                );
                assert_eq!(
                    PENDING_SECONDARY_WINDOW_OPENS.with(|tasks| tasks.borrow().len()),
                    1
                );
                shared.request_exit_policy_reevaluation();
                assert!(reevaluation.drive());
                assert!(PENDING_SECONDARY_WINDOW_OPENS.with(|tasks| tasks.borrow().is_empty()));
                let late = deferred
                    .resolve_next()
                    .expect("platform can still deliver abandoned request");
                dispatch_platform_realm(
                    primary,
                    RealmTask::Frame(Box::new(|realm| {
                        realm.scheduler().drive_async_tasks();
                    })),
                )
                .expect("pump cancellation");
                assert!(
                    PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| queue.borrow().is_empty())
                );
                APP_RUNTIME.with(|slot| assert_eq!(slot.borrow().realms.iter().count(), 1));
                late.close(); // The headless resolver intentionally returns its own retained handle.
                teardown_platform_realm();
                Ok(())
            }))
            .expect("headless run");
    }

    #[test]
    fn quit_notification_old_completion_cannot_enter_a_new_loop() {
        let _clear = OwnerHostClearGuard::arm();
        let old = std::rc::Rc::new(std::cell::RefCell::new(None));
        let saved = std::rc::Rc::clone(&old);
        flui_platform::headless_platform()
            .run(Box::new(move |owner| {
                install_owner_platform(owner).expect("install owner wake transport");
                let previous = saved
                    .borrow_mut()
                    .replace(completion_config(WindowPolicy::SharedRealm));
                drop(previous);
                super::super::realm_dispatch::request_quit_notification();
                teardown_platform_realm();
                APP_RUNTIME.with(|slot| {
                    assert_eq!(
                        slot.borrow().quit_notification,
                        crate::app::runtime::QuitNotification::Notified,
                        "generic teardown must not reopen admission"
                    );
                });
                Ok(())
            }))
            .expect("old loop");
        flui_platform::headless_platform()
            .run(Box::new(move |owner| {
                install_owner_platform(owner).expect("install owner wake transport");
                install_platform_realm(
                    crate::app::ui_realm::UiRealm::for_test(),
                    &crate::app::window_test_support::headless_test_window(),
                );
                let window = crate::app::window_test_support::headless_test_host_window();
                let closed = Arc::new(AtomicUsize::new(0));
                let observed = Arc::clone(&closed);
                window.on_close(Box::new(move || {
                    observed.fetch_add(1, Ordering::SeqCst);
                }));
                let stale = old.borrow_mut().take().expect("old completion");
                assert!(finish_open_secondary_window(stale, window).is_err());
                assert_eq!(closed.load(Ordering::SeqCst), 1);
                assert!(secondary_install_admitted(
                    &completion_config(WindowPolicy::SharedRealm).loop_identity
                ));
                APP_RUNTIME.with(|slot| {
                    let state = slot.borrow();
                    assert_eq!(state.realms.iter().count(), 1);
                    assert_eq!(
                        state
                            .realms
                            .iter()
                            .next()
                            .expect("primary")
                            .1
                            .realm
                            .as_ref()
                            .expect("restored")
                            .presentation_count(),
                        1
                    );
                });
                teardown_platform_realm();
                Ok(())
            }))
            .expect("new loop");
    }

    /// `open_window` (the content-bearing companion of [`open_secondary_window`])
    /// refuses [`WindowPolicy::SharedRealm`] at admission, before any native
    /// window is created. The raster lane and the renderer a content window
    /// owns are per-realm, not per-presentation, and this gate is the one
    /// place that constraint is turned into a typed failure instead of a
    /// deferred crash (`install_desktop_window`'s mount inside a shared
    /// realm would land its own `install_realm_alongside` call against an
    /// already-hosted realm, which the registry correctly refuses — that
    /// panic would be a worse signal than this `Err`).
    ///
    /// The headless platform answers `Ready` for a window creation, so the
    /// gate is what stops the test short of any real GPU work; the test
    /// would fail identically in production, never on the shared-realm arm.
    #[test]
    fn open_window_with_content_refuses_shared_realm_at_admission() {
        let _clear = OwnerHostClearGuard::arm();
        let platform = flui_platform::HeadlessPlatform::new();
        let platform: Box<dyn flui_platform::Platform> = Box::new(platform);
        platform
            .run(Box::new(|owner| {
                install_owner_platform(owner).expect("install owner wake transport");
                // A hosted realm is required for the admission check to
                // consider the SharedRealm alternative at all — without one
                // the refusal would come from the shared-realm lookup
                // instead of the policy gate we are pinning.
                install_platform_realm(
                    crate::app::ui_realm::UiRealm::for_test(),
                    &crate::app::window_test_support::headless_test_window(),
                );
                let outcome = open_window_with_content_impl(
                    AppConfig::default(),
                    WindowPolicy::SharedRealm,
                    SecondaryContentStub,
                );
                match outcome {
                    Err(error) => {
                        let message = error.to_string();
                        assert!(
                            message.contains("SeparateRealms") || message.contains("SharedRealm"),
                            "refusal must name the rejected policy, got: {message}"
                        );
                    }
                    Ok(_) => panic!("SharedRealm with content must be refused at admission"),
                }
                Ok(())
            }))
            .expect("headless run");
    }
}

/// Minimal `View` the admission-gate test installs through
/// `open_window_with_content_impl`: the gate refuses at admission, so the
/// root is never mounted and its content never runs — the type exists to
/// name the parameter, not to be driven.
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[derive(Clone)]
struct SecondaryContentStub;

#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
impl flui_view::StatelessView for SecondaryContentStub {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        use flui_view::ViewExt;
        self.clone().boxed()
    }
}

#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
impl flui_view::View for SecondaryContentStub {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}
