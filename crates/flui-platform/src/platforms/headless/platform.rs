//! Headless platform implementation for testing
//!
//! This platform implementation runs without any actual windowing system,
//! making it ideal for unit tests and CI environments.

use std::{
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, ThreadId},
};

use cursor_icon::CursorIcon;
use flui_foundation::{ClaimSlot, claim_slot};
use flui_types::{
    HapticFeedback,
    geometry::{Bounds, DevicePixels, Pixels, Point, Size},
};
use parking_lot::Mutex;

use crate::{
    data_transfer::{DataTransferSource, NullDataTransferSource},
    error::PlatformError,
    shared::{PlatformHandlers, WindowCallbacks},
    traits::{
        Clipboard, ClipboardItem, CursorError, DesktopCapabilities, DispatchEventResult,
        OpenWindowError, OwnerPlatform, PendingWindow, Platform, PlatformCapabilities,
        PlatformDisplay, PlatformExecutor, PlatformHaptics, PlatformInput, PlatformReadyCallback,
        PlatformTextInput, PlatformWindow, WindowAppearance, WindowBackgroundAppearance,
        WindowBounds, WindowEvent, WindowId, WindowOpen, WindowOptions,
        owner::{ClosedTransport, DirectOwnerHooks, OwnerHooks, ProxyTransport},
    },
};

/// The value a deferred (or synchronous) window-open request resolves to —
/// mirrors the winit backend's own private `OpenWindowResult` alias
/// (`platforms/winit/control.rs`) so both backends complete a
/// [`ClaimSlot`]/[`PendingWindow`] pair with an identical shape.
type OpenWindowResult = Result<Arc<dyn PlatformWindow>, OpenWindowError>;

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
    capabilities: DesktopCapabilities,
    state: Arc<Mutex<HeadlessState>>,
}

struct HeadlessState {
    handlers: PlatformHandlers,
    background_executor: Arc<TestExecutor>,
    clipboard: Arc<MockClipboard>,
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
}

