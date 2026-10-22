//! Headless platform implementation for testing
//!
//! This platform implementation runs without any actual windowing system,
//! making it ideal for unit tests and CI environments. Its loop and owner
//! turns are in [`crate::owner::headless`]; this file holds the windows,
//! devices, displays, preferences and host events a test drives.

use std::{
    rc::Rc,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::InMemoryClipboard;
pub use flui_platform::haptics::FakeHaptics;
use flui_platform::input_regions::{DeviceInputRegion, InputRegion, ProjectedInputRegions};
use flui_platform::text_store::TextStoreHost;
use parking_lot::Mutex;

use super::window_policy::HeadlessWindowPolicy;
use crate::display::headless::{DisplayChange, DisplayChangeError, DisplaySpec, HeadlessDisplays};
use crate::host::{self, Host};
use crate::owner::call_point::{Field, call, install};
use crate::owner::headless::{HeadlessLoop, HeadlessState, consult_exit_policy, emit_window_event};
use crate::platform::HostPlatform;
use crate::{
    error::PlatformError, host_window::HostWindow, owner::OpenWindowError,
    owner::handlers::WindowCallbacks, shared::window_state::WindowStateCell,
};
use flui_platform::{
    Clipboard, CursorError, DispatchEventResult, DisplayId, DisplaySnapshot, PlatformHaptics,
    PlatformInput, PlatformTextInput, PlatformWindow, SessionEndAnswer, SessionEndPhase,
    SizeConstraints, SystemNotice, WindowAppearance, WindowBackgroundAppearance,
    WindowCapabilities, WindowCommand, WindowCommandError, WindowEvent, WindowGeometry, WindowId,
    WindowMode, WindowOptions, WindowState, data_transfer::DataTransferSource,
};

/// Builds the text-store host a headless window offers
/// ([`HeadlessPlatform::with_text_store_host`](crate::HeadlessPlatform::with_text_store_host)). Shared with every window,
/// so `Send + Sync`; what it builds is owner-thread state.
pub(crate) type TextStoreHostFactory = Arc<dyn Fn() -> Rc<dyn TextStoreHost> + Send + Sync>;

/// Process-wide identity source for mock windows.
///
/// A headless test may construct several independent [`HeadlessPlatform`](crate::HeadlessPlatform)
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

/// The headless platform's test controls: builders and handles taken before
/// [`Platform::run`](crate::Platform::run) consumes it.
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
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
        self.owner()
            .with_state(|state| state.text_store_host = Some(Arc::new(factory)));
        self
    }
    /// Report `availability` as what this backend implements (its device
    /// presence is ignored: attached devices come from
    /// [`input_devices`](Self::input_devices)). Without it the headless
    /// backend reports pen, precise scrolling and trackpad gestures `Native`,
    /// since a test can synthesize all of them, and input regions and outgoing
    /// drags `Unavailable`.
    #[must_use]
    pub fn with_input_availability(
        self,
        availability: flui_platform::input_capabilities::InputCapabilities,
    ) -> Self {
        self.owner()
            .with_state(|state| state.input_availability = availability);
        self
    }

    /// Let this backend's windows start OS drag sessions, starting them at
    /// `timing` (desktop: `OnLeavingWindow`; iOS and the web: `AtStart`), and
    /// report outgoing drags `Native`. Without it its windows have no drag
    /// source, as a Linux/X11 window has none. The test plays the OS through
    /// [`drag_and_drop`](Self::drag_and_drop).
    #[must_use]
    pub fn with_os_drag(self, timing: flui_platform::drag::OsDragStart) -> Self {
        self.owner().with_state(|state| {
            state.drag.enable(timing);
            state.input_availability = state
                .input_availability
                .to_builder()
                .outgoing_drag(flui_platform::input_capabilities::Availability::Native)
                .build();
        });
        self
    }

    /// Place this backend's windows on one desktop, as Win32 and AppKit place
    /// theirs: each window's content starts at the position its published
    /// [`WindowState`] reports (this host draws no frame; a window opens where
    /// its options place it or at the desktop origin, and
    /// [`MockWindow::simulate_move`], a move command or a mode change moves
    /// it), the window opened last is topmost, and [`PlatformWindow::window_at`] answers from
    /// that layout. Without it `window_at` answers
    /// [`WindowAt::Unknown`](flui_platform::WindowAt::Unknown), as on Wayland.
    #[must_use]
    pub fn with_window_placement(self) -> Self {
        self.owner().with_state(|state| state.placement = true);
        self
    }

    /// The simulated OS side of drag-and-drop, valid across [`Platform::run`](crate::Platform::run).
    #[must_use]
    pub fn drag_and_drop(&self) -> super::drag::HeadlessDragAndDrop {
        super::drag::HeadlessDragAndDrop::new(
            self.owner().with_state(|state| Arc::clone(&state.drag)),
        )
    }

    /// A handle that attaches and removes simulated input devices, valid
    /// across [`Platform::run`](crate::Platform::run). Each change reaches every open window as a
    /// device event and a capability change, in order with its other input;
    /// a window opened later has the devices in its initial snapshot.
    #[must_use]
    pub fn input_devices(&self) -> HeadlessInputDevices {
        HeadlessInputDevices {
            state: Arc::downgrade(&self.owner().state),
        }
    }

    /// Replace the displays this platform reports with `displays`, laid out
    /// left to right; the first is primary. Windows opened afterwards start on
    /// the primary display.
    #[must_use]
    pub fn with_displays(self, displays: Vec<DisplaySpec>) -> Self {
        self.owner()
            .with_state(|state| state.displays = HeadlessDisplays::new(displays));
        self
    }

    /// Make every window this platform opens from now on honour exactly
    /// `capabilities`, to test a caller against a host that cannot do
    /// everything. The default is [`WindowCapabilities::ALL`]. A command whose
    /// capability is `false` is refused as unsupported and changes nothing.
    #[must_use]
    pub fn with_window_capabilities(self, capabilities: WindowCapabilities) -> Self {
        self.owner()
            .with_state(|state| state.window_capabilities = capabilities);
        self
    }

    /// Make the host clamp or decline what windows opened from now on ask
    /// for, to test that a snapshot reports the outcome and not the request.
    /// The default imposes nothing.
    #[must_use]
    pub fn with_window_policy(self, limits: HeadlessWindowPolicy) -> Self {
        self.owner()
            .with_state(|state| state.window_policy = limits);
        self
    }

    /// What the system reports as its preferences from the start, before any
    /// window exists. The default reports every field unknown.
    #[must_use]
    pub fn with_preferences(self, values: flui_platform::SystemPreferences) -> Self {
        self.owner().with_state(|state| state.preferences = values);
        self
    }

    /// Obtain the preference source before `run` consumes this platform: the
    /// handle changes the system's preferences the way an operating system
    /// would, from any thread. See [`HeadlessPreferences`].
    #[must_use]
    pub fn preference_source(&self) -> HeadlessPreferences {
        HeadlessPreferences {
            signal: Arc::downgrade(&self.owner().signal),
            state: Arc::downgrade(&self.owner().state),
        }
    }

    /// Obtain the display controls before `run` consumes this platform: the
    /// handle changes a display the way an operating system would, from any
    /// thread. See [`HeadlessDisplayControls`].
    #[must_use]
    pub fn display_controls(&self) -> HeadlessDisplayControls {
        HeadlessDisplayControls {
            signal: Arc::downgrade(&self.owner().signal),
            state: Arc::downgrade(&self.owner().state),
        }
    }

    /// Obtain the host-event handle before `run` consumes this platform: the
    /// handle reports session ends and system notices the way an operating
    /// system does. See [`HeadlessHostEvents`].
    #[must_use]
    pub fn host_events(&self) -> HeadlessHostEvents {
        HeadlessHostEvents {
            state: Arc::downgrade(&self.owner().state),
        }
    }

    /// Simulate one phase of the user's session ending; see
    /// [`HeadlessHostEvents::session_end`].
    pub fn simulate_session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        self.host_events().session_end(phase)
    }

    /// Simulate a system notice; see [`HeadlessHostEvents::notice`].
    pub fn simulate_host_notice(&self, notice: SystemNotice) {
        self.host_events().notice(notice);
    }
}

