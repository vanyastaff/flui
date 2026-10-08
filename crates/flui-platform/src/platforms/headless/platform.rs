//! Headless platform implementation for testing
//!
//! This platform implementation runs without any actual windowing system,
//! making it ideal for unit tests and CI environments.

use std::{
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, ThreadId},
};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Bounds, Point, Size};
use flui_foundation::{ClaimSlot, claim_slot};
use flui_platform_api::HapticFeedback;
use flui_platform_api::InMemoryClipboard;
use flui_platform_api::text_store::TextStoreHost;
use parking_lot::Mutex;

use crate::{
    data_transfer::{DataTransferSource, NullDataTransferSource},
    error::PlatformError,
    shared::{PlatformHandlers, WindowCallbacks},
    traits::{
        Clipboard, ClipboardItem, CursorError, DesktopCapabilities, DispatchEventResult,
        HostWindow, OpenWindowError, OwnerPlatform, PendingWindow, Platform, PlatformCapabilities,
        PlatformDisplay, PlatformExecutor, PlatformHaptics, PlatformInput, PlatformReadyCallback,
        PlatformTextInput, PlatformWindow, SessionEndAnswer, SessionEndPhase, WindowAppearance,
        WindowBackgroundAppearance, WindowBounds, WindowEvent, WindowId, WindowOpen, WindowOptions,
        owner::{DirectOwnerHooks, OwnerHooks, ProxyTransport},
    },
};

/// The value a deferred (or synchronous) window-open request resolves to —
/// mirrors the winit backend's own private `OpenWindowResult` alias
/// (`platforms/winit/control.rs`) so both backends complete a
/// [`ClaimSlot`]/[`PendingWindow`] pair with an identical shape.
type OpenWindowResult = Result<Arc<dyn HostWindow>, OpenWindowError>;

/// Builds the text-store host a headless window offers
/// ([`HeadlessPlatform::with_text_store_host`]). Shared with every window,
/// so `Send + Sync`; what it builds is owner-thread state.
type TextStoreHostFactory = Arc<dyn Fn() -> Rc<dyn TextStoreHost> + Send + Sync>;

/// Process-wide identity source for mock windows.
///
/// A headless test may construct several independent [`HeadlessPlatform`]
/// values whose windows later meet in one application registry. Scoping the
/// counter to a platform instance lets those windows alias even though real
/// backend handles are process-unique.
static NEXT_HEADLESS_WINDOW_ID: AtomicU64 = AtomicU64::new(0);

fn next_headless_window_id() -> WindowId {
    let raw = NEXT_HEADLESS_WINDOW_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .expect("BUG: exhausted the process-wide headless WindowId space");
    WindowId(raw)
}

/// Headless platform for testing
///
/// This platform implementation doesn't create any real windows or graphics
/// contexts. It's designed for:
/// - Unit tests that need a Platform implementation
/// - CI environments without display servers
/// - Benchmarking without rendering overhead
pub struct HeadlessPlatform {
    signal: Arc<crate::shared::owner_signal::OwnerSignal>,
    wake_failure: Arc<std::sync::atomic::AtomicBool>,
    capabilities: DesktopCapabilities,
    state: Arc<Mutex<HeadlessState>>,
}

struct HeadlessState {
    owner_signal: Weak<crate::shared::owner_signal::OwnerSignal>,
    handlers: PlatformHandlers,
    background_executor: Arc<TestExecutor>,
    clipboard: Arc<InMemoryClipboard>,
    active_window: Option<WindowId>,
    is_running: bool,
    windows: Vec<MockWindow>,
    appearance: WindowAppearance,
    keyboard_layout: String,
    opened_urls: Vec<String>,
    /// Deferred-window-open test mode (see
    /// [`HeadlessPlatform::enable_deferred_window_open`]): when `true`,
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
    /// ([`HeadlessPlatform::with_text_store_host`]); `None`: push-model.
    text_store_host: Option<TextStoreHostFactory>,
}

impl HeadlessPlatform {
    /// Create a new headless platform
    pub fn new() -> Self {
        let state = HeadlessState {
            owner_signal: Weak::new(),
            handlers: PlatformHandlers::new(),
            background_executor: Arc::new(TestExecutor::new("background")),
            clipboard: Arc::new(InMemoryClipboard::new()),
            active_window: None,
            is_running: false,
            windows: Vec::new(),
            appearance: WindowAppearance::default(),
            keyboard_layout: "en-US".to_string(),
            opened_urls: Vec::new(),
            deferred_window_open: false,
            pending_opens: Vec::new(),
            exit_reevaluation_requested: false,
            text_store_host: None,
        };

        let wake_failure = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let failure = Arc::clone(&wake_failure);
        let platform = Self {
            wake_failure,
            signal: crate::shared::owner_signal::OwnerSignal::new(Arc::new(move || {
                if failure.swap(false, Ordering::AcqRel) {
                    Err(PlatformError::EventLoop {
                        message: "injected headless owner wake failure".into(),
                    })
                } else {
                    Ok(())
                }
            })),
            capabilities: DesktopCapabilities,
            state: Arc::new(Mutex::new(state)),
        };
        {
            let mut state = platform.state.lock();
            state.owner_signal = Arc::downgrade(&platform.signal);
        }
        platform
    }

