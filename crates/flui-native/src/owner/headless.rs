//! The headless host's loop: the state every position of its table reads, the
//! owner turns a test drives by hand, and the deferred window opens.
//!
//! The headless host has no event loop of its own, so a test is the owner
//! thread: [`HeadlessOwnerTurns`] runs its turns, [`HeadlessExitReevaluation`]
//! its exit-policy re-checks and [`HeadlessDeferredWindowOpens`] the window
//! opens it defers.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Weak, atomic::Ordering},
    thread::{self, ThreadId},
};

use flui_foundation::geometry::Point;
use flui_foundation::{ClaimSlot, DataTransferId, claim_slot};
use flui_platform::InMemoryClipboard;
use flui_platform::data_transfer::{
    DropFeedback, OfferTable, RepresentationDescriptor, TransferActions,
};
use flui_platform::drag::{OsDragEndHook, OsDragProvider, OsDragStart};
use parking_lot::Mutex;

use crate::host::{self, Host};
use crate::owner::call_point::{Field, Once, call, install};
use crate::owner::slot::OwnerSlot;
use crate::platform::HostPlatform;
use crate::platforms::headless::{
    HeadlessWindowPolicy, MockWindow, TextStoreHostFactory, create_mock_window,
};
use crate::{
    error::PlatformError, host_window::HostWindow, owner::DirectOwnerHooks, owner::OpenWindowError,
    owner::OwnerHooks, owner::OwnerPlatform, owner::PendingWindow, owner::ProxyTransport,
    owner::WindowOpen, owner::handlers::PlatformHandlers, platform::Platform,
    platform::PlatformExecutor, platform::PlatformReadyCallback,
};
use flui_platform::{DisplaySnapshot, WindowCapabilities, WindowEvent, WindowId, WindowOptions};

/// The value a deferred (or synchronous) window-open request resolves to —
/// mirrors the winit backend's own private `OpenWindowResult` alias
/// (`owner/winit/control.rs`) so both backends complete a
/// [`ClaimSlot`]/[`PendingWindow`] pair with an identical shape.
type OpenWindowResult = Result<Arc<dyn HostWindow>, OpenWindowError>;

/// What the headless host's table names for its loop (ADR-0151 §10): the
/// displays a platform has before a test lays out its own, and the channel its
/// system notices take. The loop holds the displays' snapshot, which is all
/// the display double keeps, and names no feature.
pub(crate) trait HeadlessHost: 'static {
    /// The snapshot of the displays every platform starts with.
    fn displays() -> DisplaySnapshot;

    /// The channel the system notices a test reports take to the application.
    fn notices() -> Arc<dyn crate::owner::notices::NoticeChannel>;
}

/// The displays position is the loop's: the snapshot it holds.
impl host::Displays<HeadlessLoop> for HeadlessLoop {
    fn snapshot(owner: &HeadlessLoop) -> Result<DisplaySnapshot, PlatformError> {
        Ok(owner.with_state(|state| state.displays.clone()))
    }
}

/// The headless host's loop: the state every position of its table reads.
///
/// The headless host creates no real windows or graphics contexts; it serves
/// unit tests, CI without a display server and benchmarks without rendering.
/// [`HeadlessPlatform`](crate::HeadlessPlatform) is its platform.
pub struct HeadlessLoop {
    pub(crate) signal: Arc<crate::owner::signal::OwnerSignal>,
    wake_failure: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) state: Arc<Mutex<HeadlessState>>,
}

/// What the simulated system keeps of drag and drop (ADR-0211): the offers it
/// holds open and the session in flight. The loop holds it as it holds the
/// rest of the simulated system; `drag::headless` plays the sessions over it
/// and is the platform's one data-transfer source over it.
#[derive(Default)]
pub(crate) struct OsDrags {
    pub(crate) table: OfferTable,
    pub(crate) session: Option<OsDragSession>,
    /// The providers behind offers still open at the system, by id, so a
    /// target may fetch after the drop until it concludes it.
    pub(crate) open: HashMap<DataTransferId, Arc<OwnerSlot<OsDragProvider>>>,
}