impl host::Window<HeadlessLoop> for MockWindow {
    fn open(
        owner: &HeadlessLoop,
        options: WindowOptions,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        tracing::info!(?options, "Creating mock window");

        let platform_state = Arc::downgrade(&owner.state);
        let window = owner.with_state(|state| create_mock_window(state, platform_state, options));
        // Announced once the state lock is released: the handler may open
        // another window, install a handler or quit.
        emit_window_event(&owner.state, WindowEvent::Created(window.id()));
        Ok(window)
    }

    fn active(owner: &HeadlessLoop) -> Option<WindowId> {
        owner.with_state(|state| state.active_window)
    }

    fn capabilities(owner: &HeadlessLoop) -> WindowCapabilities {
        owner.with_state(|state| state.window_capabilities)
    }

    fn frame_inset(_: &HeadlessLoop, _: f64) -> Size<f64> {
        // A headless window draws no frame.
        Size::new(0.0, 0.0)
    }

    fn on_event(owner: &HeadlessLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        install(
            &Field::new(&*owner.state, |state| &mut state.handlers.window_event),
            callback,
        );
    }
}

impl host::Displays<HeadlessLoop> for HeadlessDisplays {
    fn snapshot(owner: &HeadlessLoop) -> Result<DisplaySnapshot, PlatformError> {
        Ok(owner.with_state(|state| state.displays.snapshot()))
    }
}

impl host::Input<HeadlessLoop> for HeadlessInputDevices {
    fn keyboard_layout(owner: &HeadlessLoop) -> String {
        owner.with_state(|state| state.keyboard_layout.clone())
    }

    fn on_keyboard_layout_change(owner: &HeadlessLoop, callback: Box<dyn FnMut() + Send>) {
        install(
            &Field::new(&*owner.state, |state| {
                &mut state.handlers.keyboard_layout_changed
            }),
            callback,
        );
    }
}

impl host::Text for FakeTextInput {}

impl host::Clipboard<HeadlessLoop> for InMemoryClipboard {
    fn clipboard(owner: &HeadlessLoop) -> Arc<dyn Clipboard> {
        owner.with_state(|state| state.clipboard.clone())
    }
}

impl host::Drag<HeadlessLoop> for super::drag::DragDouble {
    fn data_transfer(owner: &HeadlessLoop) -> Arc<dyn DataTransferSource> {
        // The headless backend's one source: it serves the drags the test plays
        // through `HeadlessDragAndDrop`; it has no clipboard transport yet.
        owner.with_state(|state| Arc::clone(&state.drag) as Arc<dyn DataTransferSource>)
    }
}

impl host::Preferences<HeadlessLoop> for HeadlessPreferences {
    fn sample(owner: &HeadlessLoop) -> Result<flui_platform::SystemPreferences, PlatformError> {
        owner.with_state(|state| match state.preference_failure.take() {
            Some(message) => Err(PlatformError::Preferences { message }),
            None => Ok(state.preferences.clone()),
        })
    }
}

impl host::Lifecycle<HeadlessLoop> for HeadlessHostEvents {
    fn on_session_end(
        owner: &HeadlessLoop,
        callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>,
    ) {
        install(
            &Field::new(&*owner.state, |state| &mut state.handlers.session_end),
            callback,
        );
    }

    fn on_host_notice(owner: &HeadlessLoop, callback: Box<dyn FnMut(SystemNotice) + Send>) {
        let notices = owner.with_state(|state| Arc::clone(&state.notices));
        notices.set_hook(callback);
    }

    fn release_session_end(owner: &HeadlessLoop) {
        owner.with_state(|state| state.session_end_releases += 1);
    }

    fn on_reopen(_: &HeadlessLoop, callback: Box<dyn FnMut() + Send>) {
        // Nothing reopens a headless application.
        drop(callback);
    }

    fn activate(_: &HeadlessLoop, _: bool) {
        // Nothing is shown, so nothing comes to the front.
    }
}