    /// Obtain the manual owner-turn driver before `run` consumes this platform.
    #[must_use]
    pub fn owner_turns(&self) -> HeadlessOwnerTurns {
        HeadlessOwnerTurns {
            signal: Arc::downgrade(&self.signal),
            wake_failure: Arc::downgrade(&self.wake_failure),
            state: Arc::downgrade(&self.state),
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
            state: Arc::downgrade(&self.state),
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
        self.with_state(|state| state.deferred_window_open = true);
        HeadlessDeferredWindowOpens {
            state: Arc::downgrade(&self.state),
        }
    }

    /// Make every window this platform opens pull-model: its
    /// [`HostWindow::text_store_host`] answers a host that `factory` builds,
    /// on the owner thread, each time it is read (the runner reads it once
    /// per window). A test-support stand-in for the Win32 text services
    /// (ADR-0135); without it a headless window is push-model and offers
    /// [`PlatformWindow::text_input`] alone.
    #[must_use]
    pub fn with_text_store_host(
        self,
        factory: impl Fn() -> Rc<dyn TextStoreHost> + Send + Sync + 'static,
    ) -> Self {
        self.with_state(|state| state.text_store_host = Some(Arc::new(factory)));
        self
    }
    /// Simulate one phase of the user's session ending, as the operating
    /// system reports it: the callback registered with
    /// [`Platform::on_session_end`] is asked, on the calling thread, and its
    /// answer returned; [`SessionEndAnswer::Proceed`] when none is
    /// registered. Like [`MockWindow::simulate_close`], it may be called from
    /// inside another callback, as a session end arriving in a nested native
    /// loop is.
    ///
    /// Not yet delivered: the callback is not asked, and every phase answers
    /// [`SessionEndAnswer::Proceed`].
    pub fn simulate_session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        let _ = phase;
        SessionEndAnswer::Proceed
    }

    fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HeadlessState) -> R,
    {
        let mut state = self.state.lock();
        f(&mut state)
    }
}

impl std::fmt::Debug for HeadlessPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessPlatform").finish_non_exhaustive()
    }
}

impl Default for HeadlessPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for HeadlessPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.background_executor.clone())
    }

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError> {
        tracing::info!("Starting headless platform (no event loop)");

        // Captured before `*self` moves into the `Arc` below -- same
        // underlying `Mutex<HeadlessState>`, just a durable handle to it
        // that survives the move.
        let state_handle = Arc::clone(&self.state);
        let signal = Arc::clone(&self.signal);
        signal.bind_owner();
        struct BootstrapGuard(Option<Arc<crate::shared::owner_signal::OwnerSignal>>);
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

        let platform: Arc<dyn Platform> = Arc::new(*self);
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

        // Consume before calling: callback bodies and captured-data destructors
        // may re-enter the platform. Matches the close/reevaluation quit paths.
        let callback = self.with_state(|state| {
            state.is_running = false;
            state.handlers.quit.take()
        });
        if let Some(mut callback) = callback {
            callback();
        }
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        self.with_state(|state| {
            state.handlers.exit_policy = Some(hook);
        });
    }

    fn request_exit_policy_reevaluation(&self) {
        // Park only (coalesced): this mock has no event loop, so the
        // owner-thread half runs when the embedder calls
        // `HeadlessExitReevaluation::drive` — see that method's doc.
        self.with_state(|state| state.exit_reevaluation_requested = true);
    }

    fn open_window(&self, options: WindowOptions) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        tracing::info!(?options, "Creating mock window");

        let platform_state = Arc::downgrade(&self.state);
        Ok(self.with_state(|state| create_mock_window(state, platform_state, options)))
    }

    fn active_window(&self) -> Option<WindowId> {
        self.with_state(|state| state.active_window)
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        // Return one mock display
        vec![Arc::new(MockDisplay::primary())]
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(MockDisplay::primary()))
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.with_state(|state| state.clipboard.clone())
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        // Headless gets a real test source with the clipboard transport
        // (ADR-0038); until then it is inert and honest.
        Arc::new(NullDataTransferSource)
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        &self.capabilities
    }

    fn name(&self) -> &'static str {
        "Headless"
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.with_state(|state| {
            state.handlers.quit = Some(callback);
        });
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.with_state(|state| {
            state.handlers.window_event = Some(callback);
        });
    }

    // ==================== US3 Methods ====================

    fn activate(&self, _ignoring_other_apps: bool) {
        // No-op in headless mode
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.with_state(|state| state.appearance)
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        if let Some(text) = item.text_content() {
            self.with_state(|state| {
                state.clipboard.write_text(text.to_string());
            });
        }
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.with_state(|state| state.clipboard.read_text().map(ClipboardItem::text))
    }

    fn open_url(&self, url: &str) {
        self.with_state(|state| {
            state.opened_urls.push(url.to_string());
        });
    }

    fn keyboard_layout(&self) -> String {
        self.with_state(|state| state.keyboard_layout.clone())
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        self.with_state(|state| {
            state.handlers.keyboard_layout_changed = Some(callback);
        });
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        Ok(PathBuf::from("/mock/app/path"))
    }
}