/// One drag session in flight at the simulated system.
pub(crate) struct OsDragSession {
    pub(crate) representations: Arc<[RepresentationDescriptor]>,
    pub(crate) allowed: TransferActions,
    pub(crate) provider: Arc<OwnerSlot<OsDragProvider>>,
    /// Told once how the session ended; absent for a drag from another
    /// application.
    pub(crate) on_end: Option<OwnerSlot<OsDragEndHook>>,
    pub(crate) over: Option<DragOver>,
}

/// The window the simulated pointer is over.
pub(crate) struct DragOver {
    pub(crate) window: WindowId,
    pub(crate) offer: DataTransferId,
    pub(crate) feedback: DropFeedback,
    pub(crate) position: Point<f64>,
}

pub(crate) struct HeadlessState {
    /// What the simulated system keeps of drag and drop.
    pub(crate) drags: Arc<Mutex<OsDrags>>,
    /// When a window opened from now on may start a drag session at the
    /// system; `None`, the default: its windows have no drag source
    /// ([`HeadlessPlatform::with_os_drag`](crate::HeadlessPlatform::with_os_drag)).
    pub(crate) os_drag_start: Option<OsDragStart>,
    pub(crate) owner_signal: Weak<crate::owner::signal::OwnerSignal>,
    pub(crate) handlers: PlatformHandlers,
    background_executor: Arc<TestExecutor>,
    pub(crate) clipboard: Arc<InMemoryClipboard>,
    pub(crate) active_window: Option<WindowId>,
    is_running: bool,
    pub(crate) windows: Vec<MockWindow>,
    /// Whether the windows sit on one desktop and answer
    /// [`PlatformWindow::window_at`](flui_platform::PlatformWindow::window_at) ([`HeadlessPlatform::with_window_placement`](crate::HeadlessPlatform::with_window_placement)).
    pub(crate) placement: bool,
    pub(crate) keyboard_layout: String,
    opened_urls: Vec<String>,
    /// Deferred-window-open test mode (see
    /// [`HeadlessPlatform::enable_deferred_window_open`](crate::HeadlessPlatform::enable_deferred_window_open)): when `true`,
    /// `Platform::run` installs `HeadlessDeferredOwnerHooks` instead of
    /// `DirectOwnerHooks`, so `OwnerPlatform::open_window` enqueues into
    /// `pending_opens` and returns `WindowOpen::Pending` instead of
    /// resolving synchronously — exercising the same arm the real winit
    /// owner lane returns for any call after `on_ready`.
    deferred_window_open: bool,
    /// Requests enqueued by `HeadlessDeferredOwnerHooks::open_owner_window`
    /// while `deferred_window_open` is set, awaiting a manual
    /// `HeadlessDeferredWindowOpens::resolve_next` call. FIFO order: the
    /// oldest request resolves first.
    pending_opens: Vec<(ClaimSlot<OpenWindowResult>, WindowOptions)>,
    /// Parked `Platform::request_exit_policy_reevaluation` request (any
    /// thread may set it; coalesced). This mock has no event loop of its
    /// own, so the embedder drives the actual owner-thread re-check via
    /// [`HeadlessExitReevaluation::drive`] — running the
    /// hook on the requesting thread instead would consult the WRONG
    /// thread-local runtime state.
    exit_reevaluation_requested: bool,
    /// What every window opened from now on offers as its text-store host
    /// ([`HeadlessPlatform::with_text_store_host`](crate::HeadlessPlatform::with_text_store_host)); `None`: push-model.
    pub(crate) text_store_host: Option<TextStoreHostFactory>,
    /// What this backend reports implemented ([`HeadlessPlatform::with_input_availability`](crate::HeadlessPlatform::with_input_availability)).
    pub(crate) input_availability: flui_platform::input_capabilities::InputCapabilities,
    /// The attached input devices: presence and device ids from one registry.
    pub(crate) devices: crate::shared::pointer_assembly::DeviceRegistry,
    /// Issues each simulated device's native label, never one twice.
    pub(crate) device_labels: flui_foundation::IdMint<std::num::NonZeroU64>,
    /// The displays this platform reports, which the table lays out
    /// ([`HeadlessHost::displays`]); replaced by
    /// [`HeadlessPlatform::with_displays`](crate::HeadlessPlatform::with_displays) and changed through
    /// [`HeadlessDisplayControls`](crate::platforms::headless::HeadlessDisplayControls).
    pub(crate) displays: DisplaySnapshot,
    /// What every window opened from now on honours
    /// ([`HeadlessPlatform::with_window_capabilities`](crate::HeadlessPlatform::with_window_capabilities)).
    pub(crate) window_capabilities: WindowCapabilities,
    /// What the host clamps or declines for every window opened from now on
    /// ([`HeadlessPlatform::with_window_policy`](crate::HeadlessPlatform::with_window_policy)).
    pub(crate) window_policy: HeadlessWindowPolicy,
    /// The channel system notices take to the host; a wake marks every window
    /// opened here stale first.
    pub(crate) notices: Arc<dyn crate::owner::notices::NoticeChannel>,
    /// How many times the host told this platform it released a session end it
    /// had answered with a block.
    pub(crate) session_end_releases: usize,
    /// What `Platform::preferences` reports: unknown until
    /// [`HeadlessPlatform::with_preferences`](crate::HeadlessPlatform::with_preferences) or [`HeadlessPreferences::set`](crate::platforms::headless::HeadlessPreferences::set).
    pub(crate) preferences: flui_platform::SystemPreferences,
    /// The failure the next `Platform::preferences` read reports instead of a
    /// value ([`HeadlessPreferences::fail_next_read`](crate::platforms::headless::HeadlessPreferences::fail_next_read)).
    pub(crate) preference_failure: Option<String>,
}