/// Attaches and removes simulated input devices on a [`HeadlessPlatform`](crate::HeadlessPlatform)
/// ([`HeadlessPlatform::input_devices`](crate::HeadlessPlatform::input_devices)).
#[derive(Clone)]
pub struct HeadlessInputDevices {
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessInputDevices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessInputDevices")
            .finish_non_exhaustive()
    }
}

impl HeadlessInputDevices {
    /// Attach a device of `kind`: every open window receives
    /// [`DeviceAdded`](flui_platform::pointer::PointerEvent::DeviceAdded),
    /// then the capability change. The id its events carry; `None` once the
    /// platform is gone or device ids are exhausted.
    pub fn attach(
        &self,
        kind: flui_platform::pointer::PointerKind,
    ) -> Option<flui_platform::pointer::DeviceId> {
        use flui_platform::pointer::PointerEvent;
        let state = self.state.upgrade()?;
        let (change, windows, present) = {
            let mut state = state.lock();
            let label = state.device_labels.next().ok()?;
            let native = crate::shared::pointer_assembly::NativeDevice(label.get());
            let time =
                flui_platform::EventTime::from_nanos(crate::shared::events::event_timestamp_ns());
            let change = state.devices.add_device(native, kind, time)?;
            let windows: Vec<_> = state
                .windows
                .iter()
                .map(|w| Arc::clone(&w.callbacks))
                .collect();
            (change, windows, state.devices.present())
        };
        for callbacks in windows {
            callbacks.dispatch_input(PlatformInput::Pointer(PointerEvent::DeviceAdded(change)));
            callbacks.set_input_capabilities(callbacks.input_capabilities().with_present(present));
        }
        Some(change.device)
    }

    /// Remove the device `id` named: every open window receives
    /// [`DeviceRemoved`](flui_platform::pointer::PointerEvent::DeviceRemoved),
    /// then the capability change. A simulated device has no contacts of its
    /// own to cancel: a test injects its pointer input already assembled.
    /// `false` when it was not attached.
    pub fn remove(&self, id: flui_platform::pointer::DeviceId) -> bool {
        use flui_platform::pointer::PointerEvent;
        let Some(state) = self.state.upgrade() else {
            return false;
        };
        let (removed, windows, present) = {
            let mut state = state.lock();
            let time =
                flui_platform::EventTime::from_nanos(crate::shared::events::event_timestamp_ns());
            let Some(removed) = state
                .devices
                .native(id)
                .and_then(|native| state.devices.remove_device(native, time))
            else {
                return false;
            };
            let windows: Vec<_> = state
                .windows
                .iter()
                .map(|w| Arc::clone(&w.callbacks))
                .collect();
            (removed, windows, state.devices.present())
        };
        for callbacks in windows {
            callbacks.dispatch_input(PlatformInput::Pointer(PointerEvent::DeviceRemoved(removed)));
            callbacks.set_input_capabilities(callbacks.input_capabilities().with_present(present));
        }
        true
    }
}

/// Builds a fresh mock window under an already-locked `state`, threading it
/// through the same register/activate bookkeeping `Platform::open_window`'s
/// synchronous path uses — shared so the deferred-open resolution path
/// ([`HeadlessDeferredWindowOpens::resolve_next`](crate::platforms::headless::HeadlessDeferredWindowOpens::resolve_next)) stays byte-for-byte
/// consistent with the synchronous one instead of duplicating it. Each caller
/// announces the window (`WindowEvent::Created`) once it released the lock.
pub(crate) fn create_mock_window(
    state: &mut HeadlessState,
    platform_state: Weak<Mutex<HeadlessState>>,
    options: WindowOptions,
) -> Arc<dyn HostWindow> {
    let window_id = next_headless_window_id();
    let display = state.displays.default_window_display();
    let mut window = MockWindow::new(
        window_id,
        options,
        platform_state,
        MockWindowHost {
            text_store_host: state.text_store_host.clone(),
            display,
            display_bounds: display.and_then(|id| state.displays.logical_bounds(id)),
            capabilities: state.window_capabilities,
            limits: state.window_policy,
        },
    );

    window.drag = state.drag.timing().map(|timing| {
        Arc::new(super::drag::HeadlessDragSource::new(
            Arc::clone(&state.drag),
            timing,
        )) as Arc<dyn flui_platform::drag::PlatformDragSource>
    });
    // The initial snapshot reflects the devices already attached (ADR-0213 §3).
    window.callbacks.set_input_capabilities(
        state
            .input_availability
            .with_present(state.devices.present()),
    );
    if let Some(signal) = state.owner_signal.upgrade() {
        window
            .callbacks
            .set_input_wake(crate::owner::signal::OwnerSignal::callbacks_wake(
                &signal,
                &window.callbacks,
            ));
        let callbacks = Arc::downgrade(&window.callbacks);
        window.owner_redraw = Some(OwnerRedraw {
            signal: Arc::downgrade(&signal),
            wake: crate::owner::signal::OwnerSignal::input_wake(&signal, move || {
                if let Some(callbacks) = callbacks.upgrade() {
                    callbacks.dispatch_request_frame();
                }
            }),
        });
    }
    state.notices.mark_stale_on_wake(&window.window_state);

    state.windows.push(window.clone());
    state.active_window = Some(window_id);

    Arc::new(window) as Arc<dyn HostWindow>
}

/// Reports session ends and system notices to the headless platform's
/// registered hooks after `run` has consumed it — obtained from
/// [`HeadlessPlatform::host_events`](crate::HeadlessPlatform::host_events) before `run`.
///
/// Each call runs the hook on the calling thread through the call point,
/// outside the platform's state lock, so the hook may call back into the
/// platform; a hook registered while it runs replaces it. A hook that panics
/// keeps its place, and its panic reaches the caller.
#[derive(Clone)]
pub struct HeadlessHostEvents {
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessHostEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessHostEvents").finish_non_exhaustive()
    }
}

