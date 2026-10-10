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
use super::host::{APP_RUNTIME, runtime_wake_callback, with_owner_platform};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use super::owner_dispatch::{
    PresentationDispatcher, RuntimeEvent, RuntimeTask, close_this_window,
    dispatch_platform_ui_runtime, install_input_wiring, prepare_presentation_alongside,
    prepare_ui_runtime_alongside,
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

/// A ui_runtime/presentation install failure, retained whole as
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
/// ui_runtime/presentation topology the new window becomes. Must be called from
/// the owner thread while a loop is live (mirrors `bootstrap_desktop`'s own
/// `OwnerPlatform` access constraint: reachable only from inside, or after,
/// `on_ready` — e.g. from a `window.on_input`/`window.on_should_close`
/// callback the FIRST window already registered).
///
/// # [`WindowPolicy::Isolated`]
///
/// Opens a fully independent second ui_runtime: its own `UiRuntime`, its own
/// `GlobalKeyScope`, its own `UpdateScheduler`, prepared for alongside publication.
/// `two_ui_runtimes_via_isolated_policy_share_nothing` pins
/// the "share nothing but `SharedEngineServices`" guarantee this policy
/// claims. Input/close/should-close/focus/visibility/resize dispatch are
/// wired and addressed to this new UI runtime exactly like the FIRST window's
/// own dispatch.
///
/// # [`WindowPolicy::Shared`]
///
/// Installs a second PRESENTATION into the FIRST UI runtime hosted on this
/// thread (via `prepare_presentation_alongside`) — real forest membership,
/// a real `WindowRegistry` mapping, real addressed
/// input/close/should-close/focus/visibility dispatch.
/// `one_ui_runtime_two_windows_policy_routes_by_presentation` pins that this
/// really is forest-membership routing, not a second UI runtime in disguise.
///
/// # Completion and ownership
///
/// Native creation and owner publication can each defer completion. A reservation
/// holds loop liveness until both finish. Native `Pending` is polled on
/// window-independent owner turns. Worker completion
/// wakes that loop through its stamped `PlatformProxy`; no UI runtime scheduler or
/// visible window is needed. Unwoken futures are not polled on unrelated turns.
/// An accepted separate-UI runtime request survives closure of its originating ui_runtime.
/// Shared-UI runtime requests capture the exact target identity at admission and fail
/// if that UI runtime disappears; they never retarget another UI runtime.
///
/// Polling and installation run outside runtime borrows. Installation waits for
/// any active UI runtime checkout to finish. Quit and loop replacement invalidate
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
/// Window creation ([`AppWindowError::Native`]) and (`WindowPolicy::Isolated` only)
/// `UiRuntime` construction ([`AppWindowError::Mount`]) surface as `Err`
/// exactly like `bootstrap_desktop`'s own first-window failures — this call
/// does not tear down or exit the loop on failure, unlike a first-window
/// bootstrap failure (which propagates out of `Platform::run` and ends the
/// loop): the caller decides what a failed secondary-window open means for
/// their app. A call from a thread with no running loop is
/// [`AppWindowError::NoOwnerLoop`]; one made while the application is
/// quitting is [`AppWindowError::AdmissionClosed`]; `WindowPolicy::Shared` with no
/// UI runtime hosted on this thread yet is [`AppWindowError::UnsupportedPolicy`].
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
/// UI runtime — the same contract `run_app`'s primary window satisfies. The
/// `root` widget is mounted as the new window's root, sharing nothing
/// widget-visible with any sibling presentation on this host.
///
/// # Policy
///
/// `WindowPolicy::Isolated` is the only policy that admits content:
/// each such window owns its own `UiRuntime`, its own widget tree, its own
/// raster lane. `WindowPolicy::Shared` currently refuses content at
/// admission with an `Err` — the UI runtime's single-raster-lane contract would
/// have to be relaxed before a presentation inside one shared UI runtime could
/// own its own renderer, and that relaxation is deliberately not smuggled
/// in through this API.
///
/// # Errors
///
/// The same as [`open_secondary_window`], plus the renderer-initialization
/// failures `install_desktop_window` can produce: GPU init
/// ([`AppWindowError::Renderer`]), mount ([`AppWindowError::Mount`]).
/// `WindowPolicy::Shared` is refused as [`AppWindowError::UnsupportedPolicy`].
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
    shared_with: Option<flui_foundation::UiRuntimeId>,
    reservation: WindowReservation,
    close_request_handler: Option<CloseRequestHandler>,
    frame_failure_detail: FrameFailureDetail,
    pointer_resampling: flui_runtime::presentation::PointerResampling,
    motion_preference: flui_runtime::MotionPreference,
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
    stage: CompletionStage,
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
enum CompletionStage {
    Bare,
    Content(SecondaryWindowInstall),
    Publishing(super::installed_host::Installation),
}

/// A window the open path resolved synchronously: its UI runtime dispatcher and
/// the platform window it drives. `None` from the open functions means the
/// window was accepted but its creation is deferred to a later owner turn.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) type OpenedWindow = (
    PresentationDispatcher,
    Arc<dyn flui_platform::traits::PlatformWindow>,
);