impl HeadlessLoop {
    /// The loop of host `H`, whose table makes the displays and the notice
    /// channel.
    pub(crate) fn new<H: HeadlessHost>() -> Self {
        let state = HeadlessState {
            drags: Arc::default(),
            os_drag_start: None,
            owner_signal: Weak::new(),
            handlers: PlatformHandlers::new(),
            background_executor: Arc::new(TestExecutor::new("background")),
            clipboard: Arc::new(InMemoryClipboard::new()),
            active_window: None,
            is_running: false,
            windows: Vec::new(),
            placement: false,
            keyboard_layout: "en-US".to_string(),
            opened_urls: Vec::new(),
            deferred_window_open: false,
            pending_opens: Vec::new(),
            exit_reevaluation_requested: false,
            text_store_host: None,
            input_availability: default_input_availability(),
            devices: crate::shared::pointer_assembly::DeviceRegistry::default(),
            device_labels: flui_foundation::IdMint::new(),
            displays: H::displays(),
            window_capabilities: WindowCapabilities::ALL,
            window_policy: HeadlessWindowPolicy::default(),
            notices: H::notices(),
            session_end_releases: 0,
            preferences: flui_platform::SystemPreferences::default(),
            preference_failure: None,
        };

        let wake_failure = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let failure = Arc::clone(&wake_failure);
        let platform = Self {
            wake_failure,
            signal: crate::owner::signal::OwnerSignal::new(Arc::new(move || {
                if failure.swap(false, Ordering::AcqRel) {
                    Err(PlatformError::EventLoop {
                        message: "injected headless owner wake failure".into(),
                    })
                } else {
                    Ok(())
                }
            })),
            state: Arc::new(Mutex::new(state)),
        };
        platform.state.lock().owner_signal = Arc::downgrade(&platform.signal);
        platform
    }

    pub(crate) fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HeadlessState) -> R,
    {
        let mut state = self.state.lock();
        f(&mut state)
    }
}

/// Call the application's window-event handler with `event` through the call
/// point, with the state lock released: the handler re-enters the embedder's
/// runtime, which may open a window, install another handler or quit
/// (ADR-0039). Its panic reaches the caller. An event emitted from inside the
/// handler finds the slot empty and is dropped, as on winit.
pub(crate) fn emit_window_event(state: &Mutex<HeadlessState>, event: WindowEvent) {
    call(
        &Field::new(state, |state| &mut state.handlers.window_event),
        |handler| handler(event),
    )
    .or_resume(());
}