impl HeadlessHostEvents {
    /// One phase of the user's session ending, as the operating system reports
    /// it: the hook registered with `OwnerPlatform::on_session_end` is asked and
    /// its answer returned; [`SessionEndAnswer::Proceed`] when none is
    /// registered or the platform is gone. Like [`MockWindow::simulate_close`],
    /// it may be called from inside another callback, as a session end arriving
    /// in a nested native loop is.
    pub fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        let Some(state) = self.state.upgrade() else {
            return SessionEndAnswer::Proceed;
        };
        call(
            &Field::new(&*state, |state| &mut state.handlers.session_end),
            |hook| hook(phase),
        )
        .or_resume(SessionEndAnswer::Proceed)
    }

    /// A system notice other than a session end's phases: the hook registered
    /// with `OwnerPlatform::on_host_notice` is called. Nothing happens when
    /// none is registered or the platform is gone.
    ///
    /// A wake marks the state of every window opened on this platform stale
    /// first (`shared::system_notice`).
    pub fn notice(&self, notice: SystemNotice) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let notices = Arc::clone(&state.lock().notices);
        notices.dispatch(notice);
    }

    /// How many times the host told the platform it released a session end it
    /// had answered with a block (`Platform::release_session_end`).
    #[must_use]
    pub fn session_end_releases(&self) -> usize {
        self.state
            .upgrade()
            .map_or(0, |state| state.lock().session_end_releases)
    }
}

/// Changes the headless platform's displays after `run` has consumed it —
/// obtained from [`HeadlessPlatform::display_controls`](crate::HeadlessPlatform::display_controls) before `run`, the same
/// take-a-handle-first shape as [`HeadlessPlatform::owner_turns`](crate::HeadlessPlatform::owner_turns).
///
/// A change commits to the platform's snapshot and then wakes the owner, as a
/// native source does when the OS reports a mode switch, and so works from any
/// thread: the owner re-samples `Platform::display_snapshot` on its next turn.
#[derive(Clone)]
pub struct HeadlessDisplayControls {
    signal: Weak<crate::owner::signal::OwnerSignal>,
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessDisplayControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDisplayControls")
            .finish_non_exhaustive()
    }
}

impl HeadlessDisplayControls {
    /// The platform's current snapshot, `None` once the platform is gone.
    #[must_use]
    pub fn snapshot(&self) -> Option<DisplaySnapshot> {
        let state = self.state.upgrade()?;
        let snapshot = state.lock().displays.snapshot();
        Some(snapshot)
    }

    /// Apply `change` to display `id` and wake the owner. A platform that is
    /// gone applies nothing.
    ///
    /// # Errors
    /// [`DisplayChangeError::UnknownDisplay`] for an id this platform does not
    /// hold; [`DisplayChangeError::RevisionExhausted`] when the snapshot
    /// revision cannot advance.
    pub fn simulate_display_change(
        &self,
        id: DisplayId,
        change: DisplayChange,
    ) -> Result<(), DisplayChangeError> {
        let Some(state) = self.state.upgrade() else {
            return Ok(());
        };
        state.lock().displays.apply(id, change)?;
        // A failed wake leaves the signal's pending debt for the next turn.
        if let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
        Ok(())
    }
}

/// The headless platform's system preferences after `run` has consumed it:
/// obtained from [`HeadlessPlatform::preference_source`](crate::HeadlessPlatform::preference_source) before `run`.
///
/// A change commits to the platform's preferences and then wakes the owner, as
/// a native source does when the OS reports a setting change, so it works from
/// any thread: the host reads `Platform::preferences` on its next turn and
/// delivers what it accepts to every runtime.
#[derive(Clone)]
pub struct HeadlessPreferences {
    signal: Weak<crate::owner::signal::OwnerSignal>,
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessPreferences {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessPreferences")
            .finish_non_exhaustive()
    }
}

impl HeadlessPreferences {
    /// Report `values` as the system's preferences and wake the owner. A
    /// platform that is gone applies nothing.
    pub fn set(&self, values: flui_platform::SystemPreferences) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        state.lock().preferences = values;
        self.wake();
    }

    /// Make the next read fail with `message`, as a native getter that cannot
    /// complete does, and wake the owner. The host keeps the values it accepted
    /// last; the read after that reports the platform's preferences again.
    pub fn fail_next_read(&self, message: impl Into<String>) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        state.lock().preference_failure = Some(message.into());
        self.wake();
    }

    fn wake(&self) {
        // A failed wake leaves the signal's pending debt for the next turn.
        if let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
    }
}

// ==================== Mock Implementations ====================

/// The canonical test/headless window double.
///
/// Every window [`HeadlessPlatform`](crate::HeadlessPlatform) opens is a `MockWindow`: a fully
/// functional [`PlatformWindow`] that needs no
/// display server. Beyond the trait surface it offers what a test harness
/// needs to drive a window from the outside:
///
/// - a configurable scale factor ([`Self::simulate_scale_factor_change`],
///   delivered through the resize path exactly like the winit backend);
/// - `request_redraw` dispatching through the registered callbacks on the
///   owner thread, a request from another thread in the owner's next turn
///   ([`WindowCallbacks`]), like a real backend's frame wire;
/// - programmatic event injection ([`Self::inject_event`]) and lifecycle
///   simulation (`simulate_resize` / `simulate_focus` /
///   `simulate_visibility` / `simulate_close`), each firing the same
///   registered callback a real platform would.
///
/// There is no public constructor: obtain one by opening a window on a
/// [`HeadlessPlatform`](crate::HeadlessPlatform) and downcasting the returned
/// `Arc<dyn PlatformWindow>` via its `as_any()` to `&MockWindow`.
#[derive(Clone)]
pub struct MockWindow {
    id: WindowId,
    state: Arc<Mutex<MockWindowState>>,
    /// The published snapshot, shared by every clone of this window.
    window_state: Arc<WindowStateCell>,
    capabilities: WindowCapabilities,
    limits: HeadlessWindowPolicy,
    callbacks: Arc<WindowCallbacks>,
    text_input: Arc<FakeTextInput>,
    haptics: Arc<FakeHaptics>,
    system_chrome: Arc<FakeSystemChrome>,
    accessibility: Arc<FakeAccessibility>,
    /// Builds this window's text-store host, when the platform was made
    /// pull-model.
    text_store_host: Option<TextStoreHostFactory>,
    /// The input regions last handed to the simulated OS, and every change in order.
    input_regions: Arc<Mutex<RecordedInputRegions>>,
    /// Back-reference to the platform this window was opened on, so
    /// closing it can remove its own entry from `HeadlessState::windows`
    /// and — only once every tracked window is gone — consult the
    /// exit-policy hook exactly like the winit backend's `CloseRequested`
    /// handling does (see `notify_closed`). `Weak`: a window must never
    /// keep the platform state alive past the platform's own lifetime.
    platform_state: Weak<Mutex<HeadlessState>>,
    /// The OS drag session this window can start, when the platform was given one.
    drag: Option<Arc<dyn flui_platform::drag::PlatformDragSource>>,
    /// Carries a frame request made off the owner thread to the owner's next
    /// turn; `None` for a window opened with no owner signal.
    owner_redraw: Option<OwnerRedraw>,
}

