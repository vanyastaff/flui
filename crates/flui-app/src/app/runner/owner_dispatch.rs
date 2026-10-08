use flui_runtime::owner::RuntimeOperation;
#[cfg(any(test, target_os = "android"))]
use flui_scheduler::AppLifecycleState;

use super::host::APP_RUNTIME;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::lifecycle_state::preserve_first_lifecycle_panic;

#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct PresentationDispatcher {
    pub(super) owner_thread: std::thread::ThreadId,
    pub(super) address: flui_foundation::PresentationAddress,
}

#[cfg(any(test, target_os = "android", target_os = "ios"))]
impl PresentationDispatcher {
    pub(super) fn runtime(self) -> RuntimeDispatcher {
        RuntimeDispatcher {
            owner_thread: self.owner_thread,
            id: self.address.ui_runtime_id,
        }
    }
}

/// Runtime-wide authority does not expire when its original window closes.
#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct RuntimeDispatcher {
    pub(super) owner_thread: std::thread::ThreadId,
    pub(super) id: flui_foundation::UiRuntimeId,
}

impl RuntimeDispatcher {
    pub(super) fn fonts_changed(self) -> Result<(), DispatchError> {
        dispatch_owner_turn(OwnerTurn::Runtime(self, RuntimeOperation::FontsChanged))
    }

    #[cfg(any(test, target_os = "android"))]
    pub(super) fn lifecycle(self, state: AppLifecycleState) -> Result<(), DispatchError> {
        dispatch_owner_turn(OwnerTurn::Runtime(self, RuntimeOperation::Lifecycle(state)))
    }

    #[cfg(any(
        all(test, not(target_os = "android"), not(target_arch = "wasm32")),
        target_os = "ios"
    ))]
    pub(super) fn background(self) -> Result<(), DispatchError> {
        dispatch_owner_turn(OwnerTurn::Runtime(self, RuntimeOperation::Background))
    }
}

/// An admitted operation carries exactly the authority its behavior needs.
pub(in crate::app) enum OwnerTurn {
    Presentation(PresentationDispatcher, RuntimeTask),
    Runtime(RuntimeDispatcher, RuntimeOperation),
}