impl HeadlessPlatform {
    /// Create a new headless platform
    pub fn new() -> Self {
        let state = HeadlessState {
            handlers: PlatformHandlers::new(),
            background_executor: Arc::new(TestExecutor::new("background")),
            clipboard: Arc::new(MockClipboard::new()),
            active_window: None,
            is_running: false,
            windows: Vec::new(),
            appearance: WindowAppearance::default(),
            keyboard_layout: "en-US".to_string(),
            opened_urls: Vec::new(),
            deferred_window_open: false,
            pending_opens: Vec::new(),
            exit_reevaluation_requested: false,
        };

        Self {
            capabilities: DesktopCapabilities,
            state: Arc::new(Mutex::new(state)),
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

        self.with_state(|state| {
            state.is_running = true;
        });

        // Default: no owner lane on this backend, every
        // `OwnerPlatform::open_window` call creates directly and is always
        // `Ready` (ADR-0039 slice 2). "For the loop's life" means "until the
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
                owner_thread,
            })
        } else {
            Arc::new(DirectOwnerHooks::new(Arc::clone(&platform)))
        };

        // In headless mode, just call on_ready and return immediately. A
        // fallible bootstrap has nowhere else to go on this backend since
        // there is no loop to keep running with a half-built app --
        // propagate straight out of `run`.
        on_ready(OwnerPlatform::new(platform, hooks)).map_err(PlatformError::bootstrap)?;

        tracing::info!("Headless platform ready");
        Ok(())
    }

    fn quit(&self) {
        tracing::info!("Quitting headless platform");

        self.with_state(|state| {
            state.is_running = false;
            state.handlers.invoke_quit();
        });
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

    fn open_window(
        &self,
        options: WindowOptions,
    ) -> Result<Arc<dyn PlatformWindow>, OpenWindowError> {
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
) -> Arc<dyn PlatformWindow> {
    let window_id = next_headless_window_id();
    let window = MockWindow::new(window_id, options, platform_state);

    state.windows.push(window.clone());
    state.active_window = Some(window_id);

    state
        .handlers
        .invoke_window_event(WindowEvent::Created(window_id));

    Arc::new(window) as Arc<dyn PlatformWindow>
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
    state: Arc<Mutex<HeadlessState>>,
    owner_thread: ThreadId,
}

impl OwnerHooks for HeadlessDeferredOwnerHooks {
    fn open_owner_window(&self, options: WindowOptions) -> Result<WindowOpen, OpenWindowError> {
        // No wake-worthy event loop to notify on abandonment (this backend
        // never parks a real loop on the request) -- see `claim_slot`'s own
        // doc for why a no-op wake is the correct choice for a generic
        // caller like this one.
        let (slot, handle) = claim_slot::<OpenWindowResult>(Arc::new(|| {}));
        self.state.lock().pending_opens.push((slot, options));
        Ok(WindowOpen::Pending(PendingWindow::new(
            handle,
            self.owner_thread,
        )))
    }

    fn transport(&self) -> Arc<dyn ProxyTransport> {
        // This test mode defers window creation only; it does not add a
        // cross-thread request lane, so `PlatformProxy` stays permanently
        // unsupported here exactly as it is under `DirectOwnerHooks`.
        Arc::new(ClosedTransport::new(self.owner_thread))
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
        // embedder's runtime (dropping removed realm state whose
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

        let quit_callback = {
            let mut state = platform_state.lock();
            state.is_running = false;
            state.handlers.quit.take()
        };
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
    pub fn resolve_next(&self) -> Option<Arc<dyn PlatformWindow>> {
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
    bounds: Bounds<Pixels>,
    scale_factor: f64,
    focused: bool,
    visible: bool,
    maximized: bool,
    fullscreen: bool,
    hovered: bool,
    modifiers: keyboard_types::Modifiers,
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
            visible: self.visible,
            maximized: self.maximized,
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
                visible: options.visible,
                maximized: false,
                fullscreen: false,
                hovered: false,
                modifiers: keyboard_types::Modifiers::empty(),
                appearance: WindowAppearance::default(),
                cursor: CursorIcon::default(),
            })),
            callbacks: Arc::new(WindowCallbacks::new()),
            text_input: Arc::new(FakeTextInput::new()),
            haptics: Arc::new(FakeHaptics::new()),
            accessibility: Arc::new(FakeAccessibility::new()),
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
    /// realm state, whose destructors may call back into this platform (a
    /// dispose hook opening another window, say). Calling either callback
    /// while still holding `platform_state`'s lock would deadlock the
    /// instant such a callback re-entered any lock-guarded method (e.g.
    /// `open_window`).
    /// Leases the global window-event handler out of the platform state,
    /// invokes it with `event`, and restores it unless something fresher was
    /// installed meanwhile.
    ///
    /// Never invoked under the state lock. The handler re-enters the
    /// embedder's runtime — in `flui-app` it drops the removed realm's state,
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
        self.state.lock().visible = false;
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

        let quit_callback = {
            let mut state = platform_state.lock();
            state.is_running = false;
            state.handlers.quit.take()
        };
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
    pub fn simulate_resize(&self, width: f32, height: f32) {
        use flui_types::geometry::px;
        let size = Size::new(px(width), px(height));
        let scale = self.state.lock().scale_factor as f32;
        self.state.lock().bounds.size = size;
        self.callbacks.dispatch_resize(size, scale);
    }

    /// Simulate a monitor DPI change for testing: the scale factor moves
    /// with NO size change, and — matching the winit backend — the new
    /// scale is delivered through the resize path with the current logical
    /// size, because that path is the only one the realm learns its
    /// device-pixel ratio from.
    pub fn simulate_scale_factor_change(&self, scale_factor: f64) {
        let size = {
            let mut state = self.state.lock();
            state.scale_factor = scale_factor;
            state.bounds.size
        };
        self.callbacks.dispatch_resize(size, scale_factor as f32);
    }

    /// Simulate focus change for testing.
    /// Fires the registered `on_active_status_change` callback.
    pub fn simulate_focus(&self, focused: bool) {
        self.state.lock().focused = focused;
        self.callbacks.dispatch_active_status_change(focused);
    }

    /// Simulate a visibility/occlusion change for testing.
    /// Fires the registered `on_visibility_status_change` callback.
    pub fn simulate_visibility(&self, visible: bool) {
        self.state.lock().visible = visible;
        self.callbacks.dispatch_visibility_status_change(visible);
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
/// re-enters the embedder's runtime, which drops realm state whose destructors
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

impl crate::traits::PlatformWindow for MockWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<DevicePixels> {
        use flui_types::geometry::device_px;

        let state = self.state.lock();
        Size::new(
            device_px((state.bounds.size.width.0 * state.scale_factor as f32) as i32),
            device_px((state.bounds.size.height.0 * state.scale_factor as f32) as i32),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
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

    fn is_visible(&self) -> bool {
        self.state.lock().visible
    }

    // ==================== Query Methods (US2) ====================

    fn bounds(&self) -> Bounds<Pixels> {
        self.state.lock().bounds
    }

    fn content_size(&self) -> Size<Pixels> {
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

    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }

    fn modifiers(&self) -> keyboard_types::Modifiers {
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

    fn accessibility(&self) -> Option<Arc<dyn crate::traits::PlatformAccessibility>> {
        Some(Arc::clone(&self.accessibility) as Arc<dyn crate::traits::PlatformAccessibility>)
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

    fn activate(&self) {
        self.state.lock().focused = true;
    }

    fn minimize(&self) {
        let mut state = self.state.lock();
        state.maximized = false;
        state.fullscreen = false;
    }

    fn maximize(&self) {
        let mut state = self.state.lock();
        state.maximized = true;
        state.fullscreen = false;
    }

    fn restore(&self) {
        let mut state = self.state.lock();
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

    fn resize(&self, size: Size<Pixels>) {
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

/// Recording fake for [`PlatformAccessibility`](crate::traits::PlatformAccessibility),
/// backing the headless backend's [`PlatformWindow::accessibility`].
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
    cursor_area_calls: Vec<Bounds<Pixels>>,
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
    pub fn cursor_area_calls(&self) -> Vec<Bounds<Pixels>> {
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

    fn set_ime_cursor_area(&self, area: Bounds<Pixels>) {
        self.state.lock().cursor_area_calls.push(area);
    }
}

/// Recording fake for [`PlatformHaptics`], backing the headless backend's
/// [`PlatformWindow::haptics`].
///
/// Every `perform` call is appended to an in-memory history so a test can
/// assert exactly which feedback kinds the haptics bridge
/// (`flui-app`'s `UiRealm::perform_haptic_feedback`, forwarded through
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

    fn bounds(&self) -> Bounds<DevicePixels> {
        use flui_types::geometry::device_px;

        // Mock display: 1920x1080 at origin (0, 0)
        Bounds::new(
            Point::new(device_px(0), device_px(0)),
            Size::new(device_px(1920), device_px(1080)),
        )
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

/// Mock clipboard with in-memory storage
struct MockClipboard {
    content: Mutex<Option<String>>,
}

impl MockClipboard {
    fn new() -> Self {
        Self {
            content: Mutex::new(None),
        }
    }
}

impl Clipboard for MockClipboard {
    fn read_text(&self) -> Option<String> {
        self.content.lock().clone()
    }

    fn write_text(&self, text: String) {
        *self.content.lock() = Some(text);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    #[test]
    fn separate_headless_platforms_mint_distinct_window_ids() {
        let first_platform = HeadlessPlatform::new();
        let second_platform = HeadlessPlatform::new();

        let first = first_platform
            .open_window(WindowOptions::default())
            .expect("first mock window opens");
        let second = second_platform
            .open_window(WindowOptions::default())
            .expect("second mock window opens");

        assert_ne!(
            first.id(),
            second.id(),
            "window identity must be process-unique, not scoped to one mock platform instance"
        );
    }

    /// Drives `OwnerPlatform::open_window` through the exact arm the real
    /// winit owner lane returns for any call outside `on_ready`
    /// (`WindowOpen::Pending`) instead of the default synchronous `Ready`
    /// this backend otherwise always returns — the probe
    /// `HeadlessPlatform::enable_deferred_window_open` exists for: without
    /// it, nothing in this backend ever exercises the `Pending` completion
    /// path a caller like `flui-app`'s `open_secondary_window` must handle.
    #[test]
    fn deferred_window_open_resolves_via_the_pending_arm_like_the_real_winit_owner_lane() {
        let platform = HeadlessPlatform::new();
        let deferred = platform.enable_deferred_window_open();

        let opened: Arc<Mutex<Option<Arc<dyn PlatformWindow>>>> = Arc::new(Mutex::new(None));
        let opened_for_ready = Arc::clone(&opened);

        Box::new(platform)
            .run(Box::new(move |owner: OwnerPlatform| {
                let open = owner
                    .open_window(WindowOptions::default())
                    .expect("deferred mode still accepts the request");
                let mut pending = match open {
                    WindowOpen::Pending(pending) => pending,
                    WindowOpen::Ready(_) => {
                        panic!("deferred mode must return Pending, not Ready")
                    }
                };

                assert!(
                    pending.try_take().is_none(),
                    "nothing delivered before resolve_next runs"
                );
                let resolved_window = deferred
                    .resolve_next()
                    .expect("exactly one request was pending");
                assert!(
                    deferred.resolve_next().is_none(),
                    "a second resolve_next has nothing left to resolve"
                );

                let delivered_window = pending
                    .try_take()
                    .expect("resolve_next delivers synchronously")
                    .expect("mock window creation cannot fail");
                assert!(
                    Arc::ptr_eq(&resolved_window, &delivered_window),
                    "resolve_next's own return value must be the SAME window it delivered \
                     through the PendingWindow, not a second, different one"
                );
                *opened_for_ready.lock() = Some(delivered_window);

                Ok(())
            }))
            .expect("run succeeds");

        assert!(
            opened.lock().is_some(),
            "the Pending request resolved to a real mock window"
        );
    }

    #[test]
    fn test_headless_platform_creation() {
        let platform = HeadlessPlatform::new();
        assert_eq!(platform.name(), "Headless");
        assert!(platform.active_window().is_none());
    }

    #[test]
    fn test_mock_clipboard() {
        let clipboard = MockClipboard::new();
        assert_eq!(clipboard.read_text(), None);

        clipboard.write_text("test".to_string());
        assert_eq!(clipboard.read_text(), Some("test".to_string()));
    }

    #[test]
    fn test_mock_window_creation() {
        let platform = HeadlessPlatform::new();

        let options = WindowOptions {
            title: "Test".to_string(),
            size: Size::new(
                flui_types::geometry::px(800.0),
                flui_types::geometry::px(600.0),
            ),
            ..Default::default()
        };

        let window = platform.open_window(options).unwrap();
        assert_eq!(
            window.logical_size(),
            Size::new(
                flui_types::geometry::px(800.0),
                flui_types::geometry::px(600.0)
            )
        );
        assert!(window.is_focused());
        assert!(window.is_visible());
    }

    #[test]
    fn test_on_input_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        use crate::traits::{DispatchEventResult, PlatformInput};

        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        window.on_input(Box::new(move |_event| {
            called_clone.store(true, Ordering::SeqCst);
            DispatchEventResult::default()
        }));

        // Inject a keyboard event
        let event = PlatformInput::Keyboard(ui_events::keyboard::KeyboardEvent {
            state: ui_events::keyboard::KeyState::Down,
            key: crate::traits::Key::Named(keyboard_types::NamedKey::Enter),
            code: ui_events::keyboard::Code::Unidentified,
            location: ui_events::keyboard::Location::Standard,
            modifiers: keyboard_types::Modifiers::empty(),
            repeat: false,
            is_composing: false,
        });

        window.inject_event(event);
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_on_resize_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        window.on_resize(Box::new(move |size, _scale| {
            assert_eq!(size.width.0, 1024.0);
            assert_eq!(size.height.0, 768.0);
            called_clone.store(true, Ordering::SeqCst);
        }));

        window.simulate_resize(1024.0, 768.0);
        assert!(called.load(Ordering::SeqCst));
    }

    /// A DPI change with no size change must still reach the resize
    /// callback — that path is the only one the realm learns its
    /// device-pixel ratio from, so a scale-only signal that bypassed it
    /// would leave the realm rendering at the stale ratio until some
    /// unrelated resize arrived.
    #[test]
    fn a_scale_only_change_delivers_the_current_size_at_the_new_scale() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());
        let size_before = window.logical_size();

        let called = Arc::new(AtomicBool::new(false));
        let called_probe = called.clone();
        window.on_resize(Box::new(move |size, scale| {
            assert_eq!(size, size_before, "the size is unchanged");
            assert_eq!(scale, 2.0, "the new scale rides the resize path");
            called_probe.store(true, Ordering::SeqCst);
        }));

        window.simulate_scale_factor_change(2.0);
        assert!(called.load(Ordering::SeqCst));
        assert_eq!(
            window.scale_factor(),
            2.0,
            "the window reports the new scale"
        );
    }

    /// The global window-event handler sees the same lifecycle events the
    /// production backends emit, in the same order, and each route emits the
    /// set that belongs to it.
    ///
    /// Both winit (`WinitWindowEvent::CloseRequested` →
    /// `complete_window_close`) and Win32 (`WM_CLOSE` → `WM_DESTROY`) emit
    /// `CloseRequested` before `Closed` on the user route. Neither emits
    /// `CloseRequested` for a close the *application* initiated on the owning
    /// thread. Before issue #924 the headless double emitted neither event on
    /// either route, so a consumer written to that contract could not be
    /// tested against it at all.
    ///
    /// Asserted as one recorded sequence rather than as independent counters:
    /// the ordering is half the contract, and two flags cannot express it.
    #[test]
    fn both_close_routes_emit_the_lifecycle_events_that_belong_to_them() {
        let platform = HeadlessPlatform::new();
        let seen = Arc::new(Mutex::new(Vec::<WindowEvent>::new()));
        let seen_for_handler = Arc::clone(&seen);
        platform.on_window_event(Box::new(move |event| {
            seen_for_handler.lock().push(event);
        }));

        let programmatic = platform
            .open_window(WindowOptions::default())
            .expect("headless opens a window");
        let user = platform
            .open_window(WindowOptions::default())
            .expect("headless opens a second window");
        seen.lock().clear(); // drop the two `Created` events

        programmatic.close();
        let after_programmatic = seen.lock().clone();
        assert!(
            matches!(after_programmatic.as_slice(), [WindowEvent::Closed(id)] if *id == programmatic.id()),
            "the application's own close emits `Closed` for that window and \
             nothing else — it was never a request. Saw {after_programmatic:?}"
        );

        // The user route, through the same window callbacks a compositor
        // close would drive.
        let user_id = user.id();
        let user = platform
            .with_state(|state| state.windows.iter().find(|w| w.id == user_id).cloned())
            .expect("the second window is still tracked");
        assert!(user.simulate_close(), "nothing vetoes this close");

        let all = seen.lock().clone();
        assert!(
            matches!(
                all.as_slice(),
                [
                    WindowEvent::Closed(_),
                    WindowEvent::CloseRequested { window_id: asked },
                    WindowEvent::Closed(reported),
                ] if *asked == user_id && *reported == user_id
            ),
            "the user route asks first and reports the close after it, both \
             for the window that closed: saw {all:?}"
        );
    }

    /// `Closed` reaches the handler for a window that is *not* the last one.
    ///
    /// The exit-policy consult is gated on the registry emptying, and the
    /// obvious way to write this teardown is to return early when it has not.
    /// Doing so would skip the event for every close but the final one —
    /// leaving a consumer that tracks windows by these events believing a
    /// closed window is still open, which is the multi-window case they exist
    /// for.
    #[test]
    fn a_non_last_window_close_still_reports_closed() {
        let platform = HeadlessPlatform::new();
        let closed = Arc::new(Mutex::new(Vec::new()));
        let closed_for_handler = Arc::clone(&closed);
        platform.on_window_event(Box::new(move |event| {
            if let WindowEvent::Closed(id) = event {
                closed_for_handler.lock().push(id);
            }
        }));

        let first = platform
            .open_window(WindowOptions::default())
            .expect("headless opens a window");
        let _second = platform
            .open_window(WindowOptions::default())
            .expect("headless opens a second window");

        first.close();

        assert_eq!(
            closed.lock().as_slice(),
            &[first.id()],
            "the first of two windows must report its own close, even though \
             one window remains and the exit policy is not consulted"
        );
    }

    /// A window opened from inside the `Closed` handler vetoes the exit by
    /// existing — the emptiness that decides it is read where it is used.
    ///
    /// Before the global events existed, the window's removal, the emptiness
    /// check and taking the exit hook happened in **one** lock acquisition,
    /// with a comment saying why: nothing may open a window in between.
    /// Emitting `Closed` splits that acquisition, and the code running in the
    /// gap is a consumer callback — precisely the thing that opens windows.
    /// Carrying the earlier answer down would consult the hook, and quit, with
    /// a window tracked.
    ///
    /// The shape is not hypothetical: a close handler that shows the next
    /// window is the ordinary reason to register one.
    #[test]
    fn a_window_opened_from_the_closed_handler_prevents_the_exit_consult() {
        let platform = Arc::new(HeadlessPlatform::new());
        let hook_calls = Arc::new(AtomicUsize::new(0));
        let hook_calls_for_hook = Arc::clone(&hook_calls);
        platform.set_exit_policy_hook(Box::new(move || {
            hook_calls_for_hook.fetch_add(1, Ordering::SeqCst);
            true
        }));

        let reopened = Arc::new(Mutex::new(None));
        let platform_for_handler = Arc::clone(&platform);
        let reopened_for_handler = Arc::clone(&reopened);
        platform.on_window_event(Box::new(move |event| {
            if matches!(event, WindowEvent::Closed(_)) {
                // The replacement opens while the closing window has already
                // left the registry and the exit hook has not been consulted.
                *reopened_for_handler.lock() = platform_for_handler
                    .open_window(WindowOptions::default())
                    .ok();
            }
        }));

        let only = platform
            .open_window(WindowOptions::default())
            .expect("headless opens a window");
        only.close();

        assert!(
            reopened.lock().is_some(),
            "precondition: the handler must have opened the replacement, or \
             this test asserts nothing"
        );
        assert_eq!(
            hook_calls.load(Ordering::SeqCst),
            0,
            "a window exists again by the time the exit decision is taken, so \
             the hook must not be consulted at all"
        );
    }

    /// A vetoed close emits neither event and tears nothing down.
    ///
    /// Green before issue #924 as well — the double emitted no events at all —
    /// so this is a regression guard, not evidence for that change. It is here
    /// because the veto branch is the one place `CloseRequested` must NOT be
    /// emitted, and nothing else asserts that.
    #[test]
    fn a_vetoed_close_emits_no_lifecycle_event() {
        let platform = HeadlessPlatform::new();
        let seen = Arc::new(Mutex::new(Vec::<WindowEvent>::new()));
        let seen_for_handler = Arc::clone(&seen);
        platform.on_window_event(Box::new(move |event| {
            seen_for_handler.lock().push(event);
        }));

        let window = platform
            .open_window(WindowOptions::default())
            .expect("headless opens a window");
        seen.lock().clear();

        let window_id = window.id();
        let tracked = platform
            .with_state(|state| state.windows.iter().find(|w| w.id == window_id).cloned())
            .expect("the window is tracked");
        tracked.on_should_close(Box::new(|| false));
        assert!(!tracked.simulate_close(), "the veto refuses the close");

        assert!(
            seen.lock().is_empty(),
            "a refused close is not a close: saw {:?}",
            seen.lock()
        );
    }

    #[test]
    fn test_on_should_close_veto() {
        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());

        // Register a callback that vetoes close
        window.on_should_close(Box::new(|| false));

        // Close should be vetoed
        assert!(!window.simulate_close());
    }

    #[test]
    fn test_on_close_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());

        let closed = Arc::new(AtomicBool::new(false));
        let closed_clone = closed.clone();

        window.on_close(Box::new(move || {
            closed_clone.store(true, Ordering::SeqCst);
        }));

        // No on_should_close registered → defaults to allow
        assert!(window.simulate_close());
        assert!(closed.load(Ordering::SeqCst));
    }

    #[test]
    fn test_on_active_status_change() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());

        let focused = Arc::new(AtomicBool::new(false));
        let focused_clone = focused.clone();

        window.on_active_status_change(Box::new(move |is_active| {
            focused_clone.store(is_active, Ordering::SeqCst);
        }));

        window.simulate_focus(true);
        assert!(focused.load(Ordering::SeqCst));

        window.simulate_focus(false);
        assert!(!focused.load(Ordering::SeqCst));
    }

    #[test]
    fn test_on_visibility_status_change() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());

        let visible = Arc::new(AtomicBool::new(true));
        let visible_clone = visible.clone();

        window.on_visibility_status_change(Box::new(move |is_visible| {
            visible_clone.store(is_visible, Ordering::SeqCst);
        }));

        window.simulate_visibility(false);
        assert!(!visible.load(Ordering::SeqCst));

        window.simulate_visibility(true);
        assert!(visible.load(Ordering::SeqCst));
    }

    // ==================== US2 Tests ====================

    #[test]
    fn test_set_title_get_title() {
        let window = MockWindow::new(
            WindowId(0),
            WindowOptions {
                title: "Original".to_string(),
                ..Default::default()
            },
            Weak::new(),
        );

        assert_eq!(window.get_title(), "Original");
        window.set_title("Updated");
        assert_eq!(window.get_title(), "Updated");
    }

    #[test]
    fn test_window_bounds_query() {
        use flui_types::geometry::px;

        let window = MockWindow::new(
            WindowId(0),
            WindowOptions {
                size: Size::new(px(800.0), px(600.0)),
                ..Default::default()
            },
            Weak::new(),
        );

        let bounds = window.bounds();
        assert_eq!(bounds.size.width.0, 800.0);
        assert_eq!(bounds.size.height.0, 600.0);

        assert_eq!(window.content_size(), Size::new(px(800.0), px(600.0)));

        match window.window_bounds() {
            WindowBounds::Windowed(b) => assert_eq!(b.size.width.0, 800.0),
            _ => panic!("Expected Windowed"),
        }
    }

    #[test]
    fn test_maximize_restore_fullscreen() {
        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());

        assert!(!window.is_maximized());
        assert!(!window.is_fullscreen());

        window.maximize();
        assert!(window.is_maximized());
        assert!(!window.is_fullscreen());
        assert!(matches!(window.window_bounds(), WindowBounds::Maximized(_)));

        window.restore();
        assert!(!window.is_maximized());
        assert!(matches!(window.window_bounds(), WindowBounds::Windowed(_)));

        window.toggle_fullscreen();
        assert!(window.is_fullscreen());
        assert!(!window.is_maximized());
        assert!(matches!(
            window.window_bounds(),
            WindowBounds::Fullscreen(_)
        ));

        window.toggle_fullscreen();
        assert!(!window.is_fullscreen());
    }

    #[test]
    fn test_resize() {
        use flui_types::geometry::px;

        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());
        window.resize(Size::new(px(1920.0), px(1080.0)));
        assert_eq!(window.logical_size(), Size::new(px(1920.0), px(1080.0)));
    }

    #[test]
    fn test_display_query() {
        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());
        let display = window.display();
        assert!(display.is_some());
        assert!(display.unwrap().is_primary());
    }

    #[test]
    fn text_input_reaches_the_same_fake_across_calls_and_records_delivered_values() {
        use flui_types::geometry::{Bounds, Point, Size, px};

        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());
        let fake = Arc::clone(&window.text_input);
        let text_input = window.text_input().expect("headless backend supports IME");

        text_input.set_ime_allowed(true);
        text_input.set_ime_cursor_area(Bounds::new(
            Point::new(px(10.0), px(20.0)),
            Size::new(px(30.0), px(40.0)),
        ));
        window
            .text_input()
            .expect("headless backend supports IME")
            .set_ime_allowed(false);

        // A second accessor call must reach the SAME fake, not a fresh one —
        // otherwise every caller would see its own writes disappear.
        assert_eq!(fake.ime_allowed_calls(), vec![true, false]);
    }

    #[test]
    fn haptics_records_calls_in_delivery_order_and_last_reflects_the_most_recent() {
        let fake = FakeHaptics::new();
        assert_eq!(fake.calls(), Vec::new(), "no calls recorded yet");
        assert_eq!(fake.last(), None);

        fake.perform(HapticFeedback::SelectionClick);
        fake.perform(HapticFeedback::HeavyImpact);
        fake.perform(HapticFeedback::ErrorNotification);

        assert_eq!(
            fake.calls(),
            vec![
                HapticFeedback::SelectionClick,
                HapticFeedback::HeavyImpact,
                HapticFeedback::ErrorNotification,
            ]
        );
        assert_eq!(fake.last(), Some(HapticFeedback::ErrorNotification));
    }

    #[test]
    fn haptics_reaches_the_same_fake_across_calls() {
        let window = MockWindow::new(WindowId(0), WindowOptions::default(), Weak::new());
        let haptics = window.haptics().expect("headless backend supports haptics");

        haptics.perform(HapticFeedback::LightImpact);

        // A second accessor call must reach the SAME fake, not a fresh one
        // — otherwise a test driving the binding through `PlatformWindow`
        // and a test asserting on the fake directly would silently diverge.
        let calls = window
            .haptics()
            .expect("headless backend supports haptics")
            .as_any()
            .downcast_ref::<FakeHaptics>()
            .expect("headless PlatformHaptics is a FakeHaptics")
            .calls();
        assert_eq!(calls, vec![HapticFeedback::LightImpact]);
    }

    // ==================== US3 Tests ====================

    #[test]
    fn test_window_appearance() {
        let platform = HeadlessPlatform::new();
        assert_eq!(platform.window_appearance(), WindowAppearance::Light);
    }

    #[test]
    fn cursor_state_is_owned_by_each_exact_window() {
        let first = MockWindow::new(WindowId(1), WindowOptions::default(), Weak::new());
        let second = MockWindow::new(WindowId(2), WindowOptions::default(), Weak::new());

        first
            .set_cursor(CursorIcon::Pointer)
            .expect("headless window supports cursor selection");
        second
            .set_cursor(CursorIcon::Text)
            .expect("headless window supports cursor selection");

        assert_eq!(first.state.lock().cursor, CursorIcon::Pointer);
        assert_eq!(second.state.lock().cursor, CursorIcon::Text);
    }

    #[test]
    fn test_clipboard_item_roundtrip() {
        let platform = HeadlessPlatform::new();
        let item = ClipboardItem::text("hello world".to_string());
        platform.write_to_clipboard(item);
        let read = platform.read_from_clipboard();
        assert!(read.is_some());
        assert_eq!(read.unwrap().text_content(), Some("hello world"));
    }

    #[test]
    fn test_open_url_tracking() {
        let platform = HeadlessPlatform::new();
        platform.open_url("https://example.com");
        platform.open_url("https://rust-lang.org");
        platform.with_state(|state| {
            assert_eq!(state.opened_urls.len(), 2);
            assert_eq!(state.opened_urls[0], "https://example.com");
            assert_eq!(state.opened_urls[1], "https://rust-lang.org");
        });
    }

    #[test]
    fn test_keyboard_layout() {
        let platform = HeadlessPlatform::new();
        assert_eq!(platform.keyboard_layout(), "en-US");
    }

    #[test]
    fn test_keyboard_layout_change_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let platform = HeadlessPlatform::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        platform.on_keyboard_layout_change(Box::new(move || {
            called_clone.store(true, Ordering::SeqCst);
        }));

        // Simulate a keyboard layout change by invoking the handler
        platform.with_state(|state| {
            state.handlers.invoke_keyboard_layout_changed();
        });

        assert!(called.load(Ordering::SeqCst));
    }

    /// The park/probe/drive contract behind
    /// `Platform::request_exit_policy_reevaluation` on this mock: a request
    /// from a worker thread parks (coalesced); the owner-thread drive
    /// consumes it and is gated first on the window map being empty, then
    /// on the exit-policy hook's answer — a veto leaves the hook installed
    /// for the next consult (not one-shot), and an allow quits exactly
    /// once through the registered `on_quit`.
    #[test]
    fn exit_reevaluation_parks_from_any_thread_and_drives_through_the_hook() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let platform = HeadlessPlatform::new();
        let reevaluation = platform.exit_reevaluation();

        let allow_exit = Arc::new(AtomicBool::new(false));
        let allow_for_hook = Arc::clone(&allow_exit);
        platform.set_exit_policy_hook(Box::new(move || allow_for_hook.load(Ordering::SeqCst)));
        let quit_calls = Arc::new(AtomicUsize::new(0));
        let quit_for_handler = Arc::clone(&quit_calls);
        platform.on_quit(Box::new(move || {
            quit_for_handler.fetch_add(1, Ordering::SeqCst);
        }));

        let window = platform
            .open_window(WindowOptions::default())
            .expect("mock window opens");

        // Requests park from ANY thread — the production caller is a
        // worker-pool thread observing a keep-alive service's exit.
        std::thread::scope(|scope| {
            scope.spawn(|| {
                platform.request_exit_policy_reevaluation();
                platform.request_exit_policy_reevaluation();
            });
        });
        assert!(
            reevaluation.requested(),
            "the request must park (coalesced)"
        );

        // A window is still tracked: the drive consumes the request and
        // must NOT consult its way to a quit.
        assert!(!reevaluation.drive());
        assert_eq!(quit_calls.load(Ordering::SeqCst), 0);
        assert!(
            !reevaluation.requested(),
            "the drive consumes the parked request even when windows remain"
        );

        // Close the only window (the real trait close, which unregisters
        // it and consults the hook); the hook still vetoes, so no quit.
        window.close();
        assert_eq!(
            quit_calls.load(Ordering::SeqCst),
            0,
            "hook vetoes the close"
        );

        // Re-evaluation while the hook still vetoes: no quit, and the hook
        // must be restored (not consumed) for the next consult.
        platform.request_exit_policy_reevaluation();
        assert!(!reevaluation.drive());
        assert_eq!(quit_calls.load(Ordering::SeqCst), 0);

        // The holder releases: the next re-evaluation quits, exactly once.
        allow_exit.store(true, Ordering::SeqCst);
        platform.request_exit_policy_reevaluation();
        assert!(
            reevaluation.drive(),
            "with zero windows and the hook allowing, the drive must quit"
        );
        assert_eq!(quit_calls.load(Ordering::SeqCst), 1);
        assert!(
            !reevaluation.drive(),
            "no parked request remains; the drive is idempotent"
        );
        assert_eq!(quit_calls.load(Ordering::SeqCst), 1);
    }
}