/// How a [`MockWindow`] asks its owner for a frame from another thread: the
/// frame callback is owner code, so it runs in an owner turn, coalesced with
/// any request already queued, as a native backend posts its redraw.
#[derive(Clone)]
struct OwnerRedraw {
    signal: Weak<crate::owner::signal::OwnerSignal>,
    wake: crate::owner::handlers::InputWake,
}

impl std::fmt::Debug for MockWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("MockWindow")
            .field("id", &self.id)
            .field("title", &state.title)
            .field("position", &state.position)
            .field("size", &state.size)
            .field("mode", &state.mode)
            .field("scale_factor", &state.scale_factor)
            .field("focused", &state.focused)
            .field("visible", &state.visible)
            .field("callbacks", &self.callbacks)
            .finish_non_exhaustive()
    }
}

/// What a [`MockWindow`] handed its simulated OS as input regions.
#[derive(Debug, Default)]
struct RecordedInputRegions {
    projection: ProjectedInputRegions,
    updates: Vec<Vec<DeviceInputRegion>>,
}
/// What the platform hands a window it opens.
struct MockWindowHost {
    text_store_host: Option<TextStoreHostFactory>,
    /// The display the window opens on; `None` when the platform has none.
    display: Option<DisplayId>,
    /// That display's bounds in logical pixels, for a window that opens
    /// maximized or fullscreen.
    display_bounds: Option<Bounds<f64>>,
    capabilities: WindowCapabilities,
    limits: HeadlessWindowPolicy,
}

/// Mutable state for headless MockWindow
struct MockWindowState {
    /// How many times [`PlatformWindow::pre_present_notify`] was called —
    /// the oracle for "the frame pump arms the compositor frame callback
    /// once per presented frame, before the present".
    pre_present_notifies: u64,
    /// Every [`PlatformWindow::set_frame_rate_hint`] the window received, in order.
    frame_rate_hints: Vec<flui_platform::FrameRateHint>,
    title: String,
    /// The outer top-left corner. This host draws no frame, so the outer
    /// rectangle and the content area are the same.
    position: Point<f64>,
    size: Size<f64>,
    mode: WindowMode,
    /// The rectangle the window returns to from a mode other than normal.
    normal_geometry: Option<WindowGeometry>,
    constraints: SizeConstraints,
    resizable: bool,
    decorated: bool,
    scale_factor: f64,
    focused: bool,
    visible: bool,
    execution: flui_platform::WindowExecutionState,
    safe_area: flui_foundation::geometry::EdgeInsets,
    closed: bool,
    hovered: bool,
    modifiers: flui_platform::keyboard::Modifiers,
    appearance: WindowAppearance,
    cursor: CursorIcon,
    /// The display this window is on; `None` when the platform has no displays.
    display: Option<DisplayId>,
}

impl MockWindowState {
    fn geometry(&self) -> WindowGeometry {
        WindowGeometry::new(Some(self.position), self.size)
    }

    /// `size` as the host ends up with it: raised to the window's minimum,
    /// lowered to its maximum and to the largest window the host allows.
    fn clamp_size(&self, size: Size<f64>, limits: &HeadlessWindowPolicy) -> Size<f64> {
        let mut size = size;
        if let Some(min) = self.constraints.min {
            size.width = size.width.max(min.width);
            size.height = size.height.max(min.height);
        }
        if let Some(max) = self.constraints.max {
            size.width = size.width.min(max.width);
            size.height = size.height.min(max.height);
        }
        Self::clamp_to_host(size, limits)
    }

    fn clamp_to_host(size: Size<f64>, limits: &HeadlessWindowPolicy) -> Size<f64> {
        match limits.max_window_size {
            Some(max) => Size::new(size.width.min(max.width), size.height.min(max.height)),
            None => size,
        }
    }

    fn remember_normal(&mut self) {
        if self.mode == WindowMode::Normal {
            self.normal_geometry = Some(self.geometry());
        }
    }

    /// Enter `mode` the way the host does: maximized and fullscreen fill the
    /// window's display (as far as the host allows), minimized keeps the
    /// bounds, and normal returns to the remembered rectangle. A host that
    /// declines maximizing leaves the window as it was.
    fn enter_mode(
        &mut self,
        mode: WindowMode,
        limits: &HeadlessWindowPolicy,
        display_bounds: Option<Bounds<f64>>,
    ) {
        match mode {
            WindowMode::Normal => {
                if let Some(normal) = self.normal_geometry.take() {
                    if let Some(position) = normal.position {
                        self.position = position;
                    }
                    self.size = normal.size;
                }
                self.mode = WindowMode::Normal;
            }
            WindowMode::Minimized => {
                self.remember_normal();
                self.mode = WindowMode::Minimized;
                self.focused = false;
            }
            WindowMode::Maximized if limits.refuses_maximize => {}
            WindowMode::Maximized | WindowMode::Fullscreen => {
                self.remember_normal();
                if let Some(bounds) = display_bounds {
                    self.position = bounds.origin;
                    self.size = Self::clamp_to_host(bounds.size, limits);
                }
                self.mode = mode;
            }
            // `WindowMode` grows; a mode this host does not know changes nothing.
            _ => {}
        }
    }
}