/// Stop the platform and run the application's quit handler, once: it is
/// retired after its call. The signal, when there is one, closes first.
fn finish_quit(state: &Mutex<HeadlessState>) {
    let signal = {
        let mut state = state.lock();
        state.is_running = false;
        state.owner_signal.upgrade()
    };
    if let Some(signal) = signal {
        signal.close();
    }
    call(
        &Once(Field::new(state, |state| &mut state.handlers.quit)),
        |quit| quit(),
    )
    .or_resume(());
}

/// Ask the application's exit-policy hook whether the platform may quit now
/// that no window is tracked, and quit when it may. No hook never quits.
///
/// The hook runs with the state lock released: it drops removed UI runtime
/// state, whose destructors may call back into this platform (a dispose hook
/// opening another window, say). Emptiness is checked before the hook and
/// again before quitting, so a window opened meanwhile — by the hook itself
/// too — keeps the platform running. `true` when it quit.
pub(crate) fn consult_exit_policy(state: &Mutex<HeadlessState>) -> bool {
    if !state.lock().windows.is_empty() {
        return false;
    }
    // `false` when vacant, not `true`: headless's own pre-#555 default is "no
    // hook -> never quit" (matching every headless test/consumer that
    // predates this mechanism, none of which expects closing a mock window
    // to spontaneously call `quit`) -- the OPPOSITE of winit's "no hook ->
    // exit unconditionally" default, which matches real native pre-#555
    // window-close behavior instead. The two backends' defaults are
    // deliberately different; this is not a typo.
    let allowed = call(
        &Field::new(state, |state| &mut state.handlers.exit_policy),
        |hook| hook(),
    )
    .or_resume(false);
    if !allowed || !state.lock().windows.is_empty() {
        return false;
    }
    finish_quit(state);
    true
}

/// The headless platform's owner-turn handles: taken before [`Platform::run`]
/// consumes it.
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// Obtain the manual owner-turn driver before `run` consumes this platform.
    #[must_use]
    pub fn owner_turns(&self) -> HeadlessOwnerTurns {
        let owner = self.owner();
        HeadlessOwnerTurns {
            signal: Arc::downgrade(&owner.signal),
            wake_failure: Arc::downgrade(&owner.wake_failure),
            state: Arc::downgrade(&owner.state),
            owner_affinity: std::marker::PhantomData,
        }
    }

    /// A probe/drive handle for `Platform::request_exit_policy_reevaluation`
    /// requests, valid across `Platform::run` (which consumes the boxed
    /// platform) — the same take-a-handle-before-run shape as
    /// [`Self::enable_deferred_window_open`]. See
    /// [`HeadlessExitReevaluation`].
    #[must_use]
    pub fn exit_reevaluation(&self) -> HeadlessExitReevaluation {
        HeadlessExitReevaluation {
            state: Arc::downgrade(&self.owner().state),
        }
    }

    /// Switches this platform's `OwnerPlatform::open_window` behavior from
    /// the default synchronous `Ready` resolution to the deferred
    /// `Pending` arm the real winit owner lane returns for any call after
    /// `on_ready` — a test-only seam for exercising Pending-arm completion
    /// paths (e.g. `flui-app`'s `open_secondary_window`) without a real
    /// event loop. Must be called before [`Platform::run`]; the returned
    /// [`HeadlessDeferredWindowOpens`] resolves each request in FIFO order,
    /// on demand, via [`HeadlessDeferredWindowOpens::resolve_next`].
    #[must_use]
    pub fn enable_deferred_window_open(&self) -> HeadlessDeferredWindowOpens {
        self.owner()
            .with_state(|state| state.deferred_window_open = true);
        HeadlessDeferredWindowOpens {
            state: Arc::downgrade(&self.owner().state),
        }
    }
}