/// Builds a fresh mock window under an already-locked `state`, threading it
/// through the same register/activate/emit-`Created` bookkeeping
/// `Platform::open_window`'s synchronous path uses — shared so the deferred-
/// open resolution path ([`HeadlessDeferredWindowOpens::resolve_next`]) stays
/// byte-for-byte consistent with the synchronous one instead of duplicating
/// it.
fn create_mock_window(
    state: &mut HeadlessState,
    platform_state: Weak<Mutex<HeadlessState>>,
    options: WindowOptions,
) -> Arc<dyn HostWindow> {
    let window_id = next_headless_window_id();
    let window = MockWindow::new(
        window_id,
        options,
        platform_state,
        state.text_store_host.clone(),
    );

    state.windows.push(window.clone());
    state.active_window = Some(window_id);

    state
        .handlers
        .invoke_window_event(WindowEvent::Created(window_id));

    Arc::new(window) as Arc<dyn HostWindow>
}

/// [`OwnerHooks`] for the headless backend's deferred-open test mode
/// (installed by `Platform::run` in place of `DirectOwnerHooks` when
/// [`HeadlessPlatform::enable_deferred_window_open`] was called first).
/// Exercises the `WindowOpen::Pending` arm of the owner-lane contract
/// without needing a real winit event loop: every `open_owner_window` call
/// enqueues a [`ClaimSlot`] request into `HeadlessState::pending_opens`
/// instead of creating synchronously, and
/// [`HeadlessDeferredWindowOpens::resolve_next`] completes the oldest one on
/// demand.
struct HeadlessDeferredOwnerHooks {
    signal: Arc<crate::shared::owner_signal::OwnerSignal>,
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
                rejected: Some(options),
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
        Arc::new(crate::shared::owner_signal::SignalTransport::new(
            &self.signal,
        ))
    }
}

/// Deterministic headless owner-turn driver. `run` returning does not close it.
#[derive(Clone)]
pub struct HeadlessOwnerTurns {
    wake_failure: Weak<std::sync::atomic::AtomicBool>,
    owner_affinity: std::marker::PhantomData<std::rc::Rc<()>>,
    signal: Weak<crate::shared::owner_signal::OwnerSignal>,
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
                let callback = {
                    let mut state = state.lock();
                    state.is_running = false;
                    state.handlers.quit.take()
                };
                if let Some(mut callback) = callback {
                    callback();
                }
            }
        }
    }
}

/// Probe/drive handle for parked
/// `Platform::request_exit_policy_reevaluation` requests on the headless
/// mock — obtained via [`HeadlessPlatform::exit_reevaluation`] BEFORE
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
    /// quit" headless default and its take/consult-outside-the-lock/
    /// restore discipline) and quit if the hook now allows it. Returns
    /// `true` iff a quit was actually issued.
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
        let hook = {
            let mut state = platform_state.lock();
            if !state.exit_reevaluation_requested {
                return false;
            }
            state.exit_reevaluation_requested = false;
            if !state.windows.is_empty() {
                return false;
            }
            state.handlers.exit_policy.take()
        };

        // Consult OUTSIDE the state lock: the hook re-enters the
        // embedder's runtime (dropping removed ui_runtime state whose
        // destructors may call back into this platform).
        let should_quit = hook.as_ref().is_some_and(|hook| hook());
        // Restore only if nothing fresher was installed meanwhile — the
        // hook is not one-shot; a veto leaves it in place for the next
        // consult (same rule as `MockWindow::notify_closed`).
        if let Some(hook) = hook {
            let mut state = platform_state.lock();
            if state.handlers.exit_policy.is_none() {
                state.handlers.exit_policy = Some(hook);
            }
        }
        if !should_quit {
            return false;
        }

        let (quit_callback, signal) = {
            let mut state = platform_state.lock();
            state.is_running = false;
            (state.handlers.quit.take(), state.owner_signal.upgrade())
        };
        if let Some(signal) = signal {
            signal.close();
        }
        if let Some(mut callback) = quit_callback {
            callback();
        }
        true
    }
}

/// Test handle returned by [`HeadlessPlatform::enable_deferred_window_open`]
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
    /// (a real, live [`PlatformWindow`] a test can drive further — including
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

// ==================== Mock Implementations ====================

/// The canonical test/headless window double.
///
/// Every window [`HeadlessPlatform`] opens is a `MockWindow`: a fully
/// functional [`PlatformWindow`] that needs no
/// display server. Beyond the trait surface it offers what a test harness
/// needs to drive a window from the outside:
///
/// - a configurable scale factor ([`Self::simulate_scale_factor_change`],
///   delivered through the resize path exactly like the winit backend);
/// - `request_redraw` dispatching through the registered callbacks
///   ([`WindowCallbacks`]), like a real backend's frame wire;
/// - programmatic event injection ([`Self::inject_event`]) and lifecycle
///   simulation (`simulate_resize` / `simulate_focus` /
///   `simulate_visibility` / `simulate_close`), each firing the same
///   registered callback a real platform would.
///
/// There is no public constructor: obtain one by opening a window on a
/// [`HeadlessPlatform`] and downcasting the returned
/// `Arc<dyn PlatformWindow>` via its `as_any()` to `&MockWindow`.
#[derive(Clone)]
pub struct MockWindow {
    id: WindowId,
    state: Arc<Mutex<MockWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    text_input: Arc<FakeTextInput>,
    haptics: Arc<FakeHaptics>,
    accessibility: Arc<FakeAccessibility>,
    /// Builds this window's text-store host, when the platform was made
    /// pull-model.
    text_store_host: Option<TextStoreHostFactory>,
    /// Back-reference to the platform this window was opened on, so
    /// closing it can remove its own entry from `HeadlessState::windows`
    /// and — only once every tracked window is gone — consult the
    /// exit-policy hook exactly like the winit backend's `CloseRequested`
    /// handling does (see `notify_closed`). `Weak`: a window must never
    /// keep the platform state alive past the platform's own lifetime.
    platform_state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for MockWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("MockWindow")
            .field("id", &self.id)
            .field("title", &state.title)
            .field("bounds", &state.bounds)
            .field("scale_factor", &state.scale_factor)
            .field("focused", &state.focused)
            .field("visible", &state.visible)
            .field("callbacks", &self.callbacks)
            .finish_non_exhaustive()
    }
}