impl OwnerTurn {
    fn owner_thread(&self) -> std::thread::ThreadId {
        match self {
            Self::Presentation(dispatcher, _) => dispatcher.owner_thread,
            Self::Runtime(dispatcher, _) => dispatcher.owner_thread,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DispatchError {
    WrongThread,
    /// The UI runtime incarnation this dispatcher was minted for is gone — the
    /// common path: `ui_runtime_id`/`presentation_id` mint from one shared
    /// counter, so teardown+reinstall always changes both, and this check
    /// (UI runtime first) catches it before the presentation half is even
    /// compared.
    StaleRuntime,
    /// The UI runtime is live and matches, but the presentation incarnation does
    /// not — reachable today only via a forged/mixed address (a dispatcher
    /// whose presentation half was swapped for another incarnation's), and,
    /// once one UI runtime can host more than one presentation, via real
    /// presentation replacement within a live ui_runtime. Kept as its own
    /// variant now: the design-for-N contract, not dead code.
    StalePresentation,
    /// A terminal close for this exact presentation incarnation was already
    /// accepted. Later work must not jump the deferred close at a bounded
    /// batch boundary.
    PresentationClosing,
    RuntimeUnavailable,
}

impl From<flui_runtime::owner::DispatchError> for DispatchError {
    fn from(error: flui_runtime::owner::DispatchError) -> Self {
        use flui_runtime::owner::DispatchError as OwnerError;
        match error {
            OwnerError::UnknownRuntime => Self::StaleRuntime,
            OwnerError::UnknownPresentation => Self::StalePresentation,
            OwnerError::PresentationClosing => Self::PresentationClosing,
            _ => Self::RuntimeUnavailable,
        }
    }
}

/// Typed presentation observations applied on an owner turn: native input,
/// lifecycle and surface restoration. The payload is
/// `Send` (ADR-0037 §3); execution still goes through exact-address admission
/// on the owner thread, never through an arbitrary cross-thread callback.
// `pub(in crate::app)` because `RuntimeTask::Event` (also `pub(in crate::app)`, for
// `AppRuntime`'s sake) carries this type in a field the compiler considers
// reachable at that same visibility.
// The window-event variants (`WindowFocus`, `WindowVisibility`,
// `AppearanceChanged`, `WindowHover`) are produced only by the desktop
// runner's `on_window_event` wiring; the mobile and web runners drive their
// lifecycle from platform callbacks instead, so those variants are
// unconstructed there.
#[cfg_attr(
    all(
        any(not(test), target_os = "android"),
        // Only android and iOS drop the window-event variants: the web runner
        // constructs `WindowFocus`/`WindowHover` through the browser's
        // visibility/focus signals, so on wasm32 they are live.
        any(target_os = "android", target_os = "ios")
    ),
    expect(
        dead_code,
        reason = "window-event variants are produced only by the desktop runner"
    )
)]
pub(in crate::app) enum RuntimeEvent {
    Resized {
        size: flui_foundation::geometry::Size<f64>,
        scale_factor: f64,
    },
    /// Window focus changed (winit's `WindowEvent::Focused`, or the
    /// equivalent per-backend signal; same source as the deleted `Active`
    /// variant this one replaces). Feeds the `(visible, focused)` ->
    /// `AppLifecycleState` derivation below, alongside
    /// [`WindowVisibility`](Self::WindowVisibility).
    WindowFocus(bool),
    /// Reversible native execution eligibility for one presentation.
    #[cfg_attr(
        any(target_arch = "wasm32", target_os = "android"),
        expect(
            dead_code,
            reason = "these runners retain their existing host lifecycle transport"
        )
    )]
    WindowExecution(flui_platform::WindowExecutionState),
    /// Addressed logical content-view safe area.
    ///
    /// The iOS runner is the only producer; this module's own tests construct
    /// it directly to pin the addressed-write contract (a report for a
    /// presentation closed before delivery is dropped, not a panic).
    #[cfg_attr(
        not(target_os = "ios"),
        expect(
            dead_code,
            reason = "safe-area reports are produced only by the UIKit runner"
        )
    )]
    SafeAreaChanged(flui_foundation::geometry::EdgeInsets),
    /// Window visibility/occlusion changed (winit's `WindowEvent::Occluded`,
    /// negated — see `PlatformWindow::on_visibility_status_change`).
    ///
    /// Combined with [`WindowFocus`](Self::WindowFocus) via
    /// the addressed presentation's lifecycle reconciliation. The UI runtime
    /// scheduler derives its aggregate from all live presentations.
    // Not yet constructed on wasm32: `run_web` only wires `WindowFocus` —
    // no occlusion signal for the web backend yet (see run_web's comment at
    // its `on_active_status_change` registration).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    WindowVisibility(bool),
    /// The OS light/dark appearance changed (winit's `ThemeChanged`, or the
    /// equivalent per-backend signal). Republishes
    /// `MediaQueryData::platform_brightness` through the root media query.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    AppearanceChanged(flui_platform::WindowAppearance),
    /// The pointer entered (`true`) or left (`false`) the window (winit's
    /// `CursorEntered`/`CursorLeft`, via
    /// `PlatformWindow::on_hover_status_change`). Leave sweeps the addressed
    /// presentation's hover state — `MouseRegion::on_exit` fires and the
    /// cursor resets; without this a widget hovered at the moment the cursor
    /// crosses the window edge keeps its hover visuals forever. Enter is a
    /// no-op today: the next `CursorMoved` re-primes hover from a fresh hit
    /// test on its own.
    // Constructed by both the desktop and web bootstraps' registrations, so
    // every backend whose `PlatformWindow` dispatches
    // `on_hover_status_change` routes here; a backend that never fires the
    // callback simply never constructs the event.
    WindowHover(bool),
    /// A recreated native surface has no previous scene to display. Surface
    /// recovery still belongs to the primary presentation, not every sibling.
    #[cfg(any(test, target_os = "android", target_os = "ios"))]
    PrimarySurfaceRestored,
}

