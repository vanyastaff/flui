//! The headless window: [`MockWindow`], the window double every
//! [`HeadlessPlatform`](crate::HeadlessPlatform) opens, its system chrome
//! ([`FakeSystemChrome`]), what the headless "operating system" refuses or
//! clamps ([`HeadlessWindowPolicy`]), so a test can see a window snapshot
//! report the outcome rather than the request, and [`HeadlessWindows`], the
//! windows the platform tracks.

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
use flui_platform::input_regions::{DeviceInputRegion, InputRegion};
use flui_platform::text_store::TextStoreHost;
use flui_platform::{
    AdmittedCommand, ApplyError, CursorError, DispatchEventResult, DisplayId, PlatformHaptics,
    PlatformInput, PlatformTextInput, PlatformWindow, SizeConstraints, SurfaceChange, Unsupported,
    UnsupportedReason, WindowAppearance, WindowCapabilities, WindowCommand, WindowGeometry,
    WindowHandler, WindowId, WindowMaterial, WindowMode, WindowOptions, WindowState,
};
use parking_lot::Mutex;

use crate::host::{self, Host, WindowFacets, WindowPositions};
use crate::host_window::HostWindow;
use crate::input::regions::ProjectedInputRegions;
use crate::owner::OpenWindowError;

use crate::owner::handlers::OpenWindows;
use crate::owner::headless::TextStoreHostFactory;
use crate::owner::headless::{HeadlessLoop, HeadlessState, TrackedWindows, consult_exit_policy};
use crate::owner::window_delivery::{Publication, WindowDelivery};
use crate::platform::HostPlatform;
use crate::text::WindowTextInput;
use crate::window::state::{ReportedFacts, WindowStateCell};

/// What the headless host imposes on its windows.
///
/// A window command is a request (ADR-0237): the host may clamp a size or
/// decline a mode, and the window's snapshot shows what happened. The default
/// limits impose nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct HeadlessWindowPolicy {
    /// The largest window the host allows, in logical pixels. A larger
    /// resize request ends at this size.
    pub max_window_size: Option<Size<f64>>,
    /// Whether the host declines to maximize a window: the maximize request
    /// is accepted and the window stays as it was.
    pub refuses_maximize: bool,
}

impl HeadlessWindowPolicy {
    /// Cap windows at `size`.
    #[must_use]
    pub const fn with_max_window_size(mut self, size: Size<f64>) -> Self {
        self.max_window_size = Some(size);
        self
    }

    /// Make the host decline to maximize windows.
    #[must_use]
    pub const fn refusing_maximize(mut self) -> Self {
        self.refuses_maximize = true;
        self
    }
}

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

/// The windows the headless platform tracks, and what the simulated OS gives
/// every window opened from now on.
///
/// The host builds it and hands it to the loop, which holds it as
/// [`TrackedWindows`]; each window keeps it to leave it when it closes.
/// Public where it is defined, as the host's table names it, and nameable only
/// inside the crate.
pub struct HeadlessWindows {
    tracked: Mutex<Tracked>,
    /// The windows deferred opens built, kept on the owner thread until they
    /// close, as a native host keeps the windows it opened: what the opener
    /// gets is their identity, and a weak reference on the owner.
    deferred: crate::owner::slot::OwnerMap<WindowId, Rc<dyn HostWindow>>,
    /// What every window hands out of the window-level positions.
    positions: WindowPositions<()>,
}

impl std::fmt::Debug for HeadlessWindows {
    // `try_lock`, as `parking_lot::Mutex<T>: Debug` does: formatting while the
    // same thread holds the set would otherwise deadlock.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("HeadlessWindows");
        match self.tracked.try_lock() {
            Some(tracked) => out.field("windows", &tracked.windows.len()),
            None => out.field("windows", &format_args!("<locked>")),
        };
        out.finish_non_exhaustive()
    }
}

struct Tracked {
    /// The open windows, in the order they opened: the last is topmost.
    windows: Vec<MockWindow>,
    active: Option<WindowId>,
    /// Whether the windows sit on one desktop and answer
    /// [`PlatformWindow::window_at`] ([`HeadlessPlatform::with_window_placement`](crate::HeadlessPlatform::with_window_placement)).
    placement: bool,
    /// What every window opened from now on honours
    /// ([`HeadlessPlatform::with_window_capabilities`](crate::HeadlessPlatform::with_window_capabilities)).
    capabilities: WindowCapabilities,
    /// What the host clamps or declines for every window opened from now on
    /// ([`HeadlessPlatform::with_window_policy`](crate::HeadlessPlatform::with_window_policy)).
    policy: HeadlessWindowPolicy,
}

impl HeadlessWindows {
    /// No window yet; each one opened hands out `positions`.
    pub(crate) fn new(positions: WindowPositions<()>) -> Self {
        Self {
            deferred: crate::owner::slot::OwnerMap::new(),
            tracked: Mutex::new(Tracked {
                windows: Vec::new(),
                active: None,
                placement: false,
                capabilities: WindowCapabilities::ALL,
                policy: HeadlessWindowPolicy::default(),
            }),
            positions,
        }
    }