impl std::fmt::Debug for HeadlessLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessLoop").finish_non_exhaustive()
    }
}

impl host::Loop for HeadlessLoop {
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        on_ready: PlatformReadyCallback,
    ) -> Result<(), PlatformError> {
        tracing::info!("Starting headless platform (no event loop)");

        let state_handle = Arc::clone(&self.state);
        let signal = Arc::clone(&self.signal);
        signal.bind_owner();
        struct BootstrapGuard(Option<Arc<crate::owner::signal::OwnerSignal>>);
        impl Drop for BootstrapGuard {
            fn drop(&mut self) {
                if let Some(signal) = self.0.take() {
                    signal.close();
                }
            }
        }
        let mut bootstrap_guard = BootstrapGuard(Some(Arc::clone(&signal)));

        self.with_state(|state| {
            state.is_running = true;
        });

        // Default: no owner lane on this backend, every
        // `OwnerPlatform::open_window` call creates directly and is always
        // `Ready` (ADR-0039 §1). "For the loop's life" means "until the
        // value is dropped" here, since `run` returns immediately (ADR-0039
        // §1) -- there is no later point on this thread to defer to. Test
        // mode (`enable_deferred_window_open`) opts into the `Pending` arm
        // instead, so a probe can exercise the same completion path the real
        // winit owner lane exercises without a real event loop.
        let deferred_window_open = state_handle.lock().deferred_window_open;
        let owner_thread = thread::current().id();

        let hooks: Arc<dyn OwnerHooks> = if deferred_window_open {
            Arc::new(HeadlessDeferredOwnerHooks {
                state: state_handle,
                signal: Arc::clone(&signal),
                owner_thread,
            })
        } else {
            Arc::new(DirectOwnerHooks::with_signal(
                Arc::clone(&platform),
                Arc::clone(&signal),
            ))
        };

        // In headless mode, just call on_ready and return immediately. A
        // fallible bootstrap has nowhere else to go on this backend since
        // there is no loop to keep running with a half-built app --
        // propagate straight out of `run`.
        if let Err(error) = on_ready(OwnerPlatform::new(platform, hooks)) {
            signal.close();
            return Err(PlatformError::bootstrap(error));
        }
        signal.start().map_err(|error| PlatformError::EventLoop {
            message: error.to_string(),
        })?;

        bootstrap_guard.0 = None;
        tracing::info!("Headless platform ready");
        Ok(())
    }

    fn quit(&self) {
        self.signal.close();
        tracing::info!("Quitting headless platform");
        // The handler and its captures' destructors may re-enter the
        // platform. Matches the close/reevaluation quit paths.
        finish_quit(&self.state);
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        install(
            &Field::new(&*self.state, |state| &mut state.handlers.exit_policy),
            hook,
        );
    }

    fn request_exit_policy_reevaluation(&self) {
        // Park only (coalesced): this mock has no event loop, so the
        // owner-thread half runs when the embedder calls
        // `HeadlessExitReevaluation::drive` — see that method's doc.
        self.with_state(|state| state.exit_reevaluation_requested = true);
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        install(
            &Field::new(&*self.state, |state| &mut state.handlers.quit),
            callback,
        );
    }

    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        // No idle wait to bound: the test drives every owner turn.
        drop(hook);
    }
}

impl host::Services for HeadlessLoop {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.background_executor.clone())
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        Ok(PathBuf::from("/mock/app/path"))
    }

    fn open_url(&self, url: &str) {
        self.with_state(|state| {
            state.opened_urls.push(url.to_string());
        });
    }

    // Nothing is shown, so no path is revealed or opened and no prompt runs.
    fn reveal_path(&self, path: &std::path::Path) {
        let _ = path;
    }

    fn open_path(&self, path: &std::path::Path) {
        let _ = path;
    }

    fn prompt_for_paths(
        &self,
        options: crate::PathPromptOptions,
    ) -> crate::Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        let _ = options;
        crate::Task::ready(Ok(None))
    }

    fn prompt_for_new_path(
        &self,
        directory: &std::path::Path,
        suggested_name: Option<&str>,
    ) -> crate::Task<Result<Option<PathBuf>, PlatformError>> {
        let _ = (directory, suggested_name);
        crate::Task::ready(Ok(None))
    }

    fn compositor_name(&self) -> &'static str {
        ""
    }
}

