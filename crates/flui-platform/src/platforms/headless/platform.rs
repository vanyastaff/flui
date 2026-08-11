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

use anyhow::Result;
use cursor_icon::CursorIcon;
use flui_foundation::{ClaimSlot, claim_slot};
use flui_types::{
    HapticFeedback,
    geometry::{Bounds, DevicePixels, Pixels, Point, Size},
};
use parking_lot::Mutex;

use crate::{
    data_transfer::{DataTransferSource, NullDataTransferSource},
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
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
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
        };

        Self {
            capabilities: DesktopCapabilities,
            state: Arc::new(Mutex::new(state)),
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

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> anyhow::Result<()> {
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
        on_ready(OwnerPlatform::new(platform, hooks))?;

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

    fn open_window(&self, options: WindowOptions) -> Result<Arc<dyn PlatformWindow>> {
        tracing::info!(?options, "Creating mock window");

        let platform_state = Arc::downgrade(&self.state);
        Ok(self.with_state(|state| create_mock_window(state, platform_state, options)))
    }

    fn active_window(&self) -> Option<WindowId> {
        self.with_state(|state| state.active_window)
    }

    fn window_stack(&self) -> Option<Vec<WindowId>> {
        Some(self.with_state(|state| state.windows.iter().map(|w| w.id).collect()))
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

    fn app_path(&self) -> Result<PathBuf> {
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

/// Mock window for headless testing
///
/// Supports per-window callback registration and programmatic event injection
/// for testing without a display server.
#[derive(Clone)]
struct MockWindow {
    id: WindowId,
    state: Arc<Mutex<MockWindowState>>,
    callbacks: Arc<WindowCallbacks>,
    text_input: Arc<FakeTextInput>,
    haptics: Arc<FakeHaptics>,
    /// Back-reference to the platform this window was opened on, so
    /// closing it can remove its own entry from `HeadlessState::windows`
    /// and — only once every tracked window is gone — consult the
    /// exit-policy hook exactly like the winit backend's `CloseRequested`
    /// handling does (see `notify_closed`). `Weak`: a window must never
    /// keep the platform state alive past the platform's own lifetime.
    platform_state: Weak<Mutex<HeadlessState>>,
}

/// Mutable state for headless MockWindow
struct MockWindowState {
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
        let hook = {
            let mut state = platform_state.lock();
            state.windows.retain(|w| w.id != self.id);
            if state.active_window == Some(self.id) {
                state.active_window = state.windows.first().map(|w| w.id);
            }
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

    /// Inject a platform input event for testing.
    /// Fires the registered `on_input` callback.
    #[allow(dead_code)]
    pub fn inject_event(&self, event: PlatformInput) -> DispatchEventResult {
        self.callbacks.dispatch_input(event)
    }

    /// Simulate a resize for testing.
    /// Fires the registered `on_resize` callback.
    #[allow(dead_code)]
    pub fn simulate_resize(&self, width: f32, height: f32) {
        use flui_types::geometry::px;
        let size = Size::new(px(width), px(height));
        let scale = self.state.lock().scale_factor as f32;
        self.state.lock().bounds.size = size;
        self.callbacks.dispatch_resize(size, scale);
    }

    /// Simulate focus change for testing.
    /// Fires the registered `on_active_status_change` callback.
    #[allow(dead_code)]
    pub fn simulate_focus(&self, focused: bool) {
        self.state.lock().focused = focused;
        self.callbacks.dispatch_active_status_change(focused);
    }

    /// Simulate a visibility/occlusion change for testing.
    /// Fires the registered `on_visibility_status_change` callback.
    #[allow(dead_code)]
    pub fn simulate_visibility(&self, visible: bool) {
        self.state.lock().visible = visible;
        self.callbacks.dispatch_visibility_status_change(visible);
    }

    /// Simulate close request for testing.
    /// Fires `on_should_close`, then `on_close` if allowed, then (real,
    /// production behavior — not a test-only shortcut) removes this window
    /// from the platform's tracking and consults the exit-policy hook if
    /// every tracked window is now gone. See [`Self::notify_closed`].
    #[allow(dead_code)]
    pub fn simulate_close(&self) -> bool {
        let should = self.callbacks.dispatch_should_close();
        if should {
            self.callbacks.dispatch_close();
            self.notify_closed();
        }
        should
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
        self.callbacks.dispatch_close();
        self.notify_closed();
    }

    fn set_background_appearance(&self, _appearance: WindowBackgroundAppearance) {
        // No-op in headless mode
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.state.lock().cursor = cursor;
        Ok(())
    }

    // ==================== Callbacks (US1) ====================

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        *self.callbacks.on_input.lock() = Some(callback);
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_request_frame.lock() = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>) {
        *self.callbacks.on_resize.lock() = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_moved.lock() = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        *self.callbacks.on_close.lock() = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        *self.callbacks.on_should_close.lock() = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_active_status_change.lock() = Some(callback);
    }

    fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_visibility_status_change.lock() = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        *self.callbacks.on_hover_status_change.lock() = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        *self.callbacks.on_appearance_changed.lock() = Some(callback);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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
}