    /// Stops tracking window `id`; another becomes active if it was.
    fn forget(&self, id: WindowId) {
        {
            let mut tracked = self.tracked.lock();
            tracked.windows.retain(|window| window.id != id);
            if tracked.active == Some(id) {
                tracked.active = tracked.windows.first().map(|window| window.id);
            }
        }
        // Let go of a deferred open's window outside the lock.
        drop(self.deferred.remove(&id));
    }
}

impl OpenWindows for HeadlessWindows {
    fn is_empty(&self) -> bool {
        self.tracked.lock().windows.is_empty()
    }
}

impl TrackedWindows for HeadlessWindows {
    /// Builds a fresh mock window under an already-locked `state`, threading
    /// it through the same register/activate bookkeeping for the synchronous
    /// `Platform::open_window` and the deferred-open resolution
    /// ([`HeadlessDeferredWindowOpens::resolve_next`](crate::HeadlessDeferredWindowOpens::resolve_next)),
    /// so the two stay byte-for-byte consistent.
    fn open(
        self: Arc<Self>,
        state: &mut HeadlessState,
        platform: Weak<Mutex<HeadlessState>>,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Rc<dyn HostWindow> {
        let window_id = next_headless_window_id();
        let display = state.window_display();
        let (capabilities, limits) = {
            let tracked = self.tracked.lock();
            (tracked.capabilities, tracked.policy)
        };
        // The capability snapshot reflects the devices already attached
        // (ADR-0213 §3).
        let input = state
            .input_availability
            .with_present(state.devices.present());
        let mut window = MockWindow::new(
            window_id,
            options,
            platform,
            Arc::downgrade(&self),
            MockWindowHost {
                positions: self.positions,
                text_store_host: state.text_store_host.clone(),
                display,
                display_bounds: display.and_then(|id| state.display_bounds(id)),
                capabilities,
                limits,
            },
            WindowDelivery::new(handler, input),
        );

        window.drag = state.window_drag_source();
        if let Some(signal) = state.owner_signal.upgrade() {
            window.delivery.resume_through_signal(&signal);
            let delivery = Arc::downgrade(&window.delivery);
            window.redraw.owner = Some(OwnerRedraw {
                signal: Arc::downgrade(&signal),
                wake: crate::owner::signal::OwnerSignal::window_wake(&signal, move || {
                    if let Some(delivery) = delivery.upgrade() {
                        delivery.dispatch_request_frame();
                    }
                }),
            });
        }
        state.notices.mark_stale_on_wake(&window.window_state);

        {
            let mut tracked = self.tracked.lock();
            tracked.windows.push(window.clone());
            tracked.active = Some(window_id);
        }

        Rc::new(window) as Rc<dyn HostWindow>
    }

    /// The deferred open's window stays with the host until it closes.
    fn keep_deferred(&self, window: &Rc<dyn HostWindow>) {
        if self
            .deferred
            .insert(window.id(), Rc::clone(window))
            .is_err()
        {
            tracing::error!("a deferred window was resolved off the owner thread");
        }
    }

    fn deliveries_of_all(&self) -> Vec<Arc<WindowDelivery>> {
        self.tracked
            .lock()
            .windows
            .iter()
            .map(|window| Arc::clone(&window.delivery))
            .collect()
    }

    fn delivery_of(&self, id: WindowId) -> Option<Arc<WindowDelivery>> {
        self.tracked
            .lock()
            .windows
            .iter()
            .find(|window| window.id == id)
            .map(|window| Arc::clone(&window.delivery))
    }
}

/// The headless windows' test controls: builders taken before
/// [`Platform::run`](crate::Platform::run) consumes the platform.
impl<H: Host<Loop = HeadlessLoop, Window = MockWindow>> HostPlatform<H> {
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
        self.window_shared().tracked.lock().placement = true;
        self
    }

    /// Make every window this platform opens from now on honour exactly
    /// `capabilities`, to test a caller against a host that cannot do
    /// everything. The default is [`WindowCapabilities::ALL`]. A command whose
    /// capability is `false` is refused as unsupported and changes nothing.
    #[must_use]
    pub fn with_window_capabilities(self, capabilities: WindowCapabilities) -> Self {
        self.window_shared().tracked.lock().capabilities = capabilities;
        self
    }

    /// Make the host clamp or decline what windows opened from now on ask
    /// for, to test that a snapshot reports the outcome and not the request.
    /// The default imposes nothing.
    #[must_use]
    pub fn with_window_policy(self, limits: HeadlessWindowPolicy) -> Self {
        self.window_shared().tracked.lock().policy = limits;
        self
    }

    /// The windows this platform holds open, through a handle that outlives
    /// [`Platform::run`](crate::Platform::run): a test reaches a window the
    /// application opened, to close it as the user would, without a way into
    /// the application's handler.
    #[must_use]
    pub fn open_windows(&self) -> HeadlessOpenWindows {
        HeadlessOpenWindows {
            windows: Arc::downgrade(self.window_shared()),
        }
    }
}