/// What the headless backend implements unless a test says otherwise.
const fn default_input_availability() -> flui_platform::input_capabilities::InputCapabilities {
    use flui_platform::input_capabilities::Availability::Native;
    flui_platform::input_capabilities::InputCapabilities::builder()
        .pen(Native)
        .precise_scroll(Native)
        .trackpad_gestures(Native)
        .input_regions(Native)
        .build()
}

/// [`OwnerHooks`] for the headless backend's deferred-open test mode
/// (installed by `Platform::run` in place of `DirectOwnerHooks` when
/// [`HeadlessPlatform::enable_deferred_window_open`](crate::HeadlessPlatform::enable_deferred_window_open) was called first).
/// Exercises the `WindowOpen::Pending` arm of the owner-lane contract
/// without needing a real winit event loop: every `open_owner_window` call
/// enqueues a [`ClaimSlot`] request into `HeadlessState::pending_opens`
/// instead of creating synchronously, and
/// [`HeadlessDeferredWindowOpens::resolve_next`] completes the oldest one on
/// demand.
struct HeadlessDeferredOwnerHooks {
    signal: Arc<crate::owner::signal::OwnerSignal>,
    state: Arc<Mutex<HeadlessState>>,
    owner_thread: ThreadId,
}

impl OwnerHooks for HeadlessDeferredOwnerHooks {
    fn on_wake(
        &self,
        callback: Box<dyn FnMut() + Send>,
    ) -> Result<(), crate::WakeRegistrationError> {
        self.signal.register(callback)
    }

    fn open_owner_window(&self, options: WindowOptions) -> Result<WindowOpen, OpenWindowError> {
        // No wake-worthy event loop to notify on abandonment (this backend
        // never parks a real loop on the request) -- see `claim_slot`'s own
        // doc for why a no-op wake is the correct choice for a generic
        // caller like this one.
        if !self.signal.accepting() {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }
        let (slot, handle) = claim_slot::<OpenWindowResult>(Arc::new(|| {}));
        self.state.lock().pending_opens.push((slot, options));
        Ok(WindowOpen::Pending(PendingWindow::new(
            handle,
            self.owner_thread,
        )))
    }

    fn transport(&self) -> Arc<dyn ProxyTransport> {
        Arc::new(crate::owner::signal::SignalTransport::new(&self.signal))
    }
}

/// Deterministic headless owner-turn driver. `run` returning does not close it.
#[derive(Clone)]
pub struct HeadlessOwnerTurns {
    wake_failure: Weak<std::sync::atomic::AtomicBool>,
    owner_affinity: std::marker::PhantomData<std::rc::Rc<()>>,
    signal: Weak<crate::owner::signal::OwnerSignal>,
    state: Weak<Mutex<HeadlessState>>,
}
static_assertions::assert_not_impl_any!(HeadlessOwnerTurns: Send, Sync);
impl std::fmt::Debug for HeadlessOwnerTurns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessOwnerTurns").finish_non_exhaustive()
    }
}
impl HeadlessOwnerTurns {
    /// Inject one physical notification failure for deterministic recovery tests.
    /// The following post succeeds normally; this does not close the owner.
    pub fn fail_next_wake(&self) {
        if let Some(failure) = self.wake_failure.upgrade() {
            failure.store(true, Ordering::Release);
        }
    }
    /// Perform one owner turn, including a pending explicit quit. No worker callback execution.
    pub fn drive(&self) {
        let Some(signal) = self.signal.upgrade() else {
            return;
        };
        assert_eq!(
            signal.owner(),
            thread::current().id(),
            "headless owner turn must run on its owner"
        );
        if signal.drive() {
            signal.close();
            if let Some(state) = self.state.upgrade() {
                finish_quit(&state);
            }
        }
    }
}