/// Mutable state for headless MockWindow
struct MockWindowState {
    /// How many times [`PlatformWindow::pre_present_notify`] was called —
    /// the oracle for "the frame pump arms the compositor frame callback
    /// once per presented frame, before the present".
    pre_present_notifies: u64,
    title: String,
    bounds: Bounds<f64>,
    scale_factor: f64,
    focused: bool,
    visible: bool,
    execution: crate::WindowExecutionState,
    safe_area: flui_foundation::geometry::EdgeInsets,
    maximized: bool,
    minimized: bool,
    closed: bool,
    fullscreen: bool,
    hovered: bool,
    modifiers: flui_platform_api::keyboard::Modifiers,
    appearance: WindowAppearance,
    cursor: CursorIcon,
}

impl Clone for MockWindowState {
    fn clone(&self) -> Self {
        Self {
            pre_present_notifies: self.pre_present_notifies,
            title: self.title.clone(),
            bounds: self.bounds,
            scale_factor: self.scale_factor,
            focused: self.focused,
            execution: self.execution,
            safe_area: self.safe_area,
            visible: self.visible,
            maximized: self.maximized,
            minimized: self.minimized,
            closed: self.closed,
            fullscreen: self.fullscreen,
            hovered: self.hovered,
            modifiers: self.modifiers,
            appearance: self.appearance,
            cursor: self.cursor,
        }
    }
}

impl MockWindow {
    fn new(
        id: WindowId,
        options: WindowOptions,
        platform_state: Weak<Mutex<HeadlessState>>,
        text_store_host: Option<TextStoreHostFactory>,
    ) -> Self {
        Self {
            id,
            state: Arc::new(Mutex::new(MockWindowState {
                pre_present_notifies: 0,
                title: options.title.clone(),
                bounds: Bounds {
                    origin: Point::default(),
                    size: options.size,
                },
                scale_factor: 1.0,
                focused: true,
                execution: crate::WindowExecutionState::Running,
                safe_area: flui_foundation::geometry::EdgeInsets::ZERO,
                visible: options.visible,
                maximized: false,
                minimized: false,
                closed: false,
                fullscreen: false,
                hovered: false,
                modifiers: flui_platform_api::keyboard::Modifiers::NONE,
                appearance: WindowAppearance::default(),
                cursor: CursorIcon::default(),
            })),
            callbacks: Arc::new(WindowCallbacks::new()),
            text_input: Arc::new(FakeTextInput::new()),
            haptics: Arc::new(FakeHaptics::new()),
            accessibility: Arc::new(FakeAccessibility::new()),
            text_store_host,
            platform_state,
        }
    }

    /// Removes this window from the platform's own tracking and, only once
    /// every window this platform knows about is gone, consults the
    /// exit-policy hook (see [`crate::shared::PlatformHandlers::exit_policy`])
    /// exactly as the winit backend's `CloseRequested` handling does. A
    /// caller that never installed a hook sees no behavior change: this is
    /// a no-op unless [`crate::traits::Platform::set_exit_policy_hook`] was
    /// called first, matching every other headless test/consumer that
    /// predates this mechanism.
    ///
    /// Every user-supplied callback (the hook itself, and `on_quit`) is taken
    /// out of the lock and called OUTSIDE it (ADR-0039) — the same
    /// take/invoke/restore-if-none discipline `WinitPlatform`'s own
    /// window-event-handler lease uses, and for the identical reason: the
    /// hook's body (`flui-app`'s `AppRuntime::should_exit`) drops removed
    /// UI runtime state, whose destructors may call back into this platform (a
    /// dispose hook opening another window, say). Calling either callback
    /// while still holding `platform_state`'s lock would deadlock the
    /// instant such a callback re-entered any lock-guarded method (e.g.
    /// `open_window`).
    /// Leases the global window-event handler out of the platform state,
    /// invokes it with `event`, and restores it unless something fresher was
    /// installed meanwhile.
    ///
    /// Never invoked under the state lock. The handler re-enters the
    /// embedder's runtime — in `flui-app` it drops the removed UI runtime's state,
    /// whose destructors can call back into this platform (a dispose hook
    /// opening a window) — so calling it while holding `platform_state` would
    /// deadlock the moment it did. This is the discipline
    /// [`Self::notify_closed`] already applies to the exit-policy hook
    /// (ADR-0039), and the one `WinitPlatform::lease_window_event_handler`
    /// exists for.
    ///
    /// Note the consequence of leasing: an event emitted from *inside* the
    /// handler finds the slot empty and is dropped. That matches winit, whose
    /// lease has the same shape.
    fn emit_window_event(&self, event: WindowEvent) {
        let Some(platform_state) = self.platform_state.upgrade() else {
            return;
        };
        let handler = { platform_state.lock().handlers.window_event.take() };
        let Some(handler) = handler else {
            return;
        };
        // Restored by the guard's `Drop`, not by a line after the call: a
        // panicking handler would otherwise be dropped on the floor and every
        // later event silently lost, which is a worse failure than the panic.
        // `WinitPlatform`'s own lease restores in `Drop` for the same reason.
        let mut lease = WindowEventLease {
            platform_state: &platform_state,
            handler: Some(handler),
        };
        if let Some(handler) = lease.handler.as_mut() {
            handler(event);
        }
    }