/// The windows a headless platform holds open
/// ([`HeadlessPlatform::open_windows`](crate::HeadlessPlatform::open_windows)).
/// Empty once the platform is gone.
#[derive(Clone, Debug)]
pub struct HeadlessOpenWindows {
    windows: Weak<HeadlessWindows>,
}

impl HeadlessOpenWindows {
    /// Every open window, in the order it opened: the last is topmost.
    #[must_use]
    pub fn all(&self) -> Vec<MockWindow> {
        self.windows
            .upgrade()
            .map(|windows| windows.tracked.lock().windows.clone())
            .unwrap_or_default()
    }

    /// The open window `id`, `None` once it closed.
    #[must_use]
    pub fn get(&self, id: WindowId) -> Option<MockWindow> {
        let windows = self.windows.upgrade()?;
        let tracked = windows.tracked.lock();
        tracked
            .windows
            .iter()
            .find(|window| window.id == id)
            .cloned()
    }
}

impl host::Window<HeadlessLoop> for MockWindow {
    type Shared = HeadlessWindows;
    /// A headless window is made of nothing the positions need.
    type Ref = ();

    fn open(
        owner: &HeadlessLoop,
        windows: &Arc<HeadlessWindows>,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        tracing::info!(?options, "Creating mock window");

        let platform_state = Arc::downgrade(&owner.state);
        let window = owner.with_state(|state| {
            TrackedWindows::open(Arc::clone(windows), state, platform_state, options, handler)
        });
        Ok(window)
    }

    fn active(_: &HeadlessLoop, windows: &HeadlessWindows) -> Option<WindowId> {
        windows.tracked.lock().active
    }

    fn capabilities(_: &HeadlessLoop, windows: &HeadlessWindows) -> WindowCapabilities {
        windows.tracked.lock().capabilities
    }

    fn frame_inset(_: &HeadlessLoop, _: &HeadlessWindows, _: f64) -> Size<f64> {
        // A headless window draws no frame.
        Size::new(0.0, 0.0)
    }
}

// ==================== Mock Implementations ====================

/// A simulated move of a window named a display its platform does not hold
/// ([`MockWindow::simulate_move_to`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the platform holds no display {display:?}")]
#[non_exhaustive]
pub struct UnknownDisplay {
    /// The display the move named.
    pub display: DisplayId,
}

/// What a headless window's simulated window system holds
/// ([`MockWindow::window_system`]).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct HeadlessWindowSystem {
    /// The drawable area and scale; `None` for a size or scale that is not a
    /// drawable area.
    pub metrics: Option<flui_foundation::geometry::WindowMetrics>,
    /// Whether the window has input focus. The headless desktop has no
    /// activation apart from focus, so this is its activation too.
    pub focused: bool,
    /// Whether the window is shown and open.
    pub visible: bool,
    /// Whether the simulated system says none of the window reaches the
    /// screen ([`MockWindow::simulate_occlusion`]).
    pub occluded: bool,
    /// Whether the simulated system honours the window's input regions, as
    /// the platform's input availability says
    /// ([`HeadlessPlatform::with_input_availability`](crate::HeadlessPlatform::with_input_availability)).
    pub input_regions: flui_platform::Availability,
    /// The material the simulated system draws behind the window: the last
    /// [`WindowCommand::SetMaterial`] it applied, `Opaque` before any.
    pub material: WindowMaterial,
}

/// The canonical test/headless window double.
///
/// Every window [`HeadlessPlatform`](crate::HeadlessPlatform) opens is a `MockWindow`: a fully
/// functional [`PlatformWindow`] that needs no
/// display server. Beyond the trait surface it offers what a test harness
/// needs to drive a window from the outside:
///
/// - a configurable scale factor ([`Self::simulate_scale_factor_change`],
///   delivered through the resize path exactly like the winit backend);
/// - `request_redraw` dispatching through the window handler on the
///   owner thread, a request from another thread in the owner's next turn,
///   like a real backend's frame wire;
/// - programmatic event injection ([`Self::inject_event`]) and lifecycle
///   simulation (`simulate_resize` / `simulate_focus` /
///   `simulate_occlusion` / `simulate_close`), each calling the same
///   handler method a real platform would.
///
/// There is no public constructor: obtain one by opening a window on a
/// [`HeadlessPlatform`](crate::HeadlessPlatform) and downcasting the returned
/// `Rc<dyn PlatformWindow>` via its `as_any()` to `&MockWindow`.
#[derive(Clone)]
pub struct MockWindow {
    id: WindowId,
    state: Arc<Mutex<MockWindowState>>,
    /// The published snapshot, shared by every clone of this window.
    window_state: Arc<WindowStateCell>,
    /// What the window's state publisher reads; the delivery holds it weakly.
    source: Arc<SnapshotSource>,
    limits: HeadlessWindowPolicy,
    delivery: Arc<WindowDelivery>,
    /// What the window holds of the table's window-level positions.
    facets: WindowFacets,
    /// Builds this window's text-store host, when the platform was made
    /// pull-model.
    text_store_host: Option<TextStoreHostFactory>,
    /// The input regions last handed to the simulated OS, and every change in order.
    input_regions: Arc<Mutex<RecordedInputRegions>>,
    /// Back-reference to the platform this window was opened on, so closing
    /// it can — only once every tracked window is gone — consult the
    /// exit-policy hook exactly like the winit backend's `CloseRequested`
    /// handling does (see `notify_closed`). `Weak`: a window must never
    /// keep the platform state alive past the platform's own lifetime.
    platform_state: Weak<Mutex<HeadlessState>>,
    /// The windows the platform tracks, which this one leaves when it closes
    /// and whose layout [`PlatformWindow::window_at`] reads. `Weak`, as above.
    windows: Weak<HeadlessWindows>,
    /// The OS drag session this window can start, when the platform was given one.
    drag: Option<Arc<dyn flui_platform::drag::PlatformDragSource>>,
    /// Carries a frame request made off the owner thread to the owner's next
    /// turn; `None` for a window opened with no owner signal.
    redraw: HeadlessRedraw,
    /// How many frames were presented into the window's surface: each one
    /// calls its target's `pre_present_notify` first.
    presents: Arc<std::sync::atomic::AtomicU64>,
}