/// Probe/drive handle for parked
/// `Platform::request_exit_policy_reevaluation` requests on the headless
/// mock — obtained via [`HeadlessPlatform::exit_reevaluation`](crate::HeadlessPlatform::exit_reevaluation) BEFORE
/// `Platform::run` consumes the platform. The mock has no event loop of
/// its own, so the owner-thread half of a re-evaluation request (the
/// winit backend's `process_control` arm) is driven explicitly by the
/// embedding test through [`drive`](Self::drive).
pub struct HeadlessExitReevaluation {
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessExitReevaluation {
    // Manual impl: `HeadlessState` carries no blanket `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessExitReevaluation")
            .field("platform_alive", &(self.state.upgrade().is_some()))
            .finish()
    }
}

impl HeadlessExitReevaluation {
    /// Whether a re-evaluation request is parked, awaiting
    /// [`drive`](Self::drive) — the bounded-wait probe for tests
    /// observing a request that originated on a worker thread. `false`
    /// once the platform is gone.
    #[must_use]
    pub fn requested(&self) -> bool {
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let requested = state.lock().exit_reevaluation_requested;
        drop(state);
        requested
    }

    /// Runs the owner-thread half of a parked re-evaluation request: if
    /// one is parked AND no window is tracked, consult the exit-policy
    /// hook exactly as the window-close path does
    /// (`MockWindow::notify_closed`, including its "no hook -> never
    /// quit" headless default and its call outside the lock) and quit if
    /// the hook now allows it. Returns `true` iff a quit was actually
    /// issued.
    ///
    /// Call on the thread that owns the platform's runtime state (in a
    /// test: the test thread) — the whole reason requests park instead of
    /// running inline is that the requesting worker thread's own
    /// thread-local runtime state is the WRONG state for the hook to
    /// consult.
    ///
    /// # Why the request is consumed even when a window remains
    ///
    /// This reads like the lost-wakeup shape — clear the flag, then discover
    /// the work cannot be done — and it is not, because the request *is*
    /// answered: it asks "can we exit now?", a tracked window makes that a
    /// definite no, and the only thing that empties the registry is a close,
    /// which consults the hook on its own path. There is no state reachable
    /// from here where dropping the request loses an exit.
    ///
    /// That argument rests on one thing: `windows` being non-empty means a
    /// window is genuinely open. A deferred close — parked, so the window is
    /// still tracked but no longer open — would break it, and then this must
    /// become check-then-consume. Issue #937 owns that, and this note is here
    /// so the reasoning is not re-derived as a defect in the meantime.
    pub fn drive(&self) -> bool {
        let Some(platform_state) = self.state.upgrade() else {
            return false;
        };
        {
            let mut state = platform_state.lock();
            if !state.exit_reevaluation_requested {
                return false;
            }
            state.exit_reevaluation_requested = false;
        }
        // The hook is not one-shot: a veto leaves it in place for the next
        // consult (same rule as `MockWindow::notify_closed`).
        consult_exit_policy(&platform_state)
    }
}

/// Test handle returned by [`HeadlessPlatform::enable_deferred_window_open`](crate::HeadlessPlatform::enable_deferred_window_open)
/// — resolves the oldest still-pending deferred window-open request,
/// completing the [`PendingWindow`] a `HeadlessDeferredOwnerHooks` call
/// returned. `Weak`: this handle must never keep the platform state alive
/// past the platform's own lifetime (same rationale as
/// `MockWindow::platform_state`).
pub struct HeadlessDeferredWindowOpens {
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessDeferredWindowOpens {
    // Manual impl: `HeadlessState` carries no blanket `Debug` (its own
    // `windows`/`handlers` fields don't derive it either).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDeferredWindowOpens")
            .field("platform_alive", &(self.state.upgrade().is_some()))
            .finish()
    }
}