/// The snapshot the model describes, as this window's capabilities let the
/// host observe it: no position without `observes_position`, no minimized
/// mode without `observes_minimized`, and no frame while fullscreen.
fn sample_window_state(model: &MockWindowState, capabilities: &WindowCapabilities) -> WindowState {
    let observed = |geometry: WindowGeometry| {
        WindowGeometry::new(
            geometry.position.filter(|_| capabilities.observes_position),
            geometry.size,
        )
    };
    let mode = match model.mode {
        WindowMode::Minimized if !capabilities.observes_minimized => WindowMode::Normal,
        mode => mode,
    };
    WindowState::new(observed(model.geometry()), model.size)
        .with_mode(mode)
        .with_normal_geometry(model.normal_geometry.map(observed))
        .with_visible(model.visible && !model.closed)
        .with_focused(model.focused)
        .with_title(Some(model.title.clone()))
        .with_constraints(model.constraints)
        .with_resizable(model.resizable)
        .with_decorated(model.decorated && model.mode != WindowMode::Fullscreen)
        .with_display(model.display)
}

impl MockWindow {
    fn new(
        id: WindowId,
        options: WindowOptions,
        platform_state: Weak<Mutex<HeadlessState>>,
        host: MockWindowHost,
    ) -> Self {
        let MockWindowHost {
            text_store_host,
            display,
            display_bounds,
            capabilities,
            limits,
        } = host;
        let mut model = MockWindowState {
            pre_present_notifies: 0,
            frame_rate_hints: Vec::new(),
            title: options.title.clone(),
            position: options
                .position
                .filter(|_| capabilities.places_at_open)
                .unwrap_or_default(),
            size: options.size,
            mode: WindowMode::Normal,
            normal_geometry: None,
            constraints: SizeConstraints::new(options.min_size, options.max_size),
            resizable: options.resizable,
            decorated: options.decorated,
            scale_factor: 1.0,
            focused: true,
            execution: flui_platform::WindowExecutionState::Running,
            safe_area: flui_foundation::geometry::EdgeInsets::ZERO,
            visible: options.visible,
            closed: false,
            hovered: false,
            modifiers: flui_platform::keyboard::Modifiers::NONE,
            appearance: WindowAppearance::default(),
            cursor: CursorIcon::default(),
            display,
        };
        model.size = model.clamp_size(options.size, &limits);
        if capabilities.permits(&WindowCommand::SetMode(options.mode)) {
            model.enter_mode(options.mode, &limits, display_bounds);
        }
        // A window that opens maximized or fullscreen returns to the rectangle
        // it had before, which only its opener knows.
        if model.mode != WindowMode::Normal && options.normal_geometry.is_some() {
            model.normal_geometry = options.normal_geometry;
        }
        let initial = sample_window_state(&model, &capabilities);
        Self {
            id,
            state: Arc::new(Mutex::new(model)),
            window_state: Arc::new(WindowStateCell::new(initial)),
            capabilities,
            limits,
            callbacks: Arc::new(WindowCallbacks::new()),
            text_input: Arc::new(FakeTextInput::new()),
            haptics: Arc::new(FakeHaptics::new()),
            system_chrome: Arc::new(FakeSystemChrome::default()),
            accessibility: Arc::new(FakeAccessibility::new()),
            text_store_host,
            input_regions: Arc::default(),
            platform_state,
            drag: None,
            owner_redraw: None,
        }
    }

    /// The window's display bounds in logical pixels, from the platform's
    /// current displays; `None` when the platform is gone or knows none.
    fn display_bounds(&self) -> Option<Bounds<f64>> {
        let display = self.state.lock().display?;
        self.platform_state
            .upgrade()?
            .lock()
            .displays
            .logical_bounds(display)
    }

    /// Publish what the model says now: read it into a snapshot, and raise
    /// `on_state_changed` after the cell's lock is released when the revision
    /// grew.
    fn publish_state(&self) -> WindowState {
        self.window_state.invalidate();
        let published = self.window_state.state_with(|| {
            let model = self.state.lock();
            sample_window_state(&model, &self.capabilities)
        });
        if published.changed {
            self.callbacks.dispatch_state_changed();
        }
        published.state
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
    /// Reached from both routes — [`flui_platform::PlatformWindow::close`] and
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

    /// Removes this window from the platform's own tracking, announces
    /// `Closed`, and, only once every window this platform knows about is
    /// gone, consults the exit-policy hook (see
    /// [`crate::owner::handlers::PlatformHandlers::exit_policy`]) exactly as
    /// the winit backend's `CloseRequested` handling does. A caller that never
    /// installed a hook sees no behavior change: this is a no-op unless
    /// [`crate::Platform::set_exit_policy_hook`] was called first, matching
    /// every other headless test/consumer that predates this mechanism.
    ///
    /// Every application handler (the window-event handler, the hook, and
    /// `on_quit`) runs through the call point with the state lock released
    /// (ADR-0039): the hook's body (`flui-app`'s `AppRuntime::should_exit`)
    /// drops removed UI runtime state, whose destructors may call back into
    /// this platform (a dispose hook opening another window, say).
    fn notify_closed(&self) {
        let Some(platform_state) = self.platform_state.upgrade() else {
            return;
        };
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
        emit_window_event(&platform_state, WindowEvent::Closed(self.id));

        // Emptiness is read after the event, not carried down from the lock
        // above: the handler that ran in between is exactly the code that
        // opens windows (a close handler showing the next one), and the
        // policy re-checks it again before quitting.
        consult_exit_policy(&platform_state);
    }

    /// How many times the frame pump has called
    /// [`PlatformWindow::pre_present_notify`] on this window — one per
    /// presented frame, before the present, is the contract.
    #[must_use]
    pub fn pre_present_notifies(&self) -> u64 {
        self.state.lock().pre_present_notifies
    }

    /// Every input-region set this window handed its simulated OS, in device pixels and
    /// delivery order: one entry per change of the projection, scale or display.
    #[must_use]
    pub fn input_region_updates(&self) -> Vec<Vec<DeviceInputRegion>> {
        self.input_regions.lock().updates.clone()
    }
    /// Every frame-rate hint the window was given, oldest first.
    #[must_use]
    pub fn frame_rate_hints(&self) -> Vec<flui_platform::FrameRateHint> {
        self.state.lock().frame_rate_hints.clone()
    }

    /// Inject a platform input event for testing.
    /// Fires the registered `on_input` callback.
    pub fn inject_event(&self, event: PlatformInput) -> DispatchEventResult {
        self.callbacks.dispatch_input(event)
    }

    /// Simulate a resize for testing.
    /// Fires `on_state_changed` when the snapshot differs, then the
    /// registered `on_resize` callback.
    pub fn simulate_resize(&self, width: f64, height: f64) {
        let size = Size::new(width, height);
        let scale = {
            let mut state = self.state.lock();
            state.size = size;
            state.scale_factor
        };
        self.publish_state();
        self.callbacks.dispatch_logical_resize(size, scale);
    }

    /// Simulate the user (or the OS) moving the window to `position`, outer
    /// top-left in global logical pixels. This host draws no frame, so that is
    /// also where its content starts: [`PlatformWindow::window_at`] answers
    /// from it. Fires `on_state_changed` when the snapshot differs.
    pub fn simulate_move(&self, position: Point<f64>) {
        self.state.lock().position = position;
        self.publish_state();
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
            state.size
        };
        self.callbacks.dispatch_logical_resize(size, scale_factor);
    }