/// How a [`MockWindow`] asks its owner for a frame from another thread: the
/// frame callback is owner code, so it runs in an owner turn, coalesced with
/// any request already queued, as a native backend posts its redraw.
#[derive(Clone)]
struct OwnerRedraw {
    signal: Weak<crate::owner::signal::OwnerSignal>,
    wake: crate::owner::signal::WindowWake,
}

/// How a [`MockWindow`] is asked for a frame, from any thread: on the owner
/// its handler's `frame_requested` runs now; from another thread the request
/// waits for the owner's next turn.
#[derive(Clone)]
struct HeadlessRedraw {
    delivery: Weak<WindowDelivery>,
    /// Set once the window closed: a request after it wakes no owner turn.
    retirement: crate::owner::window_delivery::Retirement,
    /// `None` for a window opened with no owner signal.
    owner: Option<OwnerRedraw>,
}

impl flui_platform::RedrawRequester for HeadlessRedraw {
    fn request_redraw(&self) {
        if self.retirement.is_retired() {
            return;
        }
        if let Some(redraw) = &self.owner
            && let Some(signal) = redraw.signal.upgrade()
            && signal.owner() != thread::current().id()
        {
            (redraw.wake)();
            return;
        }
        if let Some(delivery) = self.delivery.upgrade() {
            delivery.dispatch_request_frame();
        }
    }
}

/// A headless window's surface target: there is no native window, so it
/// answers `Unavailable` always; it counts the frames presented into it.
struct HeadlessSurface {
    presents: Arc<std::sync::atomic::AtomicU64>,
}

impl raw_window_handle::HasWindowHandle for HeadlessSurface {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }
}

impl raw_window_handle::HasDisplayHandle for HeadlessSurface {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }
}

impl flui_platform::SurfaceTarget for HeadlessSurface {
    fn pre_present_notify(&self, _: flui_platform::SwapChainStatistics) {
        self.presents
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
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
            .field("delivery", &self.delivery)
            .finish_non_exhaustive()
    }
}

/// Whether the simulated window system takes a verb whose capability is
/// `availability`: a closed window takes none, and an `Unavailable`
/// capability is refused as admission refuses a command.
fn takes(
    state: &MockWindowState,
    availability: flui_platform::Availability,
) -> Result<(), Unsupported> {
    if state.closed {
        return Err(Unsupported::new(UnsupportedReason::NoWindow));
    }
    if !availability.is_available() {
        return Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform));
    }
    Ok(())
}

/// What a [`MockWindow`] handed its simulated OS as input regions.
#[derive(Debug, Default)]
struct RecordedInputRegions {
    projection: ProjectedInputRegions,
    updates: Vec<Vec<DeviceInputRegion>>,
}
/// What the platform hands a window it opens.
struct MockWindowHost {
    /// What the window hands out of the window-level positions.
    positions: WindowPositions<()>,
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
    /// Every [`PlatformWindow::set_frame_rate_hint`] the window took, in order.
    frame_rate_hints: Vec<flui_platform::FrameRateHint>,
    /// Every [`WindowCommand::SetMaterial`] the window applied, in order.
    materials: Vec<WindowMaterial>,
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
    occluded: bool,
    execution: flui_platform::WindowExecutionState,
    safe_area: flui_foundation::geometry::EdgeInsets,
    closed: bool,

    appearance: WindowAppearance,
    cursor: CursorIcon,
    /// The display this window is on; `None` when the platform has no displays.
    display: Option<DisplayId>,
    /// What the simulated window system lets this window do now.
    capabilities: WindowCapabilities,
    /// A snapshot the simulated window system reported in place of what the
    /// model says ([`MockWindow::simulate_state`]), until the model changes.
    reported: Option<WindowState>,
}

impl MockWindowState {
    fn geometry(&self) -> WindowGeometry {
        WindowGeometry::new(Some(self.position), self.size)
    }