    /// The whole close teardown, in the order both production backends run it:
    /// hide, fire `on_close`, drop the window from tracking, clear its
    /// callbacks, emit the global `Closed`, and consult the exit policy only
    /// once nothing is tracked any more.
    ///
    /// Mirrors `WinitAppHandler::complete_window_close`. Two of its steps have
    /// no analogue here and are deliberately absent rather than forgotten:
    /// there is no native handle to hide beyond the state flag, and
    /// `forget_window` has nothing to forget because this backend's data
    /// transfer is the null source.
    ///
    /// Reached from both routes — [`crate::traits::PlatformWindow::close`] and
    /// [`Self::simulate_close`] — so the two cannot drift apart.
    fn complete_close(&self) {
        {
            let mut state = self.state.lock();
            state.visible = false;
            state.closed = true;
        }
        self.callbacks.dispatch_close();
        self.notify_closed();
        // After the global `Closed`, never before: a handler that inspects the
        // window it was just told about must not find its callbacks already
        // gone.
        self.callbacks.clear();
    }

    fn notify_closed(&self) {
        let Some(platform_state) = self.platform_state.upgrade() else {
            return;
        };

        // Remove this window, then take the hook, in the SAME lock
        // acquisition that re-checks emptiness — not two separate short
        // locks. Between two separate acquisitions, another thread (or a
        // reentrant call this window's own removal triggers) could open a
        // new window in the gap, leaving a `windows_empty` read from the
        // first lock stale: true when read, false by the time the hook
        // actually runs. Re-checking here, still under one lock, closes
        // that window; the hook/quit callback themselves still run OUTSIDE
        // the lock (see this function's own doc for why). If the registry
        // is not actually empty, the hook is left installed untouched
        // (never taken) for the window close that does empty it.
        {
            let mut state = platform_state.lock();
            state.windows.retain(|w| w.id != self.id);
            if state.active_window == Some(self.id) {
                state.active_window = state.windows.first().map(|w| w.id);
            }
        }

        // Emitted for EVERY close, not only the last one: `Closed` is the
        // event a consumer tracks windows by, so skipping it while other
        // windows remain would leave that consumer believing a closed window
        // is still open — the multi-window case these events exist for.
        // Leased, so the handler runs outside the state lock (ADR-0039) and
        // survives its own panic.
        self.emit_window_event(WindowEvent::Closed(self.id));

        let hook = {
            let mut state = platform_state.lock();
            // Re-checked HERE rather than carried down from the lock above.
            // Before the global `Closed` event existed, emptiness and the
            // hook were read in one acquisition precisely so nothing could
            // open a window in between. Emitting the event splits that
            // acquisition in two — and the thing running in the gap is a
            // consumer callback, which is exactly the code that opens
            // windows (a close handler showing the next one). Carrying the
            // earlier answer would let this consult the hook, and quit,
            // with a window tracked.
            if !state.windows.is_empty() {
                return;
            }
            state.handlers.exit_policy.take()
        };

        // `is_some_and`, not `is_none_or`: headless's own pre-#555 default
        // is "no hook -> never quit" (matching every headless test/consumer
        // that predates this mechanism, none of which expects closing a
        // mock window to spontaneously call `quit`) -- the OPPOSITE of
        // winit's "no hook -> exit unconditionally" default, which matches
        // real native pre-#555 window-close behavior instead. The two
        // backends' defaults are deliberately different; this is not a
        // typo.
        let should_quit = hook.as_ref().is_some_and(|hook| hook());
        // Restore only if nothing fresher was installed meanwhile -- the
        // hook is not one-shot, unlike `quit` below: a veto here must leave
        // it in place for the NEXT window close to consult.
        if let Some(hook) = hook {
            let mut state = platform_state.lock();
            if state.handlers.exit_policy.is_none() {
                state.handlers.exit_policy = Some(hook);
            }
        }
        if !should_quit {
            return;
        }

        let (quit_callback, signal) = {
            let mut state = platform_state.lock();
            state.is_running = false;
            (state.handlers.quit.take(), state.owner_signal.upgrade())
        };
        if let Some(signal) = signal {
            signal.close();
        }
        if let Some(mut callback) = quit_callback {
            callback();
        }
    }

    /// How many times the frame pump has called
    /// [`PlatformWindow::pre_present_notify`] on this window — one per
    /// presented frame, before the present, is the contract.
    #[must_use]
    pub fn pre_present_notifies(&self) -> u64 {
        self.state.lock().pre_present_notifies
    }

    /// Inject a platform input event for testing.
    /// Fires the registered `on_input` callback.
    pub fn inject_event(&self, event: PlatformInput) -> DispatchEventResult {
        self.callbacks.dispatch_input(event)
    }

    /// Simulate a resize for testing.
    /// Fires the registered `on_resize` callback.
    pub fn simulate_resize(&self, width: f64, height: f64) {
        let size = Size::new(width, height);
        let scale = self.state.lock().scale_factor;
        self.state.lock().bounds.size = size;
        self.callbacks.dispatch_resize(size, scale);
    }