/// One queued unit of owner-thread work. Shared-UI runtime notifications use
/// typed operations; arbitrary shared-UI runtime callbacks are test-only.
/// Events carry observations; pumps own the host's frame execution protocol.
/// This enum never crosses a thread (ADR-0037 §3).
///
/// `Frame` addresses an installed driver. Like `ClosePresentation` it needs
/// `&mut UiRuntime` — [`UiRuntime::pump`](crate::app::ui_runtime::UiRuntime::pump)
/// takes the UI runtime exclusively and enters it itself — so the drain loop runs
/// it on the checked-out UI runtime without entering it first; the driver enters
/// the UI runtime explicitly for its backend gate and prelude.
pub(in crate::app) enum RuntimeTask {
    Event(RuntimeEvent),
    #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
    TestCallback(Box<dyn FnOnce(&crate::app::ui_runtime::UiRuntime)>),
    Frame(super::frame_driver::FrameBinding),
    ClosePresentation(flui_foundation::PresentationId),
}

impl RuntimeEvent {
    fn deliver(
        self,
        host: &super::InstalledHost,
        address: flui_foundation::PresentationAddress,
        target: &flui_runtime::owner::PresentationDispatcher,
    ) -> Result<flui_runtime::owner::Delivery, flui_runtime::owner::DispatchError> {
        use flui_runtime::owner::WindowObservation;
        #[cfg(not(any(test, target_os = "android", target_os = "ios")))]
        let _ = address;
        let observation = match self {
            #[cfg(any(test, target_os = "android", target_os = "ios"))]
            Self::PrimarySurfaceRestored => {
                return host
                    .logical()
                    .frame_dispatcher(address)?
                    .surface_restored(host.effects());
            }
            Self::Resized { size, scale_factor } => {
                WindowObservation::Metrics { size, scale_factor }
            }
            Self::SafeAreaChanged(insets) => WindowObservation::SafeArea(insets),
            Self::WindowFocus(focused) => WindowObservation::Focus(focused),
            Self::WindowVisibility(visible) => WindowObservation::Visibility(visible),
            Self::WindowExecution(execution) => WindowObservation::Execution(execution),
            Self::WindowHover(inside) => WindowObservation::Hover(inside),
            Self::AppearanceChanged(appearance) => {
                use flui_platform::WindowAppearance;
                WindowObservation::Brightness(match appearance {
                    WindowAppearance::Dark | WindowAppearance::VibrantDark => {
                        flui_platform_api::Brightness::Dark
                    }
                    WindowAppearance::Light | WindowAppearance::VibrantLight => {
                        flui_platform_api::Brightness::Light
                    }
                })
            }
        };
        target.observe(observation, host.effects())
    }
}

/// Test fixture for an idle, fresh host. It exercises production preparation
/// and requires completed publication before returning a dispatcher.
#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
pub(super) fn install_platform_ui_runtime(
    ui_runtime: crate::app::ui_runtime::UiRuntime,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> PresentationDispatcher {
    let prepared = prepare_replacement_ui_runtime(ui_runtime, std::sync::Arc::clone(window));
    let dispatcher = prepared.dispatcher();
    prepared
        .submit()
        .outcome()
        .expect("fresh test host publishes synchronously")
        .expect("fresh test host accepts its initial window");
    dispatcher
}

#[cfg(any(test, target_os = "android", target_arch = "wasm32"))]
pub(super) fn prepare_platform_ui_runtime(
    ui_runtime: crate::app::ui_runtime::UiRuntime,
    window: std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> super::window_install::WindowInstall {
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        let _ = state.ensure_execution();
        #[cfg(not(target_arch = "wasm32"))]
        state.reopen_lifecycles();
    });
    // Construction already read this host's services and preferences. Changing
    // the host here would invalidate that initial state after the root mounted.
    prepare_ui_runtime_alongside(ui_runtime, window)
}

