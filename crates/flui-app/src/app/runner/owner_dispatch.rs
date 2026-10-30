use std::rc::Rc;

use flui_runtime::HostInput;
use flui_runtime::owner::RuntimeOperation;
use flui_scheduler::AppLifecycleState;

use super::host::APP_RUNTIME;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::lifecycle_state::preserve_first_lifecycle_panic;

#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct PresentationDispatcher {
    pub(super) owner_thread: std::thread::ThreadId,
    pub(super) address: flui_foundation::PresentationAddress,
}

#[cfg(any(all(test, not(target_os = "android")), target_os = "ios"))]
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

    pub(super) fn host_notice(
        self,
        notice: flui_platform::HostNotice,
    ) -> Result<flui_runtime::owner::Delivery, DispatchError> {
        dispatch_owner_turn_delivery(OwnerTurn::Runtime(
            self,
            RuntimeOperation::HostNotice(notice),
        ))
    }

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
            OwnerError::PreferenceRevisionExhausted
            | OwnerError::OwnerGone
            | OwnerError::Closed
            | OwnerError::Busy
            | OwnerError::InboxFull
            | _ => Self::RuntimeUnavailable,
        }
    }
}

/// Typed presentation observations applied on an owner turn: native input,
/// lifecycle and surface restoration. The payload is
/// `Send` (ADR-0037 §3); execution still goes through exact-address admission
/// on the owner thread, never through an arbitrary cross-thread callback.
///
/// The window facts among them are read from the window's snapshot: its one
/// `WindowHandler::state_changed` subscription compares each new snapshot with the one it
/// saw before and delivers what differs ([`Self::of_state_change`]).
// `pub(in crate::app)` because `RuntimeTask::Event` (also `pub(in crate::app)`, for
// `AppRuntime`'s sake) carries this type in a field the compiler considers
// reachable at that same visibility.
pub(in crate::app) enum RuntimeEvent {
    /// The drawable area or its scale changed.
    Resized(flui_foundation::geometry::WindowMetrics),
    /// Window focus changed. Feeds the `(visible, focused)` ->
    /// `AppLifecycleState` derivation, alongside
    /// [`WindowVisibility`](Self::WindowVisibility).
    WindowFocus(bool),
    /// Reversible native execution eligibility for one presentation.
    WindowExecution(flui_platform::WindowExecutionState),
    /// Addressed logical content-view safe area. A report for a presentation
    /// closed before delivery is dropped, not a panic.
    SafeAreaChanged(flui_foundation::geometry::EdgeInsets),
    /// Whether the window's content can be seen changed
    /// ([`WindowState::is_seen`](flui_platform::WindowState::is_seen)).
    ///
    /// Combined with [`WindowFocus`](Self::WindowFocus) via
    /// the addressed presentation's lifecycle reconciliation. The UI runtime
    /// scheduler derives its aggregate from all live presentations.
    WindowVisibility(bool),
    /// The window's system light/dark appearance changed. Republishes
    /// `MediaQueryData::platform_brightness` through the root media query.
    AppearanceChanged(flui_platform::Brightness),
    /// The host derived a new display for this window from the platform's
    /// snapshot (`display_host`): which display it is on and that display's
    /// refresh period, possibly "unknown". Delivered after every sample and
    /// acted on only when it differs from what the presentation holds.
    DisplayChanged(flui_runtime::owner::DisplayUpdate),
    /// The window's published state changed; the owner reads it when it
    /// delivers this, for the window's `WindowHandle` subscribers and the
    /// content's placement.
    WindowStateChanged,
    /// The pointer entered (`true`) or left (`false`) the window. Leave sweeps
    /// the addressed presentation's hover state — `MouseRegion::on_exit` fires
    /// and the cursor resets; without this a widget hovered at the moment the
    /// cursor crosses the window edge keeps its hover visuals forever. Enter is
    /// a no-op today: the next `CursorMoved` re-primes hover from a fresh hit
    /// test on its own.
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
    Frame(
        super::frame_driver::FrameBinding,
        flui_platform::FrameRequest,
    ),
    ClosePresentation(flui_foundation::PresentationId),
}

/// What one snapshot of a window brought ([`RuntimeEvent::follow`]).
#[derive(Default)]
pub(in crate::app) struct StateFollow {
    /// The facts that changed, in the order the presentation applies them.
    pub(in crate::app) events: Vec<RuntimeEvent>,
    /// The window moved to another display, or its display now gives it
    /// another scale: the platform's display snapshot needs a new sample.
    pub(in crate::app) display_moved: bool,
}