    /// `size` as the host ends up with it: raised to the window's minimum,
    /// lowered to its maximum and to the largest window the host allows.
    fn clamp_size(&self, size: Size<f64>, limits: &HeadlessWindowPolicy) -> Size<f64> {
        Self::clamp_to_host(
            crate::window::state::held_to(size, self.constraints),
            limits,
        )
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
/// mode without `observes_minimized`, and no frame while fullscreen. A
/// closed window is detached; a size or scale that is not a drawable area
/// reports no metrics. The hover is what the last hover report said. A
/// snapshot the window system reported is read as it was reported.
fn sample_window_state(model: &MockWindowState, hovered: bool) -> WindowState {
    if let Some(reported) = &model.reported {
        return reported.clone();
    }
    let capabilities = &model.capabilities;
    let observed = |geometry: WindowGeometry| {
        WindowGeometry::new(
            geometry
                .position
                .filter(|_| capabilities.observes_position.is_available()),
            geometry.size,
        )
    };
    let mode = match model.mode {
        WindowMode::Minimized if !capabilities.observes_minimized.is_available() => {
            WindowMode::Normal
        }
        mode => mode,
    };
    let execution = if model.closed {
        flui_platform::WindowExecutionState::Detached
    } else {
        model.execution
    };
    let state = WindowState::new(observed(model.geometry()), model.size)
        .with_mode(mode)
        .with_normal_geometry(model.normal_geometry.map(observed))
        .with_visible(model.visible && !model.closed)
        .with_focused(model.focused)
        .with_title(Some(model.title.clone()))
        .with_constraints(model.constraints)
        .with_resizable(model.resizable)
        .with_decorated(model.decorated && model.mode != WindowMode::Fullscreen)
        .with_display(model.display);
    // This host has no activation apart from focus.
    ReportedFacts {
        metrics: flui_foundation::geometry::WindowMetrics::from_logical(
            model.size,
            model.scale_factor,
        )
        .ok(),
        appearance: model.appearance,
        safe_area: model.safe_area,
        execution,
        occluded: model.occluded,
        hovered,
        active: model.focused,
        capabilities: *capabilities,
    }
    .onto(state)
}

/// Read what `model` says now into the snapshot, announcing nothing.
fn read_window_state(
    cell: &WindowStateCell,
    model: &Mutex<MockWindowState>,
    delivery: &WindowDelivery,
) -> crate::window::state::Published {
    cell.invalidate();
    cell.state_with(|| sample_window_state(&model.lock(), delivery.hovered()))
}

/// What a window's state publisher reads: the model standing in for the OS,
/// held by every clone of the window.
struct SnapshotSource {
    cell: Arc<WindowStateCell>,
    model: Arc<Mutex<MockWindowState>>,
    delivery: Weak<WindowDelivery>,
}

impl SnapshotSource {
    fn read(&self) -> Publication {
        self.delivery
            .upgrade()
            .map_or(Publication::Unchanged, |delivery| {
                read_window_state(&self.cell, &self.model, &delivery).outcome()
            })
    }
}

impl MockWindow {
    fn new(
        id: WindowId,
        options: WindowOptions,
        platform_state: Weak<Mutex<HeadlessState>>,
        windows: Weak<HeadlessWindows>,
        host: MockWindowHost,
        delivery: WindowDelivery,
    ) -> Self {
        let MockWindowHost {
            positions,
            text_store_host,
            display,
            display_bounds,
            capabilities,
            limits,
        } = host;
        let mut model = MockWindowState {
            frame_rate_hints: Vec::new(),
            materials: Vec::new(),
            title: options.title.clone(),
            position: options
                .position
                .filter(|_| capabilities.places_at_open.is_available())
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
            occluded: false,
            closed: false,

            appearance: WindowAppearance::default(),
            cursor: CursorIcon::default(),
            display,
            capabilities,
            reported: None,
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
        let initial = sample_window_state(&model, false);
        let state = Arc::new(Mutex::new(model));
        let window_state = Arc::new(WindowStateCell::new(initial));
        let delivery = Arc::new(delivery);
        let source = Arc::new(SnapshotSource {
            cell: Arc::clone(&window_state),
            model: Arc::clone(&state),
            delivery: Arc::downgrade(&delivery),
        });
        delivery.publish_through(Arc::downgrade(&source), SnapshotSource::read);
        let redraw_delivery = Arc::downgrade(&delivery);
        let redraw_retirement = delivery.retirement();
        Self {
            id,
            state,
            window_state,
            source,
            limits,
            delivery,
            facets: positions.for_window(&()),
            text_store_host,
            input_regions: Arc::default(),
            platform_state,
            windows,
            drag: None,
            redraw: HeadlessRedraw {
                delivery: redraw_delivery,
                retirement: redraw_retirement,
                owner: None,
            },
            presents: Arc::default(),
        }
    }

    /// The window's display bounds in logical pixels, from the platform's
    /// current displays; `None` when the platform is gone or knows none.
    fn display_bounds(&self) -> Option<Bounds<f64>> {
        let display = self.state.lock().display?;
        self.platform_state
            .upgrade()?
            .lock()
            .display_bounds(display)
    }

    /// Publish what the model says now: read it into a snapshot, and raise
    /// the handler's `state_changed` after the cell's lock is released when the revision
    /// grew.
    fn publish_state(&self) -> WindowState {
        // The model changed, so a snapshot reported in its place is over.
        self.state.lock().reported = None;
        let published = read_window_state(&self.source.cell, &self.source.model, &self.delivery);
        if published.changed {
            self.delivery.dispatch_state_changed();
        }
        published.state
    }

    /// The whole close teardown, in the order both production backends run it:
    /// hide, deliver `closed`, drop the window from tracking, let go of its
    /// handler, and consult the exit policy only once nothing is tracked any
    /// more.
    ///
    /// Mirrors `WinitAppHandler::complete_window_close`. Two of its steps have
    /// no analogue here and are deliberately absent rather than forgotten:
    /// there is no native handle to hide beyond the state flag, and
    /// `forget_window` has nothing to forget because this backend's data
    /// transfer is the null source.
    ///
    /// Reached from both routes — [`WindowCommand::Close`] and
    /// [`Self::simulate_close`] — so the two cannot drift apart.
    fn complete_close(&self) {
        {
            let mut state = self.state.lock();
            state.visible = false;
            state.closed = true;
        }
        // Hidden and detached, in one revision, before the window is gone.
        self.publish_state();
        self.delivery.dispatch_close();
        // What the window-level positions put on the window goes with it.
        self.facets.text.detach();
        self.facets.accessibility.detach();
        self.notify_closed();
        // After the exit-policy consult, never before: a hook that inspects
        // the window it was just told about must not find its handler
        // already gone.
        self.delivery.clear();
    }

    /// Removes this window from the platform's own tracking and, only once
    /// every window this platform knows about is gone, asks the application's
    /// handler ([`crate::AppHandler::exit_policy`]) exactly as the winit
    /// backend's `CloseRequested` handling does; with no handler bound the
    /// platform keeps running.
    ///
    /// The handler is asked, and told of the quit, with the state lock
    /// released (ADR-0039): its body (`flui-app`'s `AppRuntime::should_exit`)
    /// drops removed UI runtime state, whose destructors may call back into
    /// this platform (a dispose hook opening another window, say).
    fn notify_closed(&self) {
        let Some(platform_state) = self.platform_state.upgrade() else {
            return;
        };
        if let Some(windows) = self.windows.upgrade() {
            windows.forget(self.id);
        }
        consult_exit_policy(&platform_state);
    }

    /// How many times the frame pump has called
    /// [`SurfaceTarget::pre_present_notify`](flui_platform::SurfaceTarget::pre_present_notify) on this window's surface target — one per
    /// presented frame, before the present, is the contract.
    #[must_use]
    pub fn pre_present_notifies(&self) -> u64 {
        self.presents.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Simulate the window system reporting that a frame of this window
    /// reached the screen at `presented.at`, as a compositor's presentation
    /// event does: the window's next frame request at or after that time
    /// carries it ([`FrameRequest::presented`](flui_platform::FrameRequest::presented)).
    /// A headless window has no screen, so this is the only report it makes;
    /// a report no later than one already made is dropped, as on a real
    /// host. A closed window ignores it, as does one whose capabilities say
    /// its window system reports no presentation
    /// ([`WindowCapabilities::reports_presentation`]).
    pub fn simulate_presented(&self, presented: flui_platform::FramePresented) {
        {
            let state = self.state.lock();
            if state.closed || !state.capabilities.reports_presentation.is_available() {
                return;
            }
        }
        self.delivery.presentation_reports().report(presented);
    }

    /// Every input-region set this window handed its simulated OS, in device pixels and
    /// delivery order: one entry per change of the projection, scale or display.
    #[must_use]
    pub fn input_region_updates(&self) -> Vec<Vec<DeviceInputRegion>> {
        self.input_regions.lock().updates.clone()
    }
    /// What the simulated window system holds for this window now, read from
    /// its model and never through the snapshot: what a conformance fixture
    /// compares the snapshot with, as a native fixture reads the operating
    /// system.
    #[must_use]
    pub fn window_system(&self) -> HeadlessWindowSystem {
        let input_regions = self.delivery.input_capabilities().input_regions();
        let state = self.state.lock();
        HeadlessWindowSystem {
            input_regions,
            material: state.materials.last().copied().unwrap_or_default(),
            metrics: flui_foundation::geometry::WindowMetrics::from_logical(
                state.size,
                state.scale_factor,
            )
            .ok(),
            focused: state.focused,
            visible: state.visible && !state.closed,
            occluded: state.occluded,
        }
    }

    /// Every frame-rate hint the window took, oldest first.
    #[must_use]
    pub fn frame_rate_hints(&self) -> Vec<flui_platform::FrameRateHint> {
        self.state.lock().frame_rate_hints.clone()
    }

    /// Every material the window applied, oldest first.
    #[must_use]
    pub fn materials(&self) -> Vec<WindowMaterial> {
        self.state.lock().materials.clone()
    }

    /// Inject a platform input event for testing.
    /// Delivers to the window handler's `input`.
    pub fn inject_event(&self, event: PlatformInput) -> DispatchEventResult {
        self.delivery.dispatch_input(event)
    }

    /// Simulate the window system reporting `state` for this window, as a
    /// replay of a `window_state` recording does. It is published as any read
    /// of the window system is, through [`WindowState::successor`]: the
    /// revision is the one this window's history earns, not `state`'s, a
    /// snapshot with the same facts publishes nothing, and the capabilities
    /// keep this window's owner. Fires the handler's `state_changed` when the
    /// snapshot differs, and returns what was published.
    ///
    /// Reads answer with `state` until a simulation or a command changes the
    /// window's own model, which publishes what the model says again.
    pub fn simulate_state(&self, state: WindowState) -> WindowState {
        self.state.lock().reported = Some(state);
        let published = read_window_state(&self.source.cell, &self.source.model, &self.delivery);
        if published.changed {
            self.delivery.dispatch_state_changed();
        }
        published.state
    }

    /// Simulate a resize for testing. Fires the handler's `state_changed` when the
    /// snapshot differs; a size that is not a drawable area keeps the metrics
    /// the snapshot already holds.
    pub fn simulate_resize(&self, width: f64, height: f64) {
        self.state.lock().size = Size::new(width, height);
        self.publish_state();
    }

    /// Simulate the user (or the OS) moving the window to `position`, outer
    /// top-left in global logical pixels. This host draws no frame, so that is
    /// also where its content starts: [`PlatformWindow::window_at`] answers
    /// from it. Fires the handler's `state_changed` when the snapshot differs.
    pub fn simulate_move(&self, position: Point<f64>) {
        self.state.lock().position = position;
        self.publish_state();
    }

    /// Simulate a monitor DPI change for testing: the scale factor moves with
    /// no logical size change, and the snapshot's metrics carry the new scale.
    pub fn simulate_scale_factor_change(&self, scale_factor: f64) {
        self.state.lock().scale_factor = scale_factor;
        self.publish_state();
    }

    /// Simulate focus change for testing. Fires the handler's `state_changed` when the
    /// snapshot differs.
    pub fn simulate_focus(&self, focused: bool) {
        self.state.lock().focused = focused;
        self.publish_state();
    }

    /// Simulate the pointer entering (`true`) or leaving the window's content,
    /// through the one hover report every host uses. Fires the handler's `state_changed`
    /// when the snapshot differs; a closed window ignores the report.
    pub fn simulate_hover(&self, hovered: bool) {
        if self.state.lock().closed {
            return;
        }
        self.delivery.report_hover(hovered);
    }

    /// Simulate an owner-thread safe-area report. Closed and detached windows
    /// ignore the report.
    pub fn simulate_safe_area(&self, insets: flui_foundation::geometry::EdgeInsets) {
        {
            let mut state = self.state.lock();
            if state.closed || state.execution == flui_platform::WindowExecutionState::Detached {
                return;
            }
            state.safe_area = insets;
        }
        self.publish_state();
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
        self.publish_state();
    }

    /// Simulate the window system covering the window, or showing it again:
    /// the window stays shown and its snapshot says occluded. Fires the
    /// handler's `state_changed` when the snapshot differs. Hiding is
    /// [`WindowCommand::Hide`].
    pub fn simulate_occlusion(&self, occluded: bool) {
        self.state.lock().occluded = occluded;
        self.publish_state();
    }

    /// Simulate the system light/dark appearance this window observes. Fires
    /// the handler's `state_changed` when the snapshot differs; closed windows ignore the
    /// report.
    pub fn simulate_appearance(&self, appearance: WindowAppearance) {
        {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.appearance = appearance;
        }
        self.publish_state();
    }

    /// Simulate the window system revising what it lets this window do, as a
    /// compositor that stops honouring a request does. Fires
    /// the handler's `state_changed` when the snapshot differs, with the capabilities at
    /// a new revision; a closed window ignores the report.
    pub fn simulate_capabilities(&self, capabilities: WindowCapabilities) {
        {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.capabilities = capabilities;
        }
        self.publish_state();
    }

    /// Move this window to display `id`, as the user dragging it onto another
    /// monitor does. Fires the handler's `state_changed` when the snapshot differs; a
    /// closed window ignores the report.
    ///
    /// # Errors
    /// [`UnknownDisplay`] when the platform holds no display with this id (or
    /// is gone); the window stays where it was.
    pub fn simulate_move_to(&self, id: DisplayId) -> Result<(), UnknownDisplay> {
        let known = self
            .platform_state
            .upgrade()
            .is_some_and(|platform| platform.lock().holds_display(id));
        if !known {
            return Err(UnknownDisplay { display: id });
        }
        {
            let mut state = self.state.lock();
            if state.closed {
                return Ok(());
            }
            state.display = Some(id);
        }
        self.publish_state();
        Ok(())
    }

    /// Simulate a GPU-surface availability change for testing.
    /// Delivers to the window handler's `surface_changed`.
    ///
    /// No backend in this workspace emits this signal but Android, so this is
    /// the only way a host-run test can drive the seam the released-surface
    /// path hangs off: [`SurfaceChange::Release`] asks the handler to release
    /// its surface, [`SurfaceChange::Rebuild`] to build one again.
    pub fn simulate_surface_change(&self, change: SurfaceChange) {
        self.delivery.dispatch_surface_change(change);
    }

    /// Simulate close request for testing.
    /// Fires the handler's `close_requested`, then `closed` if it closes, then (real,
    /// production behavior — not a test-only shortcut) removes this window
    /// from the platform's tracking and consults the exit-policy hook if
    /// every tracked window is now gone. See `Self::notify_closed`.
    ///
    /// Raised while one of the handler's methods runs, the request is
    /// answered `false` at once and asked behind it; the window then closes
    /// if the handler grants it.
    pub fn simulate_close(&self) -> bool {
        let later = self.clone();
        let should = self
            .delivery
            .dispatch_close_request(move || later.complete_close())
            == flui_platform::CloseDecision::Close;
        if should {
            self.complete_close();
        }
        should
    }
}

impl PlatformWindow for MockWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn request_redraw(&self) {
        flui_platform::RedrawRequester::request_redraw(&self.redraw);
    }

    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        Arc::new(self.redraw.clone())
    }

    /// No native window exists: the target answers `Unavailable` always and
    /// counts the frames presented into it ([`Self::pre_present_notifies`]).
    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        Arc::new(HeadlessSurface {
            presents: Arc::clone(&self.presents),
        })
    }

    // ==================== State and commands (ADR-0237) ====================

    fn state(&self) -> WindowState {
        self.publish_state()
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if self.state.lock().closed {
            return Err(ApplyError::Closed);
        }
        let command = command.against(&self.window_state.published_capabilities())?;
        if command == WindowCommand::Close {
            // Synchronous, as a same-thread Win32 `DestroyWindow` is: the
            // teardown and `closed` run before this returns.
            self.complete_close();
            return Ok(());
        }
        let display_bounds = match command {
            WindowCommand::SetMode(WindowMode::Maximized | WindowMode::Fullscreen) => {
                self.display_bounds()
            }
            _ => None,
        };
        {
            let mut state = self.state.lock();
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
                WindowCommand::SetMaterial(material) => state.materials.push(material),
                // A command this host does not know is refused by `admit`
                // through its capability, and `Close` returned above.
                _ => {}
            }
        }
        self.publish_state();
        Ok(())
    }