/// Explicit replacement fixture for shutdown/reentry tests. Production creates
/// its host before runtime construction and retains it through publication.
#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
pub(super) fn prepare_replacement_ui_runtime(
    ui_runtime: crate::app::ui_runtime::UiRuntime,
    window: std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> super::window_install::WindowInstall {
    let host = super::InstalledHost::new();
    let prepared = host.logical().prepare_runtime(ui_runtime);
    let installation = super::window_install::WindowInstall::new(host.clone(), prepared, window);
    let displaced = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.owner_thread = Some(std::thread::current().id());
        let _ = state.ensure_services();
        let _ = state.ensure_execution();
        #[cfg(not(target_arch = "wasm32"))]
        state.reopen_lifecycles();
        std::mem::replace(&mut state.installed_host, host)
    });
    displaced.shutdown();
    installation
}

/// Idle-host test fixture using the production installation receipt.
#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
pub(super) fn install_ui_runtime_alongside(
    ui_runtime: crate::app::ui_runtime::UiRuntime,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> Result<PresentationDispatcher, super::installed_host::InstallError> {
    let prepared = prepare_ui_runtime_alongside(ui_runtime, std::sync::Arc::clone(window));
    let dispatcher = prepared.dispatcher();
    prepared
        .submit()
        .outcome()
        .expect("idle test host publishes synchronously")?;
    Ok(dispatcher)
}

pub(super) fn prepare_ui_runtime_alongside(
    ui_runtime: crate::app::ui_runtime::UiRuntime,
    window: std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> super::window_install::WindowInstall {
    let host = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state
            .owner_thread
            .get_or_insert_with(|| std::thread::current().id());
        let _ = state.ensure_services();
        state.installed_host.clone()
    });
    let prepared = host.logical().prepare_runtime(ui_runtime);
    super::window_install::WindowInstall::new(host, prepared, window)
}

#[cfg(any(
    all(test, not(target_os = "android"), not(target_arch = "wasm32")),
    all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    )
))]
pub(super) fn prepare_presentation_alongside(
    dispatcher: PresentationDispatcher,
    window: crate::app::presentation::PresentationWindow,
) -> Result<super::window_install::WindowInstall, InstallPresentationError> {
    if std::thread::current().id() != dispatcher.owner_thread {
        return Err(InstallPresentationError::RuntimeUnavailable);
    }
    let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    let native = std::sync::Arc::clone(window.window());
    let prepared = host
        .logical()
        .prepare_presentation(dispatcher.address, window)
        .map_err(|refused| {
            let error = InstallPresentationError::from(refused.error);
            drop(refused);
            error
        })?;
    Ok(super::window_install::WindowInstall::new(
        host, prepared, native,
    ))
}