/// Owns the generic root until preparation and returns its publication receipt.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
type SecondaryWindowInstall = Box<
    dyn FnOnce(
        Arc<dyn flui_platform::traits::HostWindow>,
    ) -> Result<super::installed_host::Installation, AppWindowError>,
>;

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
impl PendingCompletion {
    fn advance(self) -> Result<(), AppWindowError> {
        let Self {
            config,
            window,
            stage,
        } = self;
        if !secondary_install_admitted(&config.loop_identity) {
            return Err(AppWindowError::AdmissionClosed);
        }
        let receipt = match stage {
            CompletionStage::Bare => {
                return finish_open_secondary_window(config, window).map(|_| ());
            }
            CompletionStage::Content(install) => {
                if !config.reservation.0.begin_install() {
                    return Err(AppWindowError::AdmissionClosed);
                }
                install(Arc::clone(&window))?
            }
            CompletionStage::Publishing(receipt) => receipt,
        };
        if !secondary_install_admitted(&config.loop_identity) {
            return Err(AppWindowError::AdmissionClosed);
        }
        match receipt.outcome() {
            None => {
                PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| {
                    queue.borrow_mut().push(Self {
                        config,
                        window,
                        stage: CompletionStage::Publishing(receipt),
                    });
                });
                Ok(())
            }
            Some(Ok(())) => Ok(()),
            Some(Err(error)) => Err(mount_error(error)),
        }
    }
}

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

    /// Resolved windows awaiting installation after UI runtime checkout clears.
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