impl HeadlessDeferredWindowOpens {
    /// Resolves the oldest still-pending deferred open by constructing a
    /// real mock window and delivering it through that request's
    /// [`ClaimSlot`] — the same registration/activation/`Created`-event
    /// bookkeeping the synchronous `Platform::open_window` path performs
    /// (`create_mock_window`), so a test cannot tell the two paths apart
    /// except by timing. Returns the same window handle that was delivered
    /// (a real, live [`PlatformWindow`](flui_platform::PlatformWindow) a test can drive further — including
    /// a genuine `.close()` — since the whole point of this test seam is
    /// that nothing else in the framework hands that handle back once a
    /// request defers), or `None` if there was nothing pending (the platform
    /// was dropped, or every request so far has already been resolved).
    pub fn resolve_next(&self) -> Option<Arc<dyn HostWindow>> {
        let platform_state = self.state.upgrade()?;

        // Pop the request and build its window under one lock acquisition,
        // then deliver OUTSIDE the lock (same discipline `MockWindow::
        // notify_closed` uses) -- `deliver`'s injected wake callback is a
        // no-op here, but a future caller reusing this pattern must not
        // rely on that.
        let (slot, window) = {
            let mut state = platform_state.lock();
            if state.pending_opens.is_empty() {
                return None;
            }
            let (slot, options) = state.pending_opens.remove(0);
            let window = create_mock_window(&mut state, Arc::downgrade(&platform_state), options);
            (slot, window)
        };
        emit_window_event(&platform_state, WindowEvent::Created(window.id()));

        // The requester may have already abandoned the request (dropped its
        // `PendingWindow` before this call) -- `deliver`'s `Err` hands the
        // freshly created window back so it is dropped/unwound instead of
        // silently leaking a window nothing will ever claim. Either way this
        // method still returns the SAME window handle to its own caller: a
        // test driving `resolve_next` directly (rather than through a real
        // `PendingWindow`) legitimately wants it regardless of whether some
        // OTHER requester abandoned their own claim to it.
        if let Err(_unclaimed) = slot.deliver(Ok(Arc::clone(&window))) {
            tracing::debug!(
                "resolved a deferred window open whose PendingWindow was \
                 already abandoned by the requester; the created mock window \
                 is still handed back to resolve_next's own caller"
            );
        }
        Some(window)
    }
}

/// Test executor that runs tasks immediately
struct TestExecutor {
    name: String,
}

impl TestExecutor {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl PlatformExecutor for TestExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        tracing::trace!(executor = %self.name, "Running task immediately");
        task();
    }

    fn is_on_executor(&self) -> bool {
        true // Always on executor in test mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Loop as _;

    #[test]
    fn explicit_quit_reenters_and_drops_its_callback_outside_the_state_lock() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct CallbackDrop {
            platform: std::sync::Weak<HeadlessLoop>,
            released: Arc<AtomicBool>,
        }
        impl Drop for CallbackDrop {
            fn drop(&mut self) {
                let platform = self
                    .platform
                    .upgrade()
                    .expect("platform still owned by test");
                // Record rather than assert in Drop: on RED the callback body
                // itself panics, so a second panic here would abort the process.
                self.released
                    .store(platform.state.try_lock().is_some(), Ordering::SeqCst);
            }
        }
        let platform = Arc::new(HeadlessLoop::new::<crate::hosts::headless::Headless>());
        platform.state.lock().is_running = true;
        let calls = Arc::new(AtomicUsize::new(0));
        let released = Arc::new(AtomicBool::new(false));
        let callback_drop = CallbackDrop {
            platform: Arc::downgrade(&platform),
            released: Arc::clone(&released),
        };
        let callback_platform = Arc::clone(&platform);
        let callback_calls = Arc::clone(&calls);
        platform.on_quit(Box::new(move || {
            let _ = &callback_drop;
            assert!(
                callback_platform.state.try_lock().is_some(),
                "quit callback must run outside the state lock"
            );
            callback_calls.fetch_add(1, Ordering::SeqCst);
            callback_platform.quit();
        }));
        platform.quit();
        platform.quit();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the registered callback is consumed before reentry"
        );
        assert!(
            released.load(Ordering::SeqCst),
            "captured callback data drops outside the state lock"
        );
        assert!(!platform.state.lock().is_running);
    }
}