/// Errors from preparing a shared presentation. A dedicated type rather
/// than folding into [`DispatchError`]: none of that enum's variants
/// mean "the UI runtime is fine, but this specific window id collided" —
/// mislabeling that as `RuntimeUnavailable` (the pre-fix shape) told a caller
/// the wrong thing about what actually went wrong.
#[cfg_attr(
    not(any(
        all(test, not(target_os = "android"), not(target_arch = "wasm32")),
        all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        )
    )),
    expect(
        dead_code,
        reason = "prepare_presentation_alongside (its production caller) is desktop-only -- \
                  android/wasm32 have no caller outside this module's own tests"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum InstallPresentationError {
    /// `dispatcher`'s UI runtime no longer exists (a newer UI runtime replaced it, or
    /// it was already torn down).
    #[error("the ui_runtime this dispatcher was minted for no longer exists")]
    RuntimeUnavailable,
    /// A dispatch or terminal notification is currently in flight on this
    /// thread; see this function's own doc for why that is a named,
    /// stated gap rather than a defer-to-idle path.
    #[error("a dispatch or terminal notification is in flight on this thread")]
    DispatchInFlight,
    /// `dispatcher`'s UI runtime is live, but `dispatcher.address` itself is no
    /// longer registered in `WindowRegistry` — the presentation it was
    /// minted for closed since, even though a DIFFERENT presentation kept
    /// the UI runtime alive. The same authorization check
    /// `dispatch_platform_ui_runtime` runs (`registry.contains_address`), applied
    /// here too: a caller must hold a dispatcher whose exact address is
    /// CURRENTLY live to authorize installing another presentation
    /// alongside it, not merely one whose UI runtime happens to still exist.
    #[error("the presentation this dispatcher was minted for is no longer registered")]
    StalePresentation,
    /// A terminal close for `dispatcher.address` was already admitted but
    /// has not necessarily reached the bounded owner FIFO yet. The address
    /// is still registered during that interval, but it no longer
    /// authorizes expanding the presentation forest: doing so could turn a
    /// sole-presentation close into a partial close after admission.
    #[error("the presentation this dispatcher was minted for is closing")]
    PresentationClosing,
    /// `window`'s id was already registered to a (possibly different)
    /// address — practically unreachable for a freshly opened window, but
    /// a real, distinct failure mode from `RuntimeUnavailable`: the UI runtime
    /// itself is perfectly fine.
    #[error("window is already registered: {0}")]
    WindowAlreadyMapped(#[from] crate::app::window_registry::RegistryError),
    #[error(transparent)]
    Native(#[from] super::native_bindings::NativeInstallError),
    #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
    #[error("presentation initialization did not complete")]
    InitializationFailed,
}

impl From<flui_runtime::owner::PublicationError> for InstallPresentationError {
    fn from(error: flui_runtime::owner::PublicationError) -> Self {
        use flui_runtime::owner::PublicationError;
        match error {
            PublicationError::PresentationClosing => Self::PresentationClosing,
            PublicationError::UnknownPresentation => Self::StalePresentation,
            PublicationError::Busy => Self::DispatchInFlight,
            _ => Self::RuntimeUnavailable,
        }
    }
}

/// Idle-host test fixture requiring the shared presentation to finish publication.
#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
pub(super) fn install_presentation_alongside(
    dispatcher: PresentationDispatcher,
    window: impl Into<crate::app::presentation::PresentationWindow>,
) -> Result<PresentationDispatcher, InstallPresentationError> {
    let prepared = prepare_presentation_alongside(dispatcher, window.into())?;
    let installed = prepared.dispatcher();
    prepared
        .submit()
        .outcome()
        .expect("idle test host publishes synchronously")
        .map_err(|error| match error {
            super::installed_host::InstallError::Logical(error) => error.into(),
            super::installed_host::InstallError::Native(
                super::native_bindings::NativeInstallError::Window(error),
            ) => InstallPresentationError::WindowAlreadyMapped(error),
            super::installed_host::InstallError::Native(error) => error.into(),
            super::installed_host::InstallError::InitializationFailed => {
                InstallPresentationError::InitializationFailed
            }
            super::installed_host::InstallError::WindowClosed => {
                InstallPresentationError::PresentationClosing
            }
        })?;
    Ok(installed)
}

/// Admit terminal closure for the exact presentation through the owner FIFO.
/// Native frame admission closes immediately; native and logical retirement
/// execute once the current checkout returns. Sibling presentations survive.
fn close_presentation(
    dispatcher: PresentationDispatcher,
    id: flui_foundation::PresentationId,
) -> Result<(), DispatchError> {
    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::ClosePresentation(id))
}

/// Closes exactly the window `dispatcher` addresses — the single production
/// `on_close` wiring point for every window this crate opens (`run_desktop`'s
/// primary, both [`open_secondary_window`](super::secondary_window::open_secondary_window) policies). Routes through
/// [`close_presentation`], which correctly reduces to a full UI runtime uninstall
/// when `dispatcher`'s presentation is its UI runtime's ONLY one (a
/// [`WindowPolicy::Isolated`](crate::app::runtime::WindowPolicy::Isolated) window, or the last surviving
/// presentation of a [`WindowPolicy::Shared`](crate::app::runtime::WindowPolicy::Shared) group), or removes just
/// that one presentation while its UI runtime and any sibling presentation
/// survive otherwise — never `request_ui_runtime_uninstall` directly, which
/// would tear down an ENTIRE `WindowPolicy::Shared` group out from under a still-open
/// sibling window.
pub(super) fn close_this_window(dispatcher: PresentationDispatcher) {
    if let Err(error) = close_presentation(dispatcher, dispatcher.address.presentation_id) {
        tracing::warn!(?dispatcher, ?error, "close_this_window: dispatch refused");
    }
}

/// Route every input event of `window` to its UI runtime: the one input wiring
/// each runner installs.
pub(super) fn install_input_wiring(
    dispatcher: PresentationDispatcher,
    window: &(impl flui_platform::traits::PlatformWindow + ?Sized),
) {
    window.on_input(Box::new(move |input| {
        dispatch_platform_input(dispatcher, input)
    }));
}

/// Deliver a window's platform input to its UI runtime and answer the platform
/// with the UI runtime's decision. A key no handler took keeps the platform's own
/// default (Alt+F4 closes, Alt+Space opens the system menu); a consumed one
/// prevents it. So does a key whose outcome is not known when the callback
/// returns (queued behind the current owner turn, or refused), so a shortcut
/// that will consume it later never races the default. Every other input
/// (pointer, IME, drag and drop) is always reported handled: the UI runtime owns
/// it, and a backend that redraws only for handled input (Android) must keep
/// doing so.
fn dispatch_platform_input(
    dispatcher: PresentationDispatcher,
    input: flui_platform::traits::PlatformInput,
) -> flui_platform::DispatchEventResult {
    let outcome = current_host(dispatcher.owner_thread).and_then(|host| {
        let _callback = host
            .logical()
            .begin_callback(host.effects())
            .map_err(DispatchError::from)?;
        let result = host
            .logical()
            .presentation_dispatcher(dispatcher.address)
            .and_then(|target| target.input(input, host.effects()))
            .map_err(DispatchError::from);
        super::fonts::announce_font_change();
        result
    });
    let default_prevented = !matches!(outcome, Ok(flui_runtime::owner::InputOutcome::Unhandled));
    flui_platform::DispatchEventResult::resolved(false, default_prevented)
}

pub(super) fn dispatch_platform_ui_runtime(
    dispatcher: PresentationDispatcher,
    event: RuntimeTask,
) -> Result<(), DispatchError> {
    dispatch_owner_turn(OwnerTurn::Presentation(dispatcher, event))
}

fn dispatch_owner_turn(turn: OwnerTurn) -> Result<(), DispatchError> {
    let host = current_host(turn.owner_thread())?;
    let _callback = host
        .logical()
        .begin_callback(host.effects())
        .map_err(DispatchError::from)?;
    let result = match turn {
        OwnerTurn::Runtime(dispatcher, operation) => {
            let target = host
                .logical()
                .runtime_dispatcher(dispatcher.id)
                .map_err(DispatchError::from)?;
            target.deliver(operation, host.effects())
        }
        OwnerTurn::Presentation(dispatcher, task) => {
            let target = host
                .logical()
                .presentation_dispatcher(dispatcher.address)
                .map_err(DispatchError::from)?;
            match task {
                RuntimeTask::Frame(binding) => {
                    if binding.address() != dispatcher.address {
                        return Err(DispatchError::StalePresentation);
                    }
                    host.logical()
                        .frame_dispatcher(dispatcher.address)
                        .and_then(|frame| frame.deliver(host.effects()))
                }
                RuntimeTask::ClosePresentation(id) => {
                    let address = flui_foundation::PresentationAddress {
                        presentation_id: id,
                        ..dispatcher.address
                    };
                    let closing = host
                        .logical()
                        .presentation_dispatcher(address)
                        .map_err(DispatchError::from)?;
                    host.native().fence_presentation(address);
                    closing.close(host.effects())
                }
                RuntimeTask::Event(event) => event.deliver(&host, dispatcher.address, &target),
                #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
                RuntimeTask::TestCallback(run) => target.test_callback(run, host.effects()),
            }
        }
    }
    .map(|_| ())
    .map_err(DispatchError::from);
    super::fonts::announce_font_change();
    result
}

fn current_host(
    owner_thread: std::thread::ThreadId,
) -> Result<super::InstalledHost, DispatchError> {
    if std::thread::current().id() != owner_thread {
        return Err(DispatchError::WrongThread);
    }
    let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    if host.logical().runtime_count() == 0 {
        return Err(DispatchError::RuntimeUnavailable);
    }
    Ok(host)
}

/// Keep the exact installed host alive across the physical callback and its tail.
pub(super) fn with_owner_callback<R>(run: impl FnOnce(bool) -> R) -> R {
    let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    let callback = host
        .logical()
        .begin_callback(host.effects())
        .expect("BUG: native callbacks run outside pure publication");
    run(callback.resumes_carried_work())
}

#[cfg_attr(
    not(target_os = "ios"),
    expect(dead_code, reason = "only iOS fans a wake out across presentations")
)]
pub(super) fn drive_fanout_owner_callback(
    fresh_roots: impl FnOnce(),
    rearm_fresh_roots: impl FnOnce(),
) {
    with_owner_callback(|carried| {
        if carried {
            rearm_fresh_roots();
        } else {
            fresh_roots();
        }
    });
}

