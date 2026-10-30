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
    rc::Rc,
    sync::{Arc, Weak, atomic::Ordering},
    thread::{self, ThreadId},
};

use crate::host::MemoryClipboard;
use flui_foundation::geometry::{Bounds, Point, Size};
use flui_foundation::{ClaimSlot, DataTransferId, claim_slot};
use flui_platform::data_transfer::{DropFeedback, RepresentationDescriptor, TransferActions};
use flui_platform::drag::{OsDragEndHook, OsDragProvider, OsDragStart, PlatformDragSource};
use parking_lot::Mutex;

use crate::host::{self, Host};
use crate::owner::app_delivery::AppDelivery;
use crate::owner::slot::OwnerSlot;
use crate::platform::HostPlatform;
use crate::{
    error::PlatformError, host_window::HostWindow, owner::DirectOwnerHooks, owner::OpenWindowError,
    owner::OwnerHooks, owner::OwnerPlatform, owner::PendingWindow, owner::ProxyTransport,
    owner::WindowOpen, owner::handlers::OpenWindows, owner::window_delivery::WindowDelivery,
    platform::AppHandler, platform::Platform, platform::PlatformExecutor,
};
use flui_platform::{DisplayId, DisplaySnapshot, WindowHandler, WindowId, WindowOptions};

/// The value a deferred (or synchronous) window-open request resolves to —
/// mirrors the winit backend's own private `OpenWindowResult` alias
/// (`owner/winit/control.rs`) so both backends complete a
/// [`ClaimSlot`]/[`PendingWindow`] pair with an identical shape.
type OpenWindowResult = Result<crate::owner::OpenedWindow, OpenWindowError>;

/// Builds the text-store host a headless window offers
/// ([`HeadlessPlatform::with_text_store_host`](crate::HeadlessPlatform::with_text_store_host)).
/// Shared with every window, so `Send + Sync`; what it builds is owner-thread
/// state.
pub(crate) type TextStoreHostFactory =
    Arc<dyn Fn() -> std::rc::Rc<dyn flui_platform::text_store::TextStoreHost> + Send + Sync>;

/// What the headless host's table names for its loop (ADR-0151 §10): the
/// displays a platform has before a test lays out its own, the channel its
/// system notices take and the simulated pointer devices. The loop holds the
/// displays' snapshot, which is all the display double keeps, and names no
/// feature.
pub(crate) trait HeadlessHost: 'static {
    /// The snapshot of the displays every platform starts with.
    fn displays() -> DisplaySnapshot;

    /// The channel the system notices a test reports take to the application.
    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn crate::owner::notices::NoticeChannel>;

    /// The simulated system's pointer devices, none attached.
    fn devices() -> Box<dyn SimulatedDevices>;

    /// The table the simulated system mints its drag offers in, empty.
    fn offers() -> Box<dyn SimulatedOffers>;

    /// The drag source a window opened now in `state`'s system hands out:
    /// none until a test lets the windows start sessions.
    fn drag_source(state: &HeadlessState) -> Option<Arc<dyn PlatformDragSource>>;
}

/// The pointer devices of the simulated system, which the input feature
/// keeps ([`HeadlessHost::devices`]) and the loop holds with the rest of that
/// system: a test attaches and removes them, and a window reads which kinds
/// are present when it opens.
pub(crate) trait SimulatedDevices: Send {
    /// Attach a device of `kind` at `time`: the change every open window
    /// hears, or `None` once device identities are exhausted.
    fn attach(
        &mut self,
        kind: flui_platform::pointer::PointerKind,
        time: flui_platform::EventTime,
    ) -> Option<flui_platform::pointer::PointerDeviceChange>;

    /// Remove the device `id` at `time`: the change every open window hears,
    /// or `None` when no such device is attached.
    fn remove(
        &mut self,
        id: flui_platform::pointer::DeviceId,
        time: flui_platform::EventTime,
    ) -> Option<flui_platform::pointer::PointerDeviceChange>;

    /// The kinds of the devices attached now.
    fn present(&self) -> flui_platform::input_capabilities::PointerKindSet;
}