/// Apply queued native-window completions in request order after runtime
/// checkout returns. Reentrant polling is suppressed until the current batch
/// finishes; logical/native publication is admitted through the installed host.
///
/// A completion's own failure is traced (`tracing::error!`), never
/// propagated: by the time this runs there is no synchronous caller left for
/// either policy to return an `Err` to.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn drain_pending_secondary_window_completions(
    recovery: flui_runtime::owner::RecoveryState,
) {
    use std::future::Future;
    if APP_RUNTIME.with(|slot| {
        slot.borrow().quit_notification != crate::app::runtime::QuitNotification::Active
    }) {
        cancel_pending_secondary_windows();
        return;
    }
    if APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        state
            .installed_host
            .logical()
            .is_executing()
            .unwrap_or(true)
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
        if recovery != flui_runtime::owner::RecoveryState::Healthy
            || first_panic.is_some()
            || !request.config.reservation.0.take_ready()
        {
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
                        stage: CompletionStage::Bare,
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
        // Settling an accepted publication releases its reservation even during
        // recovery. Starting another installer or polling a future must wait.
        if (recovery != flui_runtime::owner::RecoveryState::Healthy || first_panic.is_some())
            && !matches!(completion.stage, CompletionStage::Publishing(_))
        {
            PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| queue.borrow_mut().push(completion));
            continue;
        }
        let window = Arc::clone(&completion.window);
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| completion.advance()));
        let failed = !matches!(&result, Ok(Ok(())));
        let error = match result {
            Ok(Ok(())) => None,
            Ok(Err(error)) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tracing::error!(%error, "installing resolved secondary window failed");
            }))
            .err(),
            Err(payload) => Some(payload),
        };
        crate::app::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first_panic,
            error,
            "pending window installation",
        );
        if failed {
            let error =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| window.close())).err();
            crate::app::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first_panic,
                error,
                "failed window close",
            );
        }
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
/// `close_this_window`/`request_ui_runtime_uninstall` primitives directly, which
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

    let shared_with = if policy == WindowPolicy::Shared {
        Some(
            APP_RUNTIME
                .with(|slot| {
                    slot.borrow().installed_host.logical().runtime_ids()
                        .expect("BUG: window admission runs outside pure publication")
                        .into_iter().next()
                })
                .ok_or(AppWindowError::UnsupportedPolicy {
                    reason: "WindowPolicy::Shared requires a ui_runtime already hosted on this thread to \
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
        pointer_resampling: config.pointer_resampling,
        motion_preference: config.motion_preference,
    };

    match open {
        WindowOpen::Ready(window) => finish_open_secondary_window(install_config, window),
        WindowOpen::Pending(pending) => {
            spawn_pending_secondary_window_completion(install_config, pending)?;
            Ok(None)
        }
    }
}

/// The content-bearing companion of [`open_secondary_window_impl`]:
/// identical admission and reservation protocol, but the resolved window
/// is routed to a full `install_desktop_window`-style mount instead of
/// the bare-shell `finish_open_secondary_window` — the UI runtime owns a widget
/// tree, a GPU raster lane, and a frame pump.
///
/// `WindowPolicy::Shared` is refused at admission, before any window creation
/// work runs: the UI runtime's raster lane and content renderer are per-UI runtime,
/// not per-presentation today, so a shared UI runtime presenting N windows
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

    if policy == WindowPolicy::Shared {
        return Err(AppWindowError::UnsupportedPolicy {
            reason: "open_window with content requires WindowPolicy::Isolated; WindowPolicy::Shared \
                     would imply a lane-per-presentation raster contract the ui_runtime does not \
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
            // Preparation consumes the captured root once. The completion then
            // retains the native window and liveness reservation until the
            // owner's publication receipt settles, including when this request
            // originates inside a widget callback.
            let install_config = SecondaryWindowInstallConfig {
                loop_identity: Arc::clone(&loop_identity),
                policy,
                shared_with: None,
                reservation,
                close_request_handler: config.close_request_handler.clone(),
                frame_failure_detail: config.frame_failure_detail,
                pointer_resampling: config.pointer_resampling,
                motion_preference: config.motion_preference,
            };
            let reload = crate::app::hot_reload::WorkerReload::from_config(&config);
            let host = APP_RUNTIME.with(|slot| slot.borrow().main_host_lifecycle);
            let config_for_install = config;
            PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| {
                queue.borrow_mut().push(PendingCompletion {
                    config: install_config,
                    window: Arc::clone(&window),
                    stage: CompletionStage::Content(Box::new(move |window| {
                        super::desktop::install_desktop_window(
                            root,
                            &config_for_install,
                            reload,
                            Arc::clone(&window),
                            host,
                        )
                        .map(|rendered| rendered.installation)
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
            // `UiRuntime::request_redraw` already does from
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
/// No UI runtime or frame is required. Synchronous posting failure returns an error
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
/// ui_runtime/presentation topology [`WindowPolicy`] governs and wires every
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
) -> Result<Option<OpenedWindow>, AppWindowError> {
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
    if !secondary_install_admitted(&config.loop_identity) || !config.reservation.0.begin_install() {
        return Err(AppWindowError::AdmissionClosed);
    }

    let policy = config.policy;
    let shared_with = config.shared_with;
    let mut installation =
        match policy {
            WindowPolicy::Shared => {
                // Failure detail is ui_runtime-scoped. A secondary presentation
                // inherits the already-hosted ui_runtime's policy; its window config
                // must not mutate that policy for existing siblings.
                let shared_with = APP_RUNTIME
                .with(|slot| {
                    let state = slot.borrow();
                    let ui_runtime_id = shared_with?;
                    let status = state.installed_host.logical().runtime_status(ui_runtime_id).ok()?;
                    Some(PresentationDispatcher {
                        owner_thread: state.owner_thread?,
                        address: status.primary,
                    })
                })
                .ok_or(AppWindowError::UnsupportedPolicy {
                    reason: "WindowPolicy::Shared requires an already-hosted ui_runtime to share \
                             with; none is installed on this thread",
                })?;
                prepare_presentation_alongside(
                    shared_with,
                    super::presentation_window(Arc::clone(&host))
                        .with_pointer_resampling(config.pointer_resampling),
                )
                .map_err(mount_error)?
            }
            WindowPolicy::Isolated => {
                let scale_factor = window.scale_factor();
                let wake = runtime_wake_callback();
                let ui_runtime = super::host::build_ui_runtime(
                    &wake,
                    super::presentation_window(Arc::clone(&host))
                        .with_pointer_resampling(config.pointer_resampling),
                    scale_factor,
                )
                .map_err(mount_error)?;
                ui_runtime.set_frame_failure_detail(config.frame_failure_detail);
                ui_runtime.set_motion_preference(config.motion_preference);
                // No frame-failure handler is installed here. Under
                // `open_secondary_window`'s current contract this ui_runtime has no
                // root widget or renderer, so secondary handler ownership is
                // blocked on the documented secondary-window rendering contract.
                prepare_ui_runtime_alongside(ui_runtime, Arc::clone(&window))
            }
        };
    let owner_dispatch = installation.dispatcher();

    tracing::warn!(
        ?policy,
        ?owner_dispatch,
        "open_secondary_window: preparing an addressed window with no widget content and no \
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
    installation.close_requests(config.close_request_handler.clone());

    install_input_wiring(owner_dispatch, window.as_ref());

    window.on_resize(Box::new(move |size, scale_factor| {
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::Resized { size, scale_factor }),
        );
    }));

    // Window close -> close THIS window's own presentation, exactly like
    // `run_desktop`'s primary window (see `close_this_window`'s own doc):
    // `WindowPolicy::Isolated` reduces to a full uninstall of this new, independent
    // ui_runtime (its sole presentation); `WindowPolicy::Shared` removes just this
    // presentation from the shared ui_runtime's forest while the primary (and
    // any other sibling) survives untouched -- never a blind
    // `request_ui_runtime_uninstall`, which would tear down the WHOLE shared
    // ui_runtime out from under a still-open sibling window.
    //
    // No `on_quit` registration here — that is a single platform-level
    // callback slot the FIRST window's bootstrap already owns
    // (`Platform::on_quit`/`SharedPlatform::on_quit` replace, never stack);
    // registering a second one here would silently steal the first window's
    // Detached-lifecycle notification on process quit instead of adding to
    // it. The loop-owned quit callback visits every installed ui_runtime once,
    // including this window's ui_runtime, after any active dispatch restores it.
    installation.on_close(move || {
        tracing::info!(?owner_dispatch, "Secondary window closed");
        close_this_window(owner_dispatch);
    });
    // The prepared close-request wiring publishes its router entry with the window.
    window.on_active_status_change(Box::new(move |focused| {
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::WindowFocus(focused)),
        );
    }));
    window.on_execution_state_change(Box::new(move |state| {
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::WindowExecution(state)),
        );
    }));
    window.on_visibility_status_change(Box::new(move |visible| {
        let _ = dispatch_platform_ui_runtime(
            owner_dispatch,
            RuntimeTask::Event(RuntimeEvent::WindowVisibility(visible)),
        );
    }));
    let execution = window.execution_state();
    let focused = window.is_focused();
    let visible = window.is_visible();
    installation.observe(flui_runtime::owner::WindowObservation::Snapshot {
        execution,
        focused,
        visible,
    });
    let receipt = installation.submit();
    match receipt.outcome() {
        Some(Err(error)) => return Err(mount_error(error)),
        Some(Ok(())) => {}
        None => {
            PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| {
                queue.borrow_mut().push(PendingCompletion {
                    config,
                    window: host,
                    stage: CompletionStage::Publishing(receipt),
                });
            });
            uninstalled.0 = None;
            return Ok(None);
        }
    }
    uninstalled.0 = None;
    Ok(Some((owner_dispatch, window)))
}

#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod tests {
    use super::*;
    use flui_platform::{HeadlessPlatform, Platform};
    use flui_runtime::{owner::WindowObservation, ui_runtime::UiRuntime};
    use std::{cell::Cell, rc::Rc, sync::atomic::Ordering};

    // Inject a renderer through the same completion transport without requiring
    // a GPU. Initial metrics exercise the installed native driver, not a mock queue.
    fn rendered_secondary_retains_reservation_through_initialization() {
        installation_case(InstallCase::Ready);
    }

    fn rendered_secondary_closed_before_publication_releases_reservation() {
        installation_case(InstallCase::Closed);
    }

    fn rendered_secondary_initialization_failure_retires_membership() {
        installation_case(InstallCase::InitializationPanic);
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum InstallCase {
        Ready,
        Closed,
        InitializationPanic,
    }

    fn installation_case(case: InstallCase) {
        let _clear = super::super::host::OwnerHostClearGuard::arm();
        Box::new(HeadlessPlatform::new())
            .run(Box::new(move |owner| {
                let window = owner
                    .open_window(flui_platform::WindowOptions::default())
                    .expect("native window");
                let flui_platform::WindowOpen::Ready(window) = window else {
                    panic!("headless ready window")
                };
                super::super::host::install_owner_platform(owner).expect("install owner");
                let outer = super::super::owner_dispatch::install_platform_ui_runtime(
                    UiRuntime::for_test(),
                    &crate::app::window_test_support::headless_test_window(),
                );
                let count =
                    APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().pending_window_reservations));
                let during = Rc::new(Cell::new(None));
                let observed = Rc::clone(&during);
                let retained = Arc::clone(&count);
                let address = Rc::new(Cell::new(None));
                let installed_address = Rc::clone(&address);
                let config = SecondaryWindowInstallConfig {
                    loop_identity: APP_RUNTIME
                        .with(|slot| Arc::clone(&slot.borrow().loop_identity)),
                    policy: WindowPolicy::Isolated,
                    shared_with: None,
                    reservation: reserve_window().expect("reserve window"),
                    close_request_handler: None,
                    frame_failure_detail: AppConfig::new().frame_failure_detail,
                    pointer_resampling: AppConfig::new().pointer_resampling,
                    motion_preference: AppConfig::new().motion_preference,
                };
                PENDING_SECONDARY_WINDOW_COMPLETIONS.with(|queue| {
                    queue.borrow_mut().push(PendingCompletion {
                        config,
                        window,
                        stage: CompletionStage::Content(Box::new(move |window| {
                            let runtime = UiRuntime::for_test();
                            runtime
                                .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 30.0))
                                .expect("root");
                            let window: Arc<dyn flui_platform::traits::PlatformWindow> = window;
                            let mut prepared =
                                super::super::owner_dispatch::prepare_ui_runtime_alongside(
                                    runtime,
                                    Arc::clone(&window),
                                );
                            installed_address.set(Some(prepared.dispatcher().address));
                            prepared
                                .frame_driver(super::super::frame_driver::FrameDriver::Test(
                                    super::super::frame_driver::TestFrameDriver {
                                        installed: None,
                                        sink: flui_runtime::testing::ScriptedSink::new(|_, _| {
                                            flui_runtime::sink::SubmitVerdict::Presented
                                        }),
                                        prelude: None,
                                        resize: Some(Box::new(move |_, _| {
                                            observed.set(Some(retained.load(Ordering::Acquire)));
                                            assert!(
                                                case != InstallCase::InitializationPanic,
                                                "initial metrics failure"
                                            );
                                        })),
                                    },
                                ))
                                .expect("driver");
                            prepared.observe(WindowObservation::Metrics {
                                size: flui_foundation::geometry::Size::new(20.0, 30.0),
                                scale_factor: 1.0,
                            });
                            let receipt = prepared.submit();
                            assert!(
                                receipt.outcome().is_none(),
                                "publication is queued in the completion tail"
                            );
                            if case == InstallCase::Closed {
                                window.close();
                            }
                            Ok(receipt)
                        })),
                    });
                });
                let delivery = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    dispatch_platform_ui_runtime(
                        outer,
                        RuntimeTask::TestCallback(Box::new(|_| {})),
                    )
                    .expect("drain publication");
                }));
                assert_eq!(
                    delivery.is_err(),
                    case == InstallCase::InitializationPanic,
                    "initialization preserves its failure"
                );
                assert_eq!(
                    during.get(),
                    if case == InstallCase::Closed {
                        None
                    } else {
                        Some(1)
                    },
                    "loop liveness survives until initial native observations complete"
                );
                assert_eq!(
                    count.load(Ordering::Acquire),
                    0,
                    "completion releases its reservation"
                );
                let registered = APP_RUNTIME.with(|slot| {
                    slot.borrow()
                        .installed_host
                        .native()
                        .contains_address(address.get().expect("prepared address"))
                });
                assert_eq!(
                    registered,
                    case == InstallCase::Ready,
                    "only a completed installation retains membership"
                );
                Ok(())
            }))
            .expect("headless run");
    }

    #[test]
    fn secondary_installation_contract() {
        crate::table_test::run_table(
            "secondary_installation_contract",
            &[
                (
                    "bare_secondary_reports_pending_during_owner_delivery",
                    bare_secondary_reports_pending_during_owner_delivery as fn(),
                ),
                (
                    "rendered_secondary_retains_reservation_through_initialization",
                    rendered_secondary_retains_reservation_through_initialization as fn(),
                ),
                (
                    "rendered_secondary_closed_before_publication_releases_reservation",
                    rendered_secondary_closed_before_publication_releases_reservation as fn(),
                ),
                (
                    "rendered_secondary_initialization_failure_retires_membership",
                    rendered_secondary_initialization_failure_retires_membership as fn(),
                ),
            ],
        );
    }

    fn bare_secondary_reports_pending_during_owner_delivery() {
        for policy in [WindowPolicy::Isolated, WindowPolicy::Shared] {
            let _clear = super::super::host::OwnerHostClearGuard::arm();
            Box::new(HeadlessPlatform::new())
                .run(Box::new(move |owner| {
                    super::super::host::install_owner_platform(owner).expect("owner");
                    let outer = super::super::owner_dispatch::install_platform_ui_runtime(
                        UiRuntime::for_test(),
                        &crate::app::window_test_support::headless_test_window(),
                    );
                    dispatch_platform_ui_runtime(
                        outer,
                        RuntimeTask::TestCallback(Box::new(move |_| {
                            let opened = open_secondary_window_impl(AppConfig::new(), policy)
                                .expect("admit secondary");
                            assert!(
                                opened.is_none(),
                                "{policy:?}: queued publication cannot return a ready window"
                            );
                        })),
                    )
                    .expect("finish publication");
                    let pending = APP_RUNTIME.with(|slot| {
                        slot.borrow()
                            .pending_window_reservations
                            .load(Ordering::Acquire)
                    });
                    assert_eq!(pending, 0, "publication completion releases reservation");
                    Ok(())
                }))
                .expect("headless run");
        }
    }
}