    /// Simulate a monitor DPI change for testing: the scale factor moves
    /// with NO size change, and — matching the winit backend — the new
    /// scale is delivered through the resize path with the current logical
    /// size, because that path is the only one the UI runtime learns its
    /// device-pixel ratio from.
    pub fn simulate_scale_factor_change(&self, scale_factor: f64) {
        let size = {
            let mut state = self.state.lock();
            state.scale_factor = scale_factor;
            state.bounds.size
        };
        self.callbacks.dispatch_resize(size, scale_factor);
    }

    /// Simulate focus change for testing.
    /// Fires the registered `on_active_status_change` callback.
    pub fn simulate_focus(&self, focused: bool) {
        self.state.lock().focused = focused;
        self.callbacks.dispatch_active_status_change(focused);
    }

    /// Simulate an owner-thread safe-area report; the callback fires without
    /// holding window state. Closed and detached windows ignore the report.
    pub fn simulate_safe_area(&self, insets: flui_foundation::geometry::EdgeInsets) {
        {
            let mut state = self.state.lock();
            if state.closed
                || state.execution == crate::WindowExecutionState::Detached
                || state.safe_area == insets
            {
                return;
            }
            state.safe_area = insets;
        }
        self.callbacks.dispatch_safe_area_change(insets);
    }

    /// Simulate reversible native execution eligibility; terminal close stays terminal.
    pub fn simulate_execution_state(&self, execution: crate::WindowExecutionState) {
        {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.execution = execution;
        }
        self.callbacks.dispatch_execution_state_change(execution);
    }

    /// Simulate a visibility/occlusion change for testing.
    /// Fires the registered `on_visibility_status_change` callback.
    pub fn simulate_visibility(&self, visible: bool) {
        self.state.lock().visible = visible;
        self.callbacks.dispatch_visibility_status_change(visible);
    }

    /// Simulate a GPU-surface availability change for testing.
    /// Fires the registered `on_surface_status_change` callback.
    ///
    /// No backend in this workspace emits this signal but Android, so this is
    /// the only way a host-run test can drive the seam the released-surface
    /// path hangs off: `false` asks the callback to release its surface,
    /// `true` asks it to rebuild one.
    pub fn simulate_surface_status(&self, has_surface: bool) {
        self.callbacks.dispatch_surface_status_change(has_surface);
    }

    /// Simulate close request for testing.
    /// Fires `on_should_close`, then `on_close` if allowed, then (real,
    /// production behavior — not a test-only shortcut) removes this window
    /// from the platform's tracking and consults the exit-policy hook if
    /// every tracked window is now gone. See `Self::notify_closed`.
    pub fn simulate_close(&self) -> bool {
        let should = self.callbacks.dispatch_should_close();
        if should {
            // The user asked and nothing vetoed. This global event belongs to
            // the user route: on winit only a compositor `CloseRequested`
            // produces it, and on Win32 only the `WM_CLOSE` arm does. A veto
            // must produce neither event, which is why this sits inside the
            // `should` branch and not above it.
            self.emit_window_event(WindowEvent::CloseRequested { window_id: self.id });
            self.complete_close();
        }
        should
    }
}

/// Holds the global window-event handler out of [`HeadlessState`] for the
/// duration of one invocation and puts it back on the way out — including
/// while unwinding.
///
/// The handler cannot be invoked under the state lock: on the close path it
/// re-enters the embedder's runtime, which drops UI runtime state whose destructors
/// call back into this platform (ADR-0039). Taking it out is what makes that
/// safe; restoring it in `Drop` is what keeps a panicking handler from
/// silently disabling every later event.
struct WindowEventLease<'a> {
    platform_state: &'a Arc<Mutex<HeadlessState>>,
    handler: Option<Box<dyn FnMut(WindowEvent) + Send>>,
}

impl Drop for WindowEventLease<'_> {
    fn drop(&mut self) {
        let Some(handler) = self.handler.take() else {
            return;
        };
        let mut state = self.platform_state.lock();
        // Only if nothing fresher was installed while the lease was out: a
        // handler registered during the callback is the newer intent.
        if state.handlers.window_event.is_none() {
            state.handlers.window_event = Some(handler);
        }
    }
}