/// The table the simulated system mints its drag offers in, which the drag
/// feature keeps ([`HeadlessHost::offers`]) and the loop holds with the rest
/// of the system's drag state ([`OsDrags`]).
pub(crate) trait SimulatedOffers: Send {
    /// Mint a live offer announcing `representations`.
    fn mint(&mut self, representations: Arc<[RepresentationDescriptor]>) -> DataTransferId;

    /// Retire live offer `id`, so it is stale for good; a stale `id` is a
    /// no-op.
    fn retire(&mut self, id: DataTransferId);
}

/// The displays position is the loop's: the snapshot it holds.
impl host::Displays<HeadlessLoop> for HeadlessLoop {
    fn snapshot(owner: &HeadlessLoop) -> Result<DisplaySnapshot, PlatformError> {
        owner.with_state(|state| match state.display_failure.take() {
            Some(message) => Err(PlatformError::Displays { message }),
            None => Ok(state.displays.clone()),
        })
    }
}

/// The windows the headless platform tracks, which the window feature opens
/// and keeps (ADR-0151 §10).
///
/// The loop holds them without naming them: the host builds the window
/// feature's object and hands it to [`HeadlessLoop::new`]. The loop asks it
/// whether a window is left (the exit policy) and opens a deferred window
/// through it; the simulated devices and drag sessions reach a window's
/// callbacks through it.
pub(crate) trait TrackedWindows: OpenWindows {
    /// Builds a window delivering to `handler` under the locked `state` and
    /// tracks it; `platform` is the lock `state` came from, which the window
    /// keeps to close. The caller delivers the capability snapshot once the
    /// lock is released.
    fn open(
        self: Arc<Self>,
        state: &mut HeadlessState,
        platform: Weak<Mutex<HeadlessState>>,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Rc<dyn HostWindow>;

    /// Keep `window`, which a deferred open built, until it closes: its
    /// opener gets its identity, never the window (ADR-0254 §4).
    fn keep_deferred(&self, window: &Rc<dyn HostWindow>);

    /// What every open window delivers to its handler, in the order they
    /// opened.
    fn deliveries_of_all(&self) -> Vec<Arc<WindowDelivery>>;

    /// What the open window `id` names delivers to its handler.
    fn delivery_of(&self, id: WindowId) -> Option<Arc<WindowDelivery>>;
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
pub(crate) struct OsDrags {
    pub(crate) table: Box<dyn SimulatedOffers>,
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
    /// What the platform delivers to the application's handler, which `run`
    /// binds.
    pub(crate) app: Arc<AppDelivery>,
    /// Called when a test changes the keyboard layout.
    pub(crate) keyboard_layout_changed: Option<Box<dyn FnMut() + Send>>,
    background_executor: Arc<TestExecutor>,
    pub(crate) clipboard: Arc<dyn crate::host::SharedClipboard>,
    is_running: bool,
    /// The windows the platform tracks: the window feature's.
    pub(crate) windows: Arc<dyn TrackedWindows>,
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
    pending_opens: Vec<PendingOpen>,
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
    /// The attached input devices, as the table made them
    /// ([`HeadlessHost::devices`]).
    pub(crate) devices: Box<dyn SimulatedDevices>,
    /// The displays this platform reports, which the table lays out
    /// ([`HeadlessHost::displays`]); replaced by
    /// [`HeadlessPlatform::with_displays`](crate::HeadlessPlatform::with_displays) and changed through
    /// [`HeadlessDisplayControls`](crate::HeadlessDisplayControls).
    pub(crate) displays: DisplaySnapshot,
    /// The channel system notices take to the host; a wake marks every window
    /// opened here stale first.
    pub(crate) notices: Arc<dyn crate::owner::notices::NoticeChannel>,
    /// How many times the host told this platform it released a session end it
    /// had answered with a block.
    pub(crate) session_end_releases: usize,
    /// What `Platform::preferences` reports: unknown until
    /// [`HeadlessPlatform::with_preferences`](crate::HeadlessPlatform::with_preferences) or [`HeadlessPreferences::set`](crate::HeadlessPreferences::set).
    pub(crate) preferences: flui_platform::SystemPreferences,
    /// The failure the next `Platform::preferences` read reports instead of a
    /// value ([`HeadlessPreferences::fail_next_read`](crate::HeadlessPreferences::fail_next_read)).
    pub(crate) preference_failure: Option<String>,
    /// The failure the next `Platform::display_snapshot` read reports instead
    /// of the snapshot ([`HeadlessDisplayControls::fail_next_read`](crate::HeadlessDisplayControls::fail_next_read)).
    pub(crate) display_failure: Option<String>,
    /// What makes the drag source of a window opened now, as the table names
    /// it ([`HeadlessHost::drag_source`]).
    drag_source: fn(&HeadlessState) -> Option<Arc<dyn PlatformDragSource>>,
}

/// What the simulated system tells a window that opens in it: the displays
/// it lays out and the drag source it hands out, read from the system's state
/// so the window names neither feature.
impl HeadlessState {
    /// The display a new window opens on: the primary, or the first when
    /// none is.
    pub(crate) fn window_display(&self) -> Option<DisplayId> {
        self.displays
            .primary()
            .or_else(|| self.displays.displays().first())
            .map(|display| display.id)
    }

    /// Whether the system holds display `id`.
    pub(crate) fn holds_display(&self, id: DisplayId) -> bool {
        self.displays.get(id).is_some()
    }

    /// The bounds of display `id` in the logical pixels of its own scale.
    pub(crate) fn display_bounds(&self, id: DisplayId) -> Option<Bounds<f64>> {
        let display = self.displays.get(id)?;
        let scale = display.scale_factor;
        let device = display.bounds;
        Some(Bounds::new(
            Point::new(
                f64::from(device.origin.x) / scale,
                f64::from(device.origin.y) / scale,
            ),
            Size::new(
                f64::from(device.size.width) / scale,
                f64::from(device.size.height) / scale,
            ),
        ))
    }

    /// The drag source a window opened now hands out.
    pub(crate) fn window_drag_source(&self) -> Option<Arc<dyn PlatformDragSource>> {
        (self.drag_source)(self)
    }
}

impl HeadlessLoop {
    /// The loop of host `H`, whose table makes the displays, the notice
    /// channel and the simulated devices, and whose windows are `windows`, the
    /// window feature's.
    pub(crate) fn new<H: HeadlessHost>(windows: Arc<dyn TrackedWindows>) -> Self {
        let app = Arc::new(AppDelivery::new());
        let state = HeadlessState {
            drags: Arc::new(Mutex::new(OsDrags {
                table: H::offers(),
                session: None,
                open: HashMap::new(),
            })),
            os_drag_start: None,
            owner_signal: Weak::new(),
            notices: H::notices(&app),
            app,
            keyboard_layout_changed: None,
            background_executor: Arc::new(TestExecutor::new("background")),
            clipboard: Arc::new(MemoryClipboard::new()),
            is_running: false,
            windows,
            keyboard_layout: "en-US".to_string(),
            opened_urls: Vec::new(),
            deferred_window_open: false,
            pending_opens: Vec::new(),
            exit_reevaluation_requested: false,
            text_store_host: None,
            input_availability: default_input_availability(),
            devices: H::devices(),
            displays: H::displays(),
            session_end_releases: 0,
            preferences: flui_platform::SystemPreferences::default(),
            preference_failure: None,
            display_failure: None,
            drag_source: H::drag_source,
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

/// Stop the platform and tell the application's handler it quits; the
/// delivery hears the quit once per run. The signal, when there is one,
/// closes first.
fn finish_quit(state: &Mutex<HeadlessState>) {
    let (signal, app) = {
        let mut state = state.lock();
        state.is_running = false;
        (state.owner_signal.upgrade(), Arc::clone(&state.app))
    };
    if let Some(signal) = signal {
        signal.close();
    }
    app.quit_requested().or_resume(());
}

/// Ask the application's handler whether the platform may quit now that no
/// window is tracked, and quit when it may. With no handler bound it never
/// quits.
///
/// The handler runs with the state lock released: it drops removed UI runtime
/// state, whose destructors may call back into this platform (a dispose hook
/// opening another window, say). Emptiness is checked before the question and
/// again before quitting, so a window opened meanwhile — by the handler itself
/// too — keeps the platform running. `true` when it quit.
pub(crate) fn consult_exit_policy(state: &Mutex<HeadlessState>) -> bool {
    let app = {
        let state = state.lock();
        if !state.windows.is_empty() {
            return false;
        }
        Arc::clone(&state.app)
    };
    // `false` with no handler, not `true`: a headless platform before or
    // after its run never quits on its own, the opposite of the desktop
    // hosts, whose loop ends with its last window when nothing says
    // otherwise.
    let allowed = app.exit_policy().or_resume(false);
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
        handler: Rc<dyn AppHandler>,
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

        // In headless mode, just run the bootstrap and return immediately. A
        // failed bootstrap has nowhere else to go on this backend since there
        // is no loop to keep running with a half-built app -- propagate
        // straight out of `run`. The handler stays bound for the platform's
        // life: a test reports to it after `run` returned.
        let app = Arc::clone(&self.state.lock().app);
        app.bind(handler, &platform, &hooks);
        app.resume_through_signal(&signal);
        if let Err(error) = app.ready(OwnerPlatform::new(platform, hooks)) {
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

    fn request_exit_policy_reevaluation(&self) {
        // Park only (coalesced): this mock has no event loop, so the
        // owner-thread half runs when the embedder calls
        // `HeadlessExitReevaluation::drive` — see that method's doc.
        self.with_state(|state| state.exit_reevaluation_requested = true);
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
    use flui_platform::Availability::Native;
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
/// A deferred open waiting for [`HeadlessDeferredWindowOpens::resolve_next`]:
/// where its window goes, its options, and its handler, which stays on the
/// owner thread that made it.
pub(crate) struct PendingOpen {
    slot: ClaimSlot<OpenWindowResult>,
    options: WindowOptions,
    handler: OwnerSlot<Rc<dyn WindowHandler>>,
}

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

    fn open_owner_window(
        &self,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<WindowOpen, OpenWindowError> {
        // No wake-worthy event loop to notify on abandonment (this backend
        // never parks a real loop on the request) -- see `claim_slot`'s own
        // doc for why a no-op wake is the correct choice for a generic
        // caller like this one.
        if !self.signal.accepting() {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }
        let kept = OwnerSlot::new();
        if let Err(handler) = kept.replace(handler) {
            crate::owner::call_point::retire(handler);
            return Err(OpenWindowError::Unavailable {
                message: "the owner thread cannot keep another window handler".to_owned(),
            });
        }
        let (slot, handle) = claim_slot::<OpenWindowResult>(Arc::new(|| {}));
        self.state.lock().pending_opens.push(PendingOpen {
            slot,
            options,
            handler: kept,
        });
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
    /// Whether the platform still runs: `false` once it quit, or once it is
    /// gone.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.state
            .upgrade()
            .is_some_and(|state| state.lock().is_running)
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
    /// was dropped, or every request so far has already been resolved) or
    /// the call is not on the owner thread whose handler the oldest request
    /// carries, which it leaves pending. The window's handler hears the
    /// capability snapshot before this returns.
    pub fn resolve_next(&self) -> Option<Rc<dyn HostWindow>> {
        let platform_state = self.state.upgrade()?;

        // Pop the request and build its window under one lock acquisition,
        // then deliver OUTSIDE the lock (same discipline `MockWindow::
        // notify_closed` uses) -- `deliver`'s injected wake callback is a
        // no-op here, but a future caller reusing this pattern must not
        // rely on that.
        let (slot, window) = {
            let mut state = platform_state.lock();
            // The handler lives on the owner thread that asked: off it, the
            // request stays pending.
            if state.pending_opens.first()?.handler.is_occupied() != Some(true) {
                return None;
            }
            let PendingOpen {
                slot,
                options,
                handler,
            } = state.pending_opens.remove(0);
            let handler = handler
                .take()
                .expect("BUG: a deferred open resolves on the owner thread that made it");
            let windows = Arc::clone(&state.windows);
            let window = Arc::clone(&windows).open(
                &mut state,
                Arc::downgrade(&platform_state),
                options,
                handler,
            );
            windows.keep_deferred(&window);
            (slot, window)
        };
        // The lock is released: the handler hears its window's capabilities
        // before anything later.
        window.delivery().deliver_capability_snapshot();

        // The requester may have already abandoned the request (dropped its
        // `PendingWindow` before this call) -- `deliver`'s `Err` hands the
        // freshly created window back so it is dropped/unwound instead of
        // silently leaking a window nothing will ever claim. Either way this
        // method still returns the SAME window handle to its own caller: a
        // test driving `resolve_next` directly (rather than through a real
        // `PendingWindow`) legitimately wants it regardless of whether some
        // OTHER requester abandoned their own claim to it.
        if let Err(_unclaimed) = slot.deliver(Ok(crate::owner::OpenedWindow::new(&window))) {
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
    use crate::owner::notices::testing::NoNotices;

    /// A host with no display, notice or device, for a loop whose test reads
    /// none of them.
    struct EmptyHost;

    impl HeadlessHost for EmptyHost {
        fn displays() -> DisplaySnapshot {
            DisplaySnapshot::empty()
        }

        fn notices(_: &Arc<AppDelivery>) -> Arc<dyn crate::owner::notices::NoticeChannel> {
            Arc::new(NoNotices)
        }

        fn devices() -> Box<dyn SimulatedDevices> {
            Box::new(NoDevices)
        }

        fn offers() -> Box<dyn SimulatedOffers> {
            Box::new(crate::drag::offers::OfferTable::default())
        }

        fn drag_source(
            _: &HeadlessState,
        ) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
            None
        }
    }

    /// A system no device attaches to.
    struct NoDevices;

    impl SimulatedDevices for NoDevices {
        fn attach(
            &mut self,
            _: flui_platform::pointer::PointerKind,
            _: flui_platform::EventTime,
        ) -> Option<flui_platform::pointer::PointerDeviceChange> {
            None
        }

        fn remove(
            &mut self,
            _: flui_platform::pointer::DeviceId,
            _: flui_platform::EventTime,
        ) -> Option<flui_platform::pointer::PointerDeviceChange> {
            None
        }

        fn present(&self) -> flui_platform::input_capabilities::PointerKindSet {
            flui_platform::input_capabilities::PointerKindSet::empty()
        }
    }

    /// No window, for a loop whose test opens none.
    struct NoWindows;

    impl OpenWindows for NoWindows {
        fn is_empty(&self) -> bool {
            true
        }
    }

    impl TrackedWindows for NoWindows {
        fn open(
            self: Arc<Self>,
            _: &mut HeadlessState,
            _: Weak<Mutex<HeadlessState>>,
            _: WindowOptions,
            _: Rc<dyn WindowHandler>,
        ) -> Rc<dyn HostWindow> {
            unreachable!("the test opens no window")
        }

        fn keep_deferred(&self, _: &Rc<dyn HostWindow>) {
            unreachable!("the test opens no window")
        }

        fn deliveries_of_all(&self) -> Vec<Arc<WindowDelivery>> {
            Vec::new()
        }

        fn delivery_of(&self, _: WindowId) -> Option<Arc<WindowDelivery>> {
            None
        }
    }

    /// The quit reaches the handler once, outside the state lock, and a quit
    /// the handler issues from inside it is the same quit.
    #[test]
    fn explicit_quit_reenters_and_reaches_the_handler_outside_the_state_lock() {
        use std::cell::Cell;

        use crate::owner::app_delivery::testing::{AppCall, AppLog, bind};

        let platform = Arc::new(HeadlessLoop::new::<EmptyHost>(Arc::new(NoWindows)));
        platform.state.lock().is_running = true;
        let app = AppLog::new();
        let delivery = Arc::clone(&platform.state.lock().app);
        let run = bind(delivery, app.clone());
        run.delivery.ready(run.owner()).expect("bootstrap");
        let unlocked = Rc::new(Cell::new(false));
        let (seen, reentered) = (Rc::clone(&unlocked), Arc::clone(&platform));
        app.react(move |call, _| {
            if call == AppCall::Quit {
                seen.set(reentered.state.try_lock().is_some());
                reentered.quit();
            }
        });
        platform.quit();
        platform.quit();
        assert_eq!(app.calls(), [AppCall::Ready, AppCall::Quit]);
        assert!(
            unlocked.get(),
            "the handler heard the quit outside the state lock"
        );
        assert!(!platform.state.lock().is_running);
        run.delivery.release();
    }
}