#[cfg(any(
    all(test, not(target_os = "android"), not(target_arch = "wasm32")),
    all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    )
))]
fn stop_installed_ui_runtimes() {
    let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    let _ = host.logical().stop_runtimes(host.effects());
}

/// Close admission now; notify once all currently checked-out UI runtime state returns.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn request_quit_notification() {
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.quit_notification == crate::app::runtime::QuitNotification::Active {
            state.quit_notification = crate::app::runtime::QuitNotification::Requested;
        }
    });
    drain_quit_notification();
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn drain_quit_notification() {
    use crate::app::runtime::QuitNotification;
    let ready = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.quit_notification != QuitNotification::Requested
            || state
                .installed_host
                .logical()
                .is_executing()
                .unwrap_or(true)
        {
            return false;
        }
        state.quit_notification = QuitNotification::Notifying;
        true
    });
    if !ready {
        return;
    }
    let mut first_panic = None;
    let cancel = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        super::secondary_window::cancel_pending_secondary_windows,
    ))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, cancel, "pending window cancellation");
    let notification =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(stop_installed_ui_runtimes)).err();
    preserve_first_lifecycle_panic(&mut first_panic, notification, "runtime stop cleanup");
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
}

/// Per-pool grace deadline for joining running background work at full
/// loop-exit teardown. Bounds a hung compute job's ability to wedge process
/// exit; running work that finishes sooner ends shutdown sooner (the
/// deadline is a cap, not a sleep).
#[cfg(not(target_arch = "wasm32"))]
const EXECUTION_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Deadline for the staged SERVICE shutdown (issue #558) that runs just
/// before the pools close: every service is cancelled first, then joined
/// against this one shared deadline — the bounded flush window in which a
/// service writes its final state. A service that ignores cancellation is
/// reported (`DeadlineExceeded`) and force-abandoned by the pool shutdown
/// that follows; it cannot wedge process exit past this deadline plus the
/// per-pool grace above.
#[cfg(not(target_arch = "wasm32"))]
const SERVICE_SHUTDOWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// Full loop-exit teardown: drop every hosted UI runtime, close-request
/// registration, service and execution pool, and the platform clipboard.
///
/// Reached from each backend's loop exit — `run_desktop`/`run_android` after
/// `Platform::run` returns, and iOS from `applicationWillTerminate:`, which is
/// the only pre-exit signal a `UIApplicationMain` loop that never returns can
/// offer.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn teardown_platform_ui_runtime() {
    let displaced = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.owner_thread = None;
        std::mem::replace(&mut state.installed_host, super::InstalledHost::new())
    });
    let mut first_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        displaced.shutdown();
    }))
    .err();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(displaced))).err();
    preserve_first_lifecycle_panic(&mut first_panic, failure, "installed host retirement");

    // Service-lifecycle shutdown (issue #558) BEFORE the pools close: the
    // registry cancels every application service cooperatively and joins
    // each against one shared deadline — the flush window in which a
    // service persists its final state. Ordering is load-bearing: the
    // execution shutdown below cancels the pools' root token and
    // hard-drops any future still running at its next await point, so a
    // service joined AFTER that would lose its flush window every time.
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let report = APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .shutdown_lifecycles(SERVICE_SHUTDOWN_DEADLINE)
        });
        let incomplete: Vec<&'static str> = report
            .entries
            .iter()
            .filter(|entry| {
                entry.outcome != crate::app::lifecycle::ServiceShutdownOutcome::Completed
            })
            .map(|entry| entry.name)
            .collect();
        if incomplete.is_empty() {
            tracing::debug!(
                services = report.entries.len(),
                "application services shut down cleanly"
            );
        } else {
            tracing::warn!(
                services = report.entries.len(),
                ?incomplete,
                "some application services did not complete by the shutdown deadline"
            );
        }
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, failure, "service shutdown");

    // Execution-services shutdown (issue #557): the whole loop is exiting,
    // so stop background admission, cancel outstanding work, join running
    // work bounded by a per-pool grace deadline, and CLEAR the slot — a
    // second platform loop hosted on this same thread later (an embedder
    // running `run_app` twice in one process) must re-resolve fresh
    // services at its own ui_runtime install, not inherit an instance whose
    // admission is permanently closed. Loop-scoped like the clipboard
    // below — hot-restart never reaches this function, so a reinstalled
    // ui_runtime keeps its pools. A teardown path that skips this (panic
    // mid-teardown) still tears the pools down non-blockingly via
    // `ExecutionServices`' own `Drop`.
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .shutdown_execution(EXECUTION_SHUTDOWN_GRACE);
        });
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, failure, "execution shutdown");

    // ADR-0038 §9's install/teardown symmetry: the event loop has exited (this
    // runs from both `run_desktop` and `run_android`, after their respective
    // `platform.run(...)` returns), so drop the platform clipboard now rather
    // than let a live platform resource (arboard on X11 owns a live X11
    // connection) sit pinned behind `AppRuntime` for the rest of the
    // process's life. `Drop for AppRuntime` is the last-resort third clear
    // if this explicit path is ever skipped (a panic mid-teardown, for
    // instance) — see that impl's doc.
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let released = APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            state.clear_platform_clipboard();
            state.clear_redraw_window()
        });
        // Ordinarily `None` already: the window-close path released this pin
        // (`release_redraw_window_for`) while the event loop was still alive,
        // which is the order the platform teardown contract wants. Dropped here
        // outside the TLS borrow for the paths that never closed a window (an
        // OS-level quit with the window still open).
        drop(released);
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, failure, "native shutdown");
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
}

#[path = "owner_dispatch/tests.rs"]
#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
mod owner_dispatch_tests;