    /// Simulate focus change for testing.
    /// Fires the registered `on_active_status_change` callback.
    pub fn simulate_focus(&self, focused: bool) {
        self.state.lock().focused = focused;
        self.publish_state();
        self.callbacks.dispatch_active_status_change(focused);
    }

    /// Simulate an owner-thread safe-area report; the callback fires without
    /// holding window state. Closed and detached windows ignore the report.
    pub fn simulate_safe_area(&self, insets: flui_foundation::geometry::EdgeInsets) {
        {
            let mut state = self.state.lock();
            if state.closed
                || state.execution == flui_platform::WindowExecutionState::Detached
                || state.safe_area == insets
            {
                return;
            }
            state.safe_area = insets;
        }
        self.callbacks.dispatch_safe_area_change(insets);
    }

    /// Simulate reversible native execution eligibility; terminal close stays terminal.
    pub fn simulate_execution_state(&self, execution: flui_platform::WindowExecutionState) {
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
        self.publish_state();
        self.callbacks.dispatch_visibility_status_change(visible);
    }

    /// Simulate the system light/dark appearance this window observes.
    /// Fires `on_appearance_changed` only when the value changes, as the
    /// native backends do; closed windows ignore the report.
    pub fn simulate_appearance(&self, appearance: WindowAppearance) {
        {
            let mut state = self.state.lock();
            if state.closed || state.appearance == appearance {
                return;
            }
            state.appearance = appearance;
        }
        self.callbacks.dispatch_appearance_changed();
    }

    /// Move this window to display `id`, as the user dragging it onto another
    /// monitor does. Fires `on_display_changed` only when the display really
    /// changes; a closed window ignores the report.
    ///
    /// # Errors
    /// [`DisplayChangeError::UnknownDisplay`] when the platform holds no
    /// display with this id (or is gone); the window stays where it was.
    pub fn simulate_move_to(&self, id: DisplayId) -> Result<(), DisplayChangeError> {
        let known = self
            .platform_state
            .upgrade()
            .is_some_and(|platform| platform.lock().displays.contains(id));
        if !known {
            return Err(DisplayChangeError::UnknownDisplay);
        }
        {
            let mut state = self.state.lock();
            if state.closed || state.display == Some(id) {
                return Ok(());
            }
            state.display = Some(id);
        }
        self.publish_state();
        self.callbacks.dispatch_display_changed();
        Ok(())
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
            if let Some(platform_state) = self.platform_state.upgrade() {
                emit_window_event(
                    &platform_state,
                    WindowEvent::CloseRequested { window_id: self.id },
                );
            }
            self.complete_close();
        }
        should
    }
}