impl RuntimeEvent {
    /// What a window's new snapshot `next` says that `previous` did not, as
    /// the events its presentation applies, in that order: the state
    /// notification first, then each fact that differs — the metrics, the
    /// safe area, the appearance, then the execution state, focus and whether
    /// the content can be seen (each its own lifecycle step, as a host
    /// reports them), and the hover. A snapshot without drawable metrics keeps
    /// the ones before it, so it brings no resize.
    pub(in crate::app) fn of_state_change(
        previous: &flui_platform::WindowState,
        next: &flui_platform::WindowState,
    ) -> Vec<Self> {
        let mut events = vec![Self::WindowStateChanged];
        if next.metrics != previous.metrics
            && let Some(metrics) = next.metrics
        {
            events.push(Self::Resized(metrics));
        }
        if next.safe_area != previous.safe_area {
            events.push(Self::SafeAreaChanged(next.safe_area));
        }
        let brightness = flui_platform::Brightness::from(next.appearance);
        if brightness != flui_platform::Brightness::from(previous.appearance) {
            events.push(Self::AppearanceChanged(brightness));
        }
        if next.execution != previous.execution {
            events.push(Self::WindowExecution(next.execution));
        }
        if next.focused != previous.focused {
            events.push(Self::WindowFocus(next.focused));
        }
        if next.is_seen() != previous.is_seen() {
            events.push(Self::WindowVisibility(next.is_seen()));
        }
        if next.hovered != previous.hovered {
            events.push(Self::WindowHover(next.hovered));
        }
        events
    }

    /// Follow one window's snapshots: what `next` brings against `seen`, the
    /// snapshot the window's presentation saw last, which `next` then
    /// replaces. Before the seed (`seen` empty) it brings nothing: the seed
    /// reads `next` or a newer one. A snapshot without drawable metrics keeps
    /// the last ones as seen, so a later one that has them brings a resize
    /// only when they differ.
    pub(in crate::app) fn follow(
        seen: &mut Option<flui_platform::WindowState>,
        mut next: flui_platform::WindowState,
    ) -> StateFollow {
        let Some(previous) = seen.take() else {
            *seen = Some(next);
            return StateFollow::default();
        };
        let events = Self::of_state_change(&previous, &next);
        let scale = |state: &flui_platform::WindowState| {
            state
                .metrics
                .map(flui_foundation::geometry::WindowMetrics::scale)
        };
        if next.metrics.is_none() {
            next.metrics = previous.metrics;
        }
        let display_moved = previous.display != next.display || scale(&previous) != scale(&next);
        *seen = Some(next);
        StateFollow {
            events,
            display_moved,
        }
    }

    /// Deliver every event, each on its own: a panic in one (a `WindowHandle`
    /// subscriber's, say) does not keep the facts after it from their
    /// presentation. The first panic resumes once all were delivered.
    pub(in crate::app) fn deliver_each(events: Vec<Self>, mut deliver: impl FnMut(Self)) {
        let mut first = None;
        for event in events {
            let failure =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| deliver(event))).err();
            crate::app::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first,
                failure,
                "window state delivery",
            );
        }
        if let Some(payload) = first {
            std::panic::resume_unwind(payload);
        }
    }

    fn deliver(
        self,
        host: &super::InstalledHost,
        address: flui_foundation::PresentationAddress,
        target: &flui_runtime::owner::PresentationDispatcher,
    ) -> Result<flui_runtime::owner::Delivery, flui_runtime::owner::DispatchError> {
        use flui_runtime::owner::{Delivery, InputOutcome, WindowObservation};
        #[cfg(not(any(test, target_os = "android", target_os = "ios")))]
        let _ = address;
        let observation = match self {
            #[cfg(any(test, target_os = "android", target_os = "ios"))]
            Self::PrimarySurfaceRestored => {
                host.displays().reapply_frame_rate(address);
                return host
                    .logical()
                    .frame_dispatcher(address)?
                    .surface_restored(host.effects());
            }
            Self::Resized(metrics) => WindowObservation::Metrics(metrics),
            Self::SafeAreaChanged(insets) => WindowObservation::SafeArea(insets),
            Self::WindowFocus(focused) => WindowObservation::Focus(focused),
            Self::WindowVisibility(visible) => WindowObservation::Visibility(visible),
            Self::WindowExecution(execution) => WindowObservation::Execution(execution),
            Self::WindowHover(inside) => WindowObservation::Hover(inside),
            Self::AppearanceChanged(brightness) => WindowObservation::Brightness(brightness),
            Self::DisplayChanged(update) => WindowObservation::Display(update),
            Self::WindowStateChanged => WindowObservation::State,
        };
        target
            .dispatch(HostInput::Window(observation), host.effects())
            .map(|outcome| match outcome {
                InputOutcome::Queued => Delivery::Queued,
                InputOutcome::Handled | InputOutcome::Unhandled => Delivery::Driven,
            })
    }
}