impl PlatformWindow for MockWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<i32> {
        let state = self.state.lock();
        Size::new(
            (state.bounds.size.width * state.scale_factor) as i32,
            (state.bounds.size.height * state.scale_factor) as i32,
        )
    }

    fn logical_size(&self) -> Size<f64> {
        self.state.lock().bounds.size
    }

    fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
    }

    fn request_redraw(&self) {
        self.callbacks.dispatch_request_frame();
    }

    fn pre_present_notify(&self) {
        self.state.lock().pre_present_notifies += 1;
    }

    fn is_focused(&self) -> bool {
        self.state.lock().focused
    }

    fn safe_area_insets(&self) -> flui_foundation::geometry::EdgeInsets {
        self.state.lock().safe_area
    }

    fn execution_state(&self) -> crate::WindowExecutionState {
        let state = self.state.lock();
        if state.closed {
            crate::WindowExecutionState::Detached
        } else {
            state.execution
        }
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
    }

    // ==================== Query Methods (US2) ====================

    fn bounds(&self) -> Bounds<f64> {
        self.state.lock().bounds
    }

    fn content_size(&self) -> Size<f64> {
        self.state.lock().bounds.size
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.state.lock();
        if state.fullscreen {
            WindowBounds::Fullscreen(state.bounds)
        } else if state.maximized {
            WindowBounds::Maximized(state.bounds)
        } else {
            WindowBounds::Windowed(state.bounds)
        }
    }

    fn is_maximized(&self) -> bool {
        self.state.lock().maximized
    }

    fn is_fullscreen(&self) -> bool {
        self.state.lock().fullscreen
    }

    fn is_active(&self) -> bool {
        self.state.lock().focused
    }

    fn is_hovered(&self) -> bool {
        self.state.lock().hovered
    }

    fn mouse_position(&self) -> Point<f64> {
        Point::default()
    }

    fn modifiers(&self) -> flui_platform_api::keyboard::Modifiers {
        self.state.lock().modifiers
    }

    fn appearance(&self) -> WindowAppearance {
        self.state.lock().appearance
    }

    fn display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(MockDisplay::primary()))
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        Some(Arc::clone(&self.text_input) as Arc<dyn PlatformTextInput>)
    }

    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        Some(Arc::clone(&self.haptics) as Arc<dyn PlatformHaptics>)
    }

    fn get_title(&self) -> String {
        self.state.lock().title.clone()
    }

    // ==================== Control Methods (US2) ====================

    fn set_title(&self, title: &str) {
        self.state.lock().title = title.to_string();
    }

    fn show(&self) -> Result<(), crate::WindowShowError> {
        let mut state = self.state.lock();
        if state.closed {
            return Err(crate::WindowShowError::Closed);
        }
        state.minimized = false;
        state.visible = true;
        state.focused = true;
        Ok(())
    }

    fn activate(&self) {
        self.state.lock().focused = true;
    }

    fn minimize(&self) {
        let mut state = self.state.lock();
        state.minimized = true;
        state.focused = false;
    }

    fn maximize(&self) {
        let mut state = self.state.lock();
        state.maximized = true;
        state.fullscreen = false;
    }

    fn restore(&self) {
        let mut state = self.state.lock();
        state.minimized = false;
        state.maximized = false;
        state.fullscreen = false;
    }

    fn toggle_fullscreen(&self) {
        let mut state = self.state.lock();
        state.fullscreen = !state.fullscreen;
        if state.fullscreen {
            state.maximized = false;
        }
    }

    fn resize(&self, size: Size<f64>) {
        self.state.lock().bounds.size = size;
    }

    fn close(&self) {
        self.complete_close();
    }

    fn set_background_appearance(&self, _appearance: WindowBackgroundAppearance) {
        // No-op in headless mode
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.state.lock().cursor = cursor;
        Ok(())
    }

    // ==================== Callbacks ====================

    crate::shared::impl_window_callback_setters!(callbacks);

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl HostWindow for MockWindow {
    fn accessibility(&self) -> Option<Arc<dyn crate::traits::PlatformAccessibility>> {
        Some(Arc::clone(&self.accessibility) as Arc<dyn crate::traits::PlatformAccessibility>)
    }

    fn text_store_host(
        &self,
        _owner: crate::traits::OwnerThreadToken,
    ) -> Option<Rc<dyn TextStoreHost>> {
        self.text_store_host.as_ref().map(|factory| factory())
    }
}

/// Recording fake for [`PlatformAccessibility`](crate::traits::PlatformAccessibility),
/// backing the headless backend's [`HostWindow::accessibility`].
///
/// Records every published tree so a test can assert what an assistive
/// technology would actually have been told — the roles, labels and ids — not
/// merely that publishing did not panic. Activation is drivable
/// ([`set_active`](Self::set_active)) because the interesting behaviour is
/// conditional on it: a composition root must start assembly on attach and stop
/// on detach, and neither is observable without being able to fake the signal.
#[derive(Default)]
pub struct FakeAccessibility {
    state: Mutex<FakeAccessibilityState>,
}

#[derive(Default)]
struct FakeAccessibilityState {
    active: bool,
    published: Vec<accesskit::TreeUpdate>,
    activation_listener: Option<crate::traits::AccessibilityActivationListener>,
    action_listener: Option<crate::traits::AccessibilityActionListener>,
}

impl FakeAccessibility {
    /// Create a fresh recorder, inactive and with nothing published.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every tree published while active, in order.
    #[must_use]
    pub fn published(&self) -> Vec<accesskit::TreeUpdate> {
        self.state.lock().published.clone()
    }

    /// How many trees were published while active.
    #[must_use]
    pub fn published_count(&self) -> usize {
        self.state.lock().published.len()
    }

    /// Simulate assistive technology attaching or detaching, notifying the
    /// registered listener.
    ///
    /// The listener is cloned out of the lock before being called, so a
    /// listener that publishes (or otherwise re-enters this fake) does not
    /// deadlock against the guard that dispatched it — the same
    /// clone-and-release discipline the real platform paths use.
    pub fn set_active(&self, active: bool) {
        let listener = {
            let mut state = self.state.lock();
            // Attach/detach is a *transition*. Re-notifying on an unchanged
            // state would drive redundant enable/disable cycles in a
            // composition root that starts and stops assembly off this signal.
            if state.active == active {
                return;
            }
            state.active = active;
            state.activation_listener.clone()
        };
        if let Some(listener) = listener {
            listener(active);
        }
    }