    fn set_frame_rate_hint(&self, hint: flui_platform::FrameRateHint) -> Result<(), Unsupported> {
        // The snapshot's lock is taken before the model's, never inside it:
        // a publication holds the snapshot while it reads the model.
        let availability = self.window_state.published_capabilities().frame_rate_hint;
        let mut state = self.state.lock();
        takes(&state, availability)?;
        state.frame_rate_hints.push(hint);
        Ok(())
    }

    /// Taken while the window's
    /// [`InputCapabilities::input_regions`](flui_platform::input_capabilities::InputCapabilities::input_regions)
    /// says the simulated system honours them, as it does unless a test said
    /// otherwise.
    fn set_input_regions(&self, regions: &[InputRegion]) -> Result<(), Unsupported> {
        let availability = self.delivery.input_capabilities().input_regions();
        let (scale, display) = {
            let state = self.state.lock();
            takes(&state, availability)?;
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
        Ok(())
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::ime)
    }

    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        Some(Arc::clone(&self.facets.haptics))
    }

    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        self.drag.clone()
    }

    fn window_at(&self, position: Point<f64>) -> flui_platform::WindowAt {
        use flui_platform::WindowAt;
        let Some(tracked) = self.windows.upgrade() else {
            return WindowAt::Unknown;
        };
        // The set's lock is released before any window's is taken.
        let windows = {
            let tracked = tracked.tracked.lock();
            if !tracked.placement {
                return WindowAt::Unknown;
            }
            tracked.windows.clone()
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
                let shown = state.visible
                    && !state.closed
                    && state.mode != WindowMode::Minimized
                    && !state.occluded;
                (shown && inside).then_some(WindowAt::Window {
                    window: window.id,
                    position: local,
                })
            })
            .unwrap_or(WindowAt::Outside)
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    // ==================== Control Methods ====================

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.state.lock().cursor = cursor;
        Ok(())
    }
}

impl crate::host_window::CoreHostWindow for MockWindow {
    fn window_delivery(&self) -> &WindowDelivery {
        &self.delivery
    }
}

impl HostWindow for MockWindow {
    fn accessibility(&self) -> Option<Arc<dyn flui_semantics::platform::PlatformAccessibility>> {
        Some(Arc::clone(&self.facets.accessibility) as _)
    }

    fn capture_text_sizing(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        self.facets.text.capture_sizing()
    }

    /// The host a test made the platform pull-model with, else what the text
    /// position hands out.
    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<Rc<dyn TextStoreHost>> {
        self.text_store_host.as_ref().map_or_else(
            || {
                self.facets
                    .text
                    .window_text_input()
                    .and_then(WindowTextInput::store_host)
            },
            |factory| Some(factory()),
        )
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

/// Each headless window records its own overlay styles.
impl host::Chrome<()> for FakeSystemChrome {
    fn for_window((): &()) -> Arc<dyn flui_platform::PlatformSystemChrome> {
        Arc::new(Self::default())
    }
}