impl PlatformWindow for MockWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<i32> {
        let state = self.state.lock();
        flui_foundation::geometry::DevicePixelRatio::new(state.scale_factor)
            .map_or(Size::new(0, 0), |ratio| {
                flui_foundation::geometry::device_size(state.size, ratio)
            })
    }

    fn logical_size(&self) -> Size<f64> {
        self.state.lock().size
    }

    fn scale_factor(&self) -> f64 {
        self.state.lock().scale_factor
    }

    fn request_redraw(&self) {
        // Any thread may ask, but the frame callback is owner code: a request
        // from another thread waits for the owner's next turn.
        if let Some(redraw) = &self.owner_redraw
            && let Some(signal) = redraw.signal.upgrade()
            && signal.owner() != thread::current().id()
        {
            (redraw.wake)();
            return;
        }
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

    fn execution_state(&self) -> flui_platform::WindowExecutionState {
        let state = self.state.lock();
        if state.closed {
            flui_platform::WindowExecutionState::Detached
        } else {
            state.execution
        }
    }

    fn is_visible(&self) -> bool {
        self.state.lock().visible
    }

    // ==================== State and commands (ADR-0237) ====================

    fn state(&self) -> WindowState {
        self.publish_state()
    }

    fn capabilities(&self) -> WindowCapabilities {
        self.capabilities
    }

    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        if self.state.lock().closed {
            return Err(WindowCommandError::Closed);
        }
        command.admit(&self.capabilities)?;
        let display_bounds = match command {
            WindowCommand::SetMode(WindowMode::Maximized | WindowMode::Fullscreen) => {
                self.display_bounds()
            }
            _ => None,
        };
        let resized = {
            let mut state = self.state.lock();
            let before = state.size;
            match command {
                WindowCommand::SetTitle(title) => state.title = title,
                WindowCommand::SetPosition(at) => {
                    if state.mode == WindowMode::Normal {
                        state.position = at;
                    } else if let Some(normal) = state.normal_geometry.as_mut() {
                        normal.position = Some(at);
                    }
                }
                WindowCommand::SetSize(size) => {
                    let size = state.clamp_size(size, &self.limits);
                    if state.mode == WindowMode::Normal {
                        state.size = size;
                    } else if let Some(normal) = state.normal_geometry.as_mut() {
                        normal.size = size;
                    }
                }
                WindowCommand::SetConstraints(constraints) => {
                    state.constraints = constraints;
                    if state.mode == WindowMode::Normal {
                        state.size = state.clamp_size(state.size, &self.limits);
                    }
                }
                WindowCommand::SetResizable(resizable) => state.resizable = resizable,
                WindowCommand::SetDecorated(decorated) => state.decorated = decorated,
                WindowCommand::SetMode(mode) => {
                    state.enter_mode(mode, &self.limits, display_bounds);
                }
                WindowCommand::Show => {
                    if state.mode == WindowMode::Minimized {
                        state.enter_mode(WindowMode::Normal, &self.limits, None);
                    }
                    state.visible = true;
                }
                WindowCommand::Hide => state.visible = false,
                WindowCommand::Activate => state.focused = true,
                // A command this host does not know is refused by `admit`
                // through its capability, so none reaches here.
                _ => {}
            }
            (state.size != before).then_some((state.size, state.scale_factor))
        };
        self.publish_state();
        if let Some((size, scale)) = resized {
            self.callbacks.dispatch_logical_resize(size, scale);
        }
        Ok(())
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

    fn modifiers(&self) -> flui_platform::keyboard::Modifiers {
        self.state.lock().modifiers
    }

    fn appearance(&self) -> WindowAppearance {
        self.state.lock().appearance
    }

    fn display_id(&self) -> Option<DisplayId> {
        self.state.lock().display
    }

    fn set_frame_rate_hint(&self, hint: flui_platform::FrameRateHint) {
        self.state.lock().frame_rate_hints.push(hint);
    }

    fn set_input_regions(&self, regions: &[InputRegion]) {
        let (scale, display) = {
            let state = self.state.lock();
            (state.scale_factor, state.display)
        };
        let mut recorded = self.input_regions.lock();
        let RecordedInputRegions {
            projection,
            updates,
        } = &mut *recorded;
        if let Some(sent) = projection.update(regions, scale, display) {
            updates.push(sent.to_vec());
        }
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        Some(Arc::clone(&self.text_input) as Arc<dyn PlatformTextInput>)
    }

    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        Some(Arc::clone(&self.haptics) as Arc<dyn PlatformHaptics>)
    }

    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        self.drag.clone()
    }

    fn window_at(&self, position: Point<f64>) -> flui_platform::WindowAt {
        use flui_platform::WindowAt;
        let Some(platform) = self.platform_state.upgrade() else {
            return WindowAt::Unknown;
        };
        // The platform's lock is released before any window's is taken.
        let windows = {
            let state = platform.lock();
            if !state.placement {
                return WindowAt::Unknown;
            }
            state.windows.clone()
        };
        // No frame: a window's outer top-left is where its content starts.
        let origin = self.state.lock().position;
        let desktop = Point::new(origin.x + position.x, origin.y + position.y);
        if !(desktop.x.is_finite() && desktop.y.is_finite()) {
            return WindowAt::Outside;
        }
        windows
            .iter()
            .rev()
            .find_map(|window| {
                let state = window.state.lock();
                let (origin, size) = (state.position, state.size);
                let local = Point::new(desktop.x - origin.x, desktop.y - origin.y);
                let inside =
                    (0.0..size.width).contains(&local.x) && (0.0..size.height).contains(&local.y);
                let shown = state.visible && !state.closed && state.mode != WindowMode::Minimized;
                (shown && inside).then_some(WindowAt::Window {
                    window: window.id,
                    position: local,
                })
            })
            .unwrap_or(WindowAt::Outside)
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.system_chrome) as Arc<dyn flui_platform::PlatformSystemChrome>)
    }

    // ==================== Control Methods ====================

    fn show(&self) -> Result<(), flui_platform::WindowShowError> {
        {
            let mut state = self.state.lock();
            if state.closed {
                return Err(flui_platform::WindowShowError::Closed);
            }
            if state.mode == WindowMode::Minimized {
                state.enter_mode(WindowMode::Normal, &self.limits, None);
            }
            state.visible = true;
            state.focused = true;
        }
        self.publish_state();
        Ok(())
    }

    fn activate(&self) {
        self.state.lock().focused = true;
        self.publish_state();
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

    crate::owner::handlers::impl_window_callback_setters!(callbacks);

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::host_window::CoreHostWindow for MockWindow {}

impl HostWindow for MockWindow {
    fn accessibility(&self) -> Option<Arc<dyn flui_semantics::platform::PlatformAccessibility>> {
        Some(Arc::clone(&self.accessibility)
            as Arc<dyn flui_semantics::platform::PlatformAccessibility>)
    }

    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<Rc<dyn TextStoreHost>> {
        self.text_store_host.as_ref().map(|factory| factory())
    }
}

/// Recording fake for [`PlatformAccessibility`](flui_semantics::platform::PlatformAccessibility),
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
    activation_listener: Option<flui_semantics::platform::AccessibilityActivationListener>,
    action_listener: Option<flui_semantics::platform::AccessibilityActionListener>,
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

impl flui_semantics::platform::PlatformAccessibility for FakeAccessibility {
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

    fn set_activation_listener(
        &self,
        listener: flui_semantics::platform::AccessibilityActivationListener,
    ) {
        self.state.lock().activation_listener = Some(listener);
    }

    fn set_action_listener(&self, listener: flui_semantics::platform::AccessibilityActionListener) {
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

/// Records the overlay styles delivered through
/// [`PlatformWindow::system_chrome`], in delivery order.
#[derive(Default)]
pub struct FakeSystemChrome {
    styles: Mutex<Vec<flui_platform::SystemUiOverlayStyle>>,
}

impl FakeSystemChrome {
    /// Every delivered overlay style, in delivery order.
    #[must_use]
    pub fn styles(&self) -> Vec<flui_platform::SystemUiOverlayStyle> {
        self.styles.lock().clone()
    }
}

impl std::fmt::Debug for FakeSystemChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeSystemChrome")
            .field("styles", &*self.styles.lock())
            .finish()
    }
}

impl flui_platform::PlatformSystemChrome for FakeSystemChrome {
    fn set_overlay_style(&self, style: flui_platform::SystemUiOverlayStyle) {
        self.styles.lock().push(style);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