    /// Simulate assistive technology requesting an action.
    ///
    /// Dropped while inactive, because that is what the platform does: actions
    /// originate from an attached client, and one arriving after detach is
    /// stale. Forwarding it anyway would let a higher layer depend on
    /// action delivery outside an active session and pass here while failing
    /// against a real adapter.
    pub fn request_action(&self, request: accesskit::ActionRequest) {
        let listener = {
            let state = self.state.lock();
            if !state.active {
                return;
            }
            state.action_listener.clone()
        };
        if let Some(listener) = listener {
            listener(request);
        }
    }
}

impl std::fmt::Debug for FakeAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("FakeAccessibility")
            .field("active", &state.active)
            .field("published_count", &state.published.len())
            .finish_non_exhaustive()
    }
}

impl crate::traits::PlatformAccessibility for FakeAccessibility {
    fn publish(&self, update: accesskit::TreeUpdate) {
        let mut state = self.state.lock();
        // Inactive means nothing is listening: the update is dropped rather
        // than recorded, mirroring the real adapter's `update_if_active`.
        if state.active {
            state.published.push(update);
        }
    }

    fn is_active(&self) -> bool {
        self.state.lock().active
    }

    fn set_activation_listener(&self, listener: crate::traits::AccessibilityActivationListener) {
        self.state.lock().activation_listener = Some(listener);
    }

    fn set_action_listener(&self, listener: crate::traits::AccessibilityActionListener) {
        self.state.lock().action_listener = Some(listener);
    }
}

/// Recording fake for [`PlatformTextInput`], backing the headless backend's
/// [`PlatformWindow::text_input`].
///
/// Every `set_ime_allowed`/`set_ime_cursor_area` call is appended to an
/// in-memory history so a test can assert exactly what a presentation-owned
/// text-input session told the platform to do, rather than only that the call
/// didn't panic.
#[derive(Default)]
pub struct FakeTextInput {
    state: Mutex<FakeTextInputState>,
}

#[derive(Default, Clone)]
struct FakeTextInputState {
    ime_allowed_calls: Vec<bool>,
    cursor_area_calls: Vec<Bounds<f64>>,
}

impl FakeTextInput {
    /// Create a fresh recorder with no recorded calls.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every `set_ime_allowed` call, in delivery order.
    #[must_use]
    pub fn ime_allowed_calls(&self) -> Vec<bool> {
        self.state.lock().ime_allowed_calls.clone()
    }

    /// The most recent `set_ime_allowed` call, if any.
    #[must_use]
    pub fn last_ime_allowed(&self) -> Option<bool> {
        self.state.lock().ime_allowed_calls.last().copied()
    }

    /// Every `set_ime_cursor_area` call, in delivery order.
    #[must_use]
    pub fn cursor_area_calls(&self) -> Vec<Bounds<f64>> {
        self.state.lock().cursor_area_calls.clone()
    }
}

impl std::fmt::Debug for FakeTextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("FakeTextInput")
            .field("ime_allowed_calls", &state.ime_allowed_calls)
            .field("cursor_area_calls", &state.cursor_area_calls)
            .finish()
    }
}

impl PlatformTextInput for FakeTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.state.lock().ime_allowed_calls.push(allowed);
    }

    fn set_ime_cursor_area(&self, area: Bounds<f64>) {
        self.state.lock().cursor_area_calls.push(area);
    }
}

/// Recording fake for [`PlatformHaptics`], backing the headless backend's
/// [`PlatformWindow::haptics`].
///
/// Every `perform` call is appended to an in-memory history so a test can
/// assert exactly which feedback kinds the haptics bridge
/// (`flui-app`'s `UiRuntime::perform_haptic_feedback`, forwarded through
/// `PresentationState`) told the platform to perform, in delivery order,
/// rather than only that the call didn't panic.
#[derive(Default)]
pub struct FakeHaptics {
    calls: Mutex<Vec<HapticFeedback>>,
}

impl FakeHaptics {
    /// Create a fresh recorder with no recorded calls.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every `perform` call, in delivery order.
    #[must_use]
    pub fn calls(&self) -> Vec<HapticFeedback> {
        self.calls.lock().clone()
    }

    /// The most recent `perform` call, if any.
    #[must_use]
    pub fn last(&self) -> Option<HapticFeedback> {
        self.calls.lock().last().copied()
    }
}

impl std::fmt::Debug for FakeHaptics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeHaptics")
            .field("calls", &*self.calls.lock())
            .finish()
    }
}

impl PlatformHaptics for FakeHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        self.calls.lock().push(feedback);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Mock display for headless testing
struct MockDisplay {
    is_primary: bool,
}

impl MockDisplay {
    fn primary() -> Self {
        Self { is_primary: true }
    }
}

impl crate::traits::PlatformDisplay for MockDisplay {
    fn id(&self) -> crate::traits::DisplayId {
        crate::traits::DisplayId(0)
    }

    fn name(&self) -> String {
        "Mock Display".to_string()
    }

    fn bounds(&self) -> Bounds<i32> {
        // Mock display: 1920x1080 at origin (0, 0)
        Bounds::new(Point::new(0, 0), Size::new(1920, 1080))
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn is_primary(&self) -> bool {
        self.is_primary
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

    #[test]
    fn explicit_quit_reenters_and_drops_its_callback_outside_the_state_lock() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        struct CallbackDrop {
            platform: std::sync::Weak<HeadlessPlatform>,
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
        let platform = Arc::new(HeadlessPlatform::new());
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
            let _appearance = callback_platform.window_appearance();
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

    // ==================== US2 Tests ====================

    // ==================== US3 Tests ====================
}