/// Test fixture for an idle, fresh host. It exercises production preparation
/// and requires completed publication before returning a dispatcher.
#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
pub(super) fn install_platform_ui_runtime(
    ui_runtime: crate::app::ui_runtime::UiRuntime,
    window: &Rc<dyn flui_platform::PlatformWindow>,
) -> PresentationDispatcher {
    let prepared = prepare_replacement_ui_runtime(ui_runtime, Rc::clone(window));
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
    window: Rc<dyn flui_platform::PlatformWindow>,
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
    window: Rc<dyn flui_platform::PlatformWindow>,
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
    window: &Rc<dyn flui_platform::PlatformWindow>,
) -> Result<PresentationDispatcher, super::installed_host::InstallError> {
    let prepared = prepare_ui_runtime_alongside(ui_runtime, Rc::clone(window));
    let dispatcher = prepared.dispatcher();
    prepared
        .submit()
        .outcome()
        .expect("idle test host publishes synchronously")?;
    Ok(dispatcher)
}

pub(super) fn prepare_ui_runtime_alongside(
    ui_runtime: crate::app::ui_runtime::UiRuntime,
    window: Rc<dyn flui_platform::PlatformWindow>,
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
    let native = Rc::clone(window.window());
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
            PublicationError::ForeignOwner
            | PublicationError::UnknownRuntime
            | PublicationError::Closed
            | _ => Self::RuntimeUnavailable,
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
    window: &(impl flui_platform::PlatformWindow + ?Sized),
) {
    super::window_routes::WindowRoutes::of(window)
        .route_input(move |input| dispatch_platform_input(dispatcher, input));
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
    input: flui_platform::PlatformInput,
) -> flui_platform::DispatchEventResult {
    let outcome = current_host(dispatcher.owner_thread).and_then(|host| {
        let _callback = host
            .logical()
            .begin_callback(host.effects())
            .map_err(DispatchError::from)?;
        let result = host
            .logical()
            .presentation_dispatcher(dispatcher.address)
            .and_then(|target| target.dispatch(HostInput::Platform(input), host.effects()))
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

/// Apply the commands application code queued for its windows, when this is the
/// owner thread and no owner callback is running. Applying one may re-enter the
/// window procedure, so it never runs inside user code: a call from inside a
/// callback leaves the queue for the next turn that starts idle. Each backend's
/// owner wake starts such a turn, so a command's own wake applies it.
pub(super) fn apply_queued_window_commands(owner_thread: std::thread::ThreadId) {
    if std::thread::current().id() != owner_thread {
        return;
    }
    let (queues, host) = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        (state.window_commands.clone(), state.installed_host.clone())
    });
    if host.logical().in_callback() {
        return;
    }
    queues.apply_pending();
}

/// Deliver a system notice to every UI runtime this host runs, each on its own
/// owner turn.
pub(super) fn deliver_host_notice(notice: &flui_platform::HostNotice) -> NoticeDelivery {
    // The host's shared memory is reclaimed once, ahead of every runtime's own
    // caches and of every subscriber: the notice reaches subscribers on later
    // owner turns.
    let mut failure = flui_foundation::panic::FirstPanic::new();
    if let flui_platform::HostNotice::MemoryPressure(level) = notice {
        failure.contain(|| reclaim_host_memory(*level));
    }
    let dispatchers = APP_RUNTIME.with(|slot| {
        let Ok(runtime) = slot.try_borrow() else {
            return Vec::new();
        };
        let Some(owner_thread) = runtime.owner_thread else {
            return Vec::new();
        };
        runtime
            .installed_host
            .logical()
            .runtime_ids()
            .map(|ids| {
                ids.into_iter()
                    .map(|id| RuntimeDispatcher { owner_thread, id })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let mut outcome = NoticeDelivery::default();
    for dispatcher in dispatchers {
        match dispatcher.host_notice(notice.clone()) {
            Ok(flui_runtime::owner::Delivery::Driven) => outcome.ran += 1,
            Ok(_) => outcome.queued += 1,
            Err(error) => {
                outcome.refused += 1;
                tracing::debug!(?dispatcher, ?error, "host notice not delivered");
            }
        }
    }
    failure.resume();
    outcome
}

/// Tell every UI runtime in its slot that the session is ending, as the close
/// delivery does (ADR-0259 §6, ADR-0260 §3): each presentation commits its
/// composition and its observers hear `Detached`, so what they publish reaches
/// the flush after it. A runtime that is checked out runs none of its code; its
/// composition is lost, a documented limit. The first panic is contained and
/// reported by the caller.
pub(super) fn deliver_session_end_close() {
    let dispatchers = APP_RUNTIME.with(|slot| {
        let Ok(runtime) = slot.try_borrow() else {
            return Vec::new();
        };
        let Some(owner_thread) = runtime.owner_thread else {
            return Vec::new();
        };
        let logical = runtime.installed_host.logical();
        logical
            .runtime_ids()
            .map(|ids| {
                ids.into_iter()
                    .filter(|id| {
                        logical
                            .runtime_status(*id)
                            .is_ok_and(|status| !status.checked_out)
                    })
                    .map(|id| RuntimeDispatcher { owner_thread, id })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    for dispatcher in dispatchers {
        if let Err(error) = dispatcher.lifecycle(AppLifecycleState::Detached) {
            tracing::debug!(?dispatcher, ?error, "session-end close not delivered");
        }
    }
}

/// How a system notice reached the UI runtimes: on the spot, queued behind a
/// turn that was already running, or not at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct NoticeDelivery {
    /// Runtimes whose subscribers had heard it when the call returned.
    pub(super) ran: usize,
    /// Runtimes whose turn was busy: they hear it later, after the call.
    pub(super) queued: usize,
    /// Runtimes that could not take it.
    pub(super) refused: usize,
}

impl NoticeDelivery {
    /// Whether every subscriber has heard the notice by now.
    pub(super) fn is_complete(self) -> bool {
        self.queued == 0 && self.refused == 0
    }
}

/// Ask the host's shared memory to give back for `level`: the application's
/// receivers and every raster lane's pools.
fn reclaim_host_memory(level: flui_foundation::memory::MemoryPressureLevel) {
    use flui_foundation::memory::ReclaimMemory as _;
    // Cloned out of the cell: a receiver's destructors may touch the host.
    let memory = APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .map(|state| std::sync::Arc::clone(&state.host_memory))
    });
    if let Some(memory) = memory {
        memory.reclaim(level);
    }
}

fn dispatch_owner_turn(turn: OwnerTurn) -> Result<(), DispatchError> {
    dispatch_owner_turn_delivery(turn).map(|_| ())
}

/// [`dispatch_owner_turn`], reporting whether the work ran in this call or was
/// queued behind the turn already running.
fn dispatch_owner_turn_delivery(
    turn: OwnerTurn,
) -> Result<flui_runtime::owner::Delivery, DispatchError> {
    // Window commands queued by application code apply outside the turn's
    // callback guard, since a native call re-enters the window procedure. They
    // apply at the start of a turn. On every host but Android and iOS, a frame
    // turn applies them after its frame: the frame runs inside the platform's
    // paint callback, which holds back the resize a command causes, so a frame
    // drawn after the command would show the old size. The resize then asks
    // for the next frame. Either way the turn drains the queue, so a command
    // whose owner wake failed is applied by the redraw it fell back to.
    let owner_thread = turn.owner_thread();
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let after_frame = false;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let after_frame = matches!(turn, OwnerTurn::Presentation(_, RuntimeTask::Frame(..)));
    if !after_frame {
        apply_queued_window_commands(owner_thread);
    }
    let delivered = deliver_owner_turn(turn);
    if after_frame {
        apply_queued_window_commands(owner_thread);
    }
    delivered
}

/// One owner turn's work, inside its callback guard.
fn deliver_owner_turn(turn: OwnerTurn) -> Result<flui_runtime::owner::Delivery, DispatchError> {
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
                RuntimeTask::Frame(binding, request) => {
                    if binding.address() != dispatcher.address {
                        return Err(DispatchError::StalePresentation);
                    }
                    host.logical()
                        .frame_dispatcher(dispatcher.address)
                        .and_then(|frame| frame.deliver(request, host.effects()))
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

/// How long the framework's part of the exit may take: the flush, the
/// services and the pools, from the moment the flush starts (ADR-0258 §7).
/// Application code that runs before it (destructors, `on_quit`) is not
/// counted.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const EXIT_BUDGET: std::time::Duration = std::time::Duration::from_secs(6);

/// The part of [`EXIT_BUDGET`] the flush registry and the services share:
/// every service is cancelled first, then joined until what is left of it,
/// the window in which a service writes its final state. A service that
/// ignores cancellation is reported (`DeadlineExceeded`) and abandoned by
/// the pool shutdown that follows.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const SERVICE_SHUTDOWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// The one deadline the exit order's framework steps share, taken when the
/// flush starts: the flush and the services end by [`Self::services`], the
/// pools by [`Self::end`], with every second the services did not use.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug)]
struct ExitDeadline {
    services: std::time::Instant,
    end: std::time::Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl ExitDeadline {
    fn start(now: std::time::Instant) -> Self {
        Self {
            services: now + SERVICE_SHUTDOWN_DEADLINE,
            end: now + EXIT_BUDGET,
        }
    }

    /// When the flush and the services end.
    fn services(self) -> std::time::Instant {
        self.services
    }

    /// When the pools end.
    fn end(self) -> std::time::Instant {
        self.end
    }
}

/// How long the whole exit may take, from the main window's shutdown to the
/// exit code, application code included; past it the exit watchdog ends the
/// process with [`EXIT_CODE_HUNG`] (ADR-0258 §7).
#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
pub(crate) const EXIT_WATCHDOG: std::time::Duration = std::time::Duration::from_secs(15);

/// The exit code of an application whose exit did not end 15 seconds after
/// it began, which the exit watchdog then ended (ADR-0258 §7): the code GNU
/// `timeout` returns for a command that ran out of time, so a supervisor reads
/// a hung exit apart from a panic (101) and a crash.
pub const EXIT_CODE_HUNG: i32 = 124;

/// Bounds the whole exit: started when the exit order begins, it ends the
/// process with [`EXIT_CODE_HUNG`] if the exit order has not ended
/// [`EXIT_WATCHDOG`] later. Dropping it ends the watch.
///
/// Before it ends the process it logs the last exit step that ended and
/// writes the flush registry itself, from its own thread, for a second: the
/// registry is shared across threads and runs no application code
/// (ADR-0259 §5).
#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
pub(super) struct ExitWatchdog {
    // Dropped with the watchdog: its thread sees the channel close and ends.
    _stop: std::sync::mpsc::Sender<std::convert::Infallible>,
    reached: std::sync::Arc<std::sync::Mutex<Option<TeardownStep>>>,
}

#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
impl ExitWatchdog {
    /// Start watching the exit this thread is beginning.
    pub(super) fn start() -> Self {
        let flush = APP_RUNTIME.with(|slot| {
            slot.try_borrow()
                .ok()
                .and_then(|state| state.flush_host.clone())
        });
        let reached = std::sync::Arc::new(std::sync::Mutex::new(None));
        let (stop, stopped) = std::sync::mpsc::channel::<std::convert::Infallible>();
        let watched = std::sync::Arc::clone(&reached);
        let spawned = std::thread::Builder::new()
            .name("flui-exit-watchdog".into())
            .spawn(move || {
                if stopped.recv_timeout(EXIT_WATCHDOG)
                    != Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                {
                    return;
                }
                let reached = *watched
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                crate::app::lifecycle::contain_lifecycle_diagnostic(|| {
                    tracing::error!(
                        after = ?reached,
                        "the exit did not end within its watchdog; ending the process"
                    );
                });
                if let Some(flush) = flush {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        flush.flush_within(
                            std::time::Instant::now() + std::time::Duration::from_secs(1),
                        )
                    }));
                }
                flui_native::end_process(EXIT_CODE_HUNG)
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "the exit watchdog did not start; the exit is unbounded");
        }
        Self {
            _stop: stop,
            reached,
        }
    }

    /// Note that `step` of the exit order ended.
    pub(super) fn reached(&self, step: TeardownStep) {
        *self
            .reached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(step);
    }
}

/// One step of the loop's exit teardown, in the order it runs.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TeardownStep {
    /// The main window's intents were refused and its window retired.
    #[cfg_attr(
        any(target_os = "android", target_os = "ios"),
        allow(dead_code, reason = "only the desktop runner has a main window")
    )]
    MainWindow,
    /// Every hosted UI runtime was shut down and dropped.
    UiRuntimes,
    /// The flush registry was written, before the exit deadline's services
    /// part ran out.
    Flush,
    /// The application services were cancelled and joined.
    Services,
    /// The background execution pools were closed.
    Execution,
    /// The platform clipboard and the redraw window were released.
    NativeResources,
}

/// [`teardown_platform_ui_runtime_logged`] for a loop that keeps no log,
/// ending with the exit's outcome: the terminal failure resumes. On iOS,
/// whose `UIApplicationMain` never returns, it is logged instead.
#[cfg(all(not(target_arch = "wasm32"), any(test, target_os = "ios")))]
pub(super) fn teardown_platform_ui_runtime() {
    teardown_platform_ui_runtime_logged(&mut |_| {});
    if let Some(payload) = super::terminal::take_terminal() {
        #[cfg(not(target_os = "ios"))]
        std::panic::resume_unwind(payload);
        #[cfg(target_os = "ios")]
        {
            tracing::error!(
                panic = flui_foundation::panic::payload_text(payload.as_ref()),
                "the application ends with a terminal failure"
            );
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
}

/// Full loop-exit teardown: drop every hosted UI runtime, close-request
/// registration, service and execution pool, and the platform clipboard,
/// telling `log` each step as it ends.
///
/// Reached from each backend's loop exit — `run_desktop`/`run_android` after
/// `Platform::run` returns, and iOS from `applicationWillTerminate:`, which is
/// the only pre-exit signal a `UIApplicationMain` loop that never returns can
/// offer.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn teardown_platform_ui_runtime_logged(log: &mut dyn FnMut(TeardownStep)) {
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
    log(TeardownStep::UiRuntimes);

    // The framework's part of the exit runs against one deadline taken
    // here, after the application code above (ADR-0258 §7). The flush
    // registry is written first, whether or not a failure is held: the
    // services and the pools after it may abandon the IO pool's writes.
    let exit = ExitDeadline::start(std::time::Instant::now());
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let flush = APP_RUNTIME.with(|slot| slot.borrow().flush_host.clone());
        if let Some(flush) = flush {
            let report = flush.flush_within(exit.services());
            if !report.is_complete() {
                tracing::warn!(
                    failed = report.failed.len(),
                    unfinished = report.unfinished.len(),
                    "the flush registry did not write everything by the exit deadline"
                );
            }
        }
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, failure, "exit flush");
    log(TeardownStep::Flush);

    // Service-lifecycle shutdown (issue #558) BEFORE the pools close: the
    // registry cancels every application service cooperatively and joins
    // each against one shared deadline — the flush window in which a
    // service persists its final state. Ordering is load-bearing: the
    // execution shutdown below cancels the pools' root token and
    // hard-drops any future still running at its next await point, so a
    // service joined AFTER that would lose its flush window every time.
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let report =
            APP_RUNTIME.with(|slot| slot.borrow_mut().shutdown_lifecycles(exit.services()));
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
    log(TeardownStep::Services);

    // Execution-services shutdown (issue #557): the whole loop is exiting,
    // so stop background admission, cancel outstanding work, join running
    // work until the exit deadline's end, and CLEAR the slot — a
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
            slot.borrow_mut().shutdown_execution(exit.end());
        });
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, failure, "execution shutdown");
    log(TeardownStep::Execution);

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
            let mut state = slot.borrow_mut();
            state.clear_platform_clipboard();
            // The run's providers are user code: they drop outside the borrow.
            (state.capabilities.take(), state.clear_redraw_window())
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
    log(TeardownStep::NativeResources);
    // Terminal (ADR-0258 §2): held for the exit's end, after the flush above.
    if let Some(payload) = first_panic {
        super::terminal::hold_terminal(payload, "loop exit");
    }
}

#[path = "owner_dispatch/display_tests.rs"]
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod display_tests;
#[path = "owner_dispatch/native_window_tests.rs"]
#[cfg(all(
    test,
    feature = "native",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod native_window_tests;
#[path = "owner_dispatch/notice_tests.rs"]
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod notice_tests;
#[path = "owner_dispatch/tests.rs"]
#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
mod owner_dispatch_tests;
#[path = "owner_dispatch/state_follow_tests.rs"]
#[cfg(test)]
mod state_follow_tests;
