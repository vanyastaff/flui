//! The winit window: [`WinitWindow`], the backend's [`PlatformWindow`], a thin
//! wrapper over `Arc<winit::window::Window>` carrying the platform-minted
//! [`WindowId`], the per-window callback table, and the route its
//! programmatic `WindowCommand::Close` takes back to the event-loop owner
//! (`ControlSender`, crate-private); [`plan`], what the backend decides about
//! a window before and while it exists; and [`WinitWindows`], which opens the
//! windows the loop tracks (`owner::winit::LoopWindows`).

use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use cursor_icon::CursorIcon;

use flui_foundation::geometry::{Point, Size, WindowMetrics};
use parking_lot::Mutex;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::{
    dpi::{LogicalPosition, LogicalSize},
    event_loop::ActiveEventLoop,
    window::{Fullscreen, Window, WindowAttributes},
};

use crate::host::{self, WindowFacets, WindowPositions};
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;

use crate::owner::window_delivery::{Publication, WindowDelivery};
use crate::owner::winit::HostDisplays;
use crate::owner::winit::control::ControlSender;
use crate::owner::winit::{
    ACTIVE_EVENT_LOOP, LoopWindow, LoopWindows, OpenWindowStateError, WindowSeed, WinitLoop,
};
use crate::text::WindowTextInput;
#[cfg(target_os = "linux")]
use crate::window::WindowManagerView;
use crate::window::plan::{FrameInset, OpenPlan, PlacementDisplay, WindowShell};
use crate::window::state::{ModeFlags, ReportedFacts, WindowStateCell, mode_of};
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{
    AdmittedCommand, ApplyError, CursorError, DisplayId, PlatformTextInput, PlatformWindow,
    SizeConstraints, WindowAppearance, WindowCapabilities, WindowCommand, WindowGeometry,
    WindowHandler, WindowId, WindowMode, WindowOptions, WindowState,
};
use flui_semantics::platform::PlatformAccessibility;

/// How long a window that opens unmapped waits for the window manager to say
/// what frame it will have.
#[cfg(target_os = "linux")]
const FRAME_EXTENTS_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// Concrete winit window wrapper
///
/// Wraps `winit::window::Window` to implement `PlatformWindow`.
/// Includes what the window delivers to its handler, using the causal FIFO
/// dispatch pattern for reentrancy safety.
pub struct WinitWindow {
    id: WindowId,
    closed: AtomicBool,
    window: Arc<Window>,
    is_focused: parking_lot::Mutex<bool>,
    is_visible: parking_lot::Mutex<bool>,
    delivery: Arc<WindowDelivery>,
    /// What the table's positions made for this window with it: its
    /// accessibility bridge (on Linux its AT-SPI announcement, so a screen
    /// reader that attaches at any later point finds it on the bus), its
    /// actuator and system chrome (none on desktop winit targets), and
    /// winit's IME as its text input.
    facets: WindowFacets,
    /// The Settings portal's `color-scheme` as last delivered to this window.
    /// winit reports no theme on X11, nor on Wayland unless the app overrode
    /// it, so the portal is the Linux source.
    #[cfg(target_os = "linux")]
    portal_dark: std::sync::atomic::AtomicBool,
    /// The route a programmatic [`WindowCommand::Close`] takes to the
    /// event-loop owner (issue #919). The close teardown needs the live
    /// `ActiveEventLoop` (to end the loop when this was the last window),
    /// which only an owner-turn callback holds — so the command cannot run it
    /// in place; it posts a non-droppable per-window request and the owner
    /// runs the exact teardown the compositor's `CloseRequested` arm runs.
    close_lane: ControlSender,
    /// The platform's native display source, by which this window names its
    /// display.
    displays: Arc<dyn HostDisplays>,
    /// The display the window was last read on, so the window is read again
    /// only when it really moved.
    reported_display: parking_lot::Mutex<Option<DisplayId>>,
    /// The snapshot last published (ADR-0237). Shared with the platform so a
    /// wake from sleep marks it stale.
    window_state: Arc<WindowStateCell>,
    /// What `apply` honours on this window.
    capabilities: WindowCapabilities,
    /// The title last set. winit cannot read one back on X11 or Wayland.
    title: Mutex<String>,
    /// The size limits last set. winit has no getter for them.
    constraints: Mutex<SizeConstraints>,
    /// The rectangle the window returns to from a maximized or fullscreen
    /// mode: the geometry it had in normal mode, or the one it opened with.
    normal_geometry: Mutex<Option<WindowGeometry>>,
    /// Whether the window was created hidden and still waits for its first
    /// frame (or an explicit show) to become visible.
    reveal_pending: Mutex<bool>,
    /// Whether the window was asked to minimize while it was unmapped: it is
    /// minimized once mapped.
    minimize_on_map: AtomicBool,
    /// Whether the window was created unmapped to open maximized or fullscreen
    /// and is mapped once its mode is set: see [`OpenPlan::map_after_setup`].
    map_after_setup: bool,
    /// The mode an unmapped window opens in. A window manager reads it from the
    /// window's properties when it maps the window, so it is written there.
    #[cfg(target_os = "linux")]
    opens_in: WindowMode,
    /// The outer size an unmapped window opened with a remembered rectangle
    /// has to end up with once its frame is known.
    #[cfg(target_os = "linux")]
    outer_to_fit: Mutex<Option<Size<f64>>>,
    /// The frame around decorated windows, measured from this and the
    /// platform's other windows.
    frame_inset: Arc<FrameInset>,
    /// The X11 probe and this window's X11 id, once attached.
    #[cfg(target_os = "linux")]
    x11: std::sync::OnceLock<(Arc<dyn WindowManagerView>, u32)>,
    /// Set when this is an X11 window and no probe could be attached: its
    /// minimization then goes unseen until some other event samples it.
    #[cfg(target_os = "linux")]
    minimize_unobserved: AtomicBool,
    /// Whether winit last reported the window fully covered (`Occluded`).
    occluded: AtomicBool,
}

/// What the window system lets a program do to `window`: a Wayland surface
/// gets none of the placement, hiding and focus commands, every other shell
/// gets them all.
pub(crate) fn shell_of(window: &impl HasDisplayHandle) -> WindowShell {
    match window.display_handle().map(|display| display.as_raw()) {
        Ok(raw_window_handle::RawDisplayHandle::Wayland(_)) => WindowShell::Wayland,
        _ => WindowShell::Desktop,
    }
}

impl std::fmt::Debug for WinitWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The window's delivery holds a handler that need not implement
        // `Debug`; print the focus/visibility flags only.
        f.debug_struct("WinitWindow")
            .field("is_focused", &*self.is_focused.lock())
            .field("is_visible", &*self.is_visible.lock())
            .finish_non_exhaustive()
    }
}

impl WinitWindow {
    /// Create a new WinitWindow wrapper from what the loop's `seed` hands it:
    /// the platform-minted id the loop already allocated, the owner lane its
    /// programmatic `WindowCommand::Close` reports to, the displays and the
    /// handler it delivers to. Backend-private: only this backend's own
    /// window creation can supply that lane, so nothing outside it can
    /// construct a window whose close would go nowhere.
    ///
    /// `plan` is what the window was created from, so its snapshot starts from
    /// the mode it opened in and, if it opened maximized, the rectangle it
    /// returns to; `windows` holds the frame inset it reports and the
    /// positions that make what it hands out.
    pub(crate) fn new(
        seed: WindowSeed<'_>,
        window: Arc<Window>,
        options: &WindowOptions,
        plan: &OpenPlan,
        windows: &WinitWindows,
    ) -> Self {
        let WindowSeed {
            id,
            close_lane,
            displays,
            handler,
            ..
        } = seed;
        let displays = Arc::clone(displays);
        let positions = &windows.positions;
        let shell = shell_of(&window);
        let facets = positions.for_window(&window);
        let capabilities = shell.capabilities();
        let constraints = SizeConstraints::new(options.min_size, options.max_size);
        let title = options.title.clone();
        let display = displays.id_of_window(&window);
        let initial = sample_state(
            &window,
            &RecordedState {
                title: &title,
                constraints,
                focused: true,
                visible: options.visible,
                display,
                probed_mode: None,
                reported: reported_facts(
                    &window,
                    appearance_of(&window, false),
                    true,
                    false,
                    capabilities,
                ),
            },
        );
        Self {
            id,
            closed: AtomicBool::new(false),
            window_state: Arc::new(WindowStateCell::new(initial)),
            capabilities,
            title: Mutex::new(title),
            constraints: Mutex::new(constraints),
            normal_geometry: Mutex::new(plan.normal_geometry),
            map_after_setup: plan.map_after_setup,
            #[cfg(target_os = "linux")]
            opens_in: if plan.unmapped_until_shown() {
                plan.mode
            } else {
                WindowMode::Normal
            },
            #[cfg(target_os = "linux")]
            outer_to_fit: Mutex::new(
                plan.normal_geometry
                    .filter(|_| plan.unmapped_until_shown())
                    .map(|geometry| geometry.size),
            ),
            reveal_pending: Mutex::new(plan.hidden_until_first_frame),
            minimize_on_map: AtomicBool::new(false),
            frame_inset: Arc::clone(&windows.frame_inset),
            #[cfg(target_os = "linux")]
            x11: std::sync::OnceLock::new(),
            #[cfg(target_os = "linux")]
            minimize_unobserved: AtomicBool::new(false),
            window,
            is_focused: parking_lot::Mutex::new(true),
            is_visible: parking_lot::Mutex::new(options.visible),
            occluded: AtomicBool::new(false),
            delivery: Arc::new(WindowDelivery::new(handler, InputCapabilities::UNAVAILABLE)),
            facets,
            #[cfg(target_os = "linux")]
            portal_dark: std::sync::atomic::AtomicBool::new(false),
            close_lane,
            displays,
            reported_display: parking_lot::Mutex::new(None),
        }
    }

    /// Read this window's mode, and hear of its changes, through the probe
    /// `probe` makes. Does nothing, and makes none, for a window that is not
    /// an X11 window.
    #[cfg(target_os = "linux")]
    pub(crate) fn attach_x11_probe(
        &self,
        probe: impl FnOnce() -> Option<Arc<dyn WindowManagerView>>,
    ) {
        use raw_window_handle::RawWindowHandle;

        let id = match self.window.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Xlib(handle)) => u32::try_from(handle.window).ok(),
            Ok(RawWindowHandle::Xcb(handle)) => Some(handle.window.get()),
            _ => None,
        };
        let Some(id) = id else {
            return;
        };
        let Some(probe) = probe() else {
            // winit reports no event when an X11 window is minimized, so
            // without the probe the snapshot cannot follow it.
            self.minimize_unobserved.store(true, Ordering::Release);
            return;
        };
        probe.watch(id);
        // Only a window that stays unmapped has a mode to seed: one already
        // mapped is the window manager's to place, and writing its properties
        // would undo a rule that maximized it meanwhile.
        if self.opens_in != WindowMode::Normal {
            probe.set_mode_hint(id, self.opens_in);
        }
        let _ = self.x11.set((probe, id));
        // The snapshot made at creation knew nothing of the probe.
        self.refresh_state();
    }

    /// The mode the X server and window manager report, if a probe is attached
    /// and can say.
    #[cfg_attr(
        not(target_os = "linux"),
        expect(clippy::unused_self, reason = "only an X11 window has a probe to ask")
    )]
    fn probed_mode(&self) -> Option<WindowMode> {
        #[cfg(target_os = "linux")]
        {
            let (probe, id) = self.x11.get()?;
            probe.mode(*id)
        }
        #[cfg(not(target_os = "linux"))]
        None
    }

    /// The cell a wake from sleep marks stale.
    pub(crate) fn state_cell(&self) -> &Arc<WindowStateCell> {
        &self.window_state
    }

    /// Get the underlying `Arc<Window>`
    pub fn inner(&self) -> &Arc<Window> {
        &self.window
    }

    /// Update focus state
    pub fn set_focused(&self, focused: bool) {
        *self.is_focused.lock() = focused;
    }

    /// Update visibility state
    pub fn set_visible(&self, visible: bool) {
        *self.is_visible.lock() = visible;
    }

    /// Read the window again for its state publisher, announcing nothing:
    /// `WindowDelivery::dispatch_state_changed` calls `WindowHandler::state_changed`. A minimize
    /// the X11 window manager reports is the snapshot's mode, which
    /// `WindowState::is_seen` reads.
    fn read_state(&self) -> Publication {
        self.window_state.invalidate();
        self.window_state.state_with(|| self.sample()).outcome()
    }

    /// The window as the operating system describes it now.
    ///
    /// Reads winit with no lock of the snapshot held, and keeps the rectangle
    /// a maximized or fullscreen window returns to: the outer geometry the
    /// window had the last time it was in normal mode.
    fn sample(&self) -> WindowState {
        let title = self.title.lock().clone();
        // Read once: a second lock of the flag inside the literal below would
        // wait on the first one's guard, alive until the statement ends.
        let focused = *self.is_focused.lock();
        let mut state = sample_state(
            &self.window,
            &RecordedState {
                title: &title,
                constraints: *self.constraints.lock(),
                focused,
                visible: *self.is_visible.lock() && !self.occluded.load(Ordering::Acquire),
                display: self.display(),
                probed_mode: self.probed_mode(),
                reported: reported_facts(
                    &self.window,
                    self.appearance(),
                    focused,
                    self.delivery.hovered(),
                    self.capabilities(),
                ),
            },
        );
        let mut remembered = self.normal_geometry.lock();
        match state.mode {
            WindowMode::Normal => {
                *remembered = Some(state.outer);
                if state.visible {
                    self.frame_inset.observe(
                        state.decorated,
                        device_size(self.window.outer_size()),
                        device_size(self.window.inner_size()),
                    );
                }
            }
            WindowMode::Maximized | WindowMode::Fullscreen | WindowMode::Minimized => {
                state.normal_geometry = *remembered;
            }
            _ => {}
        }
        #[cfg(target_os = "linux")]
        if self.x11.get().is_some() && state.mode == WindowMode::Minimized {
            state.visible = false;
        }
        state
    }

    /// The display the window is on, as the platform's display snapshot names
    /// it.
    fn display(&self) -> Option<DisplayId> {
        self.displays.id_of_window(&self.window)
    }

    /// The user's theme as applied to the window: winit's, else the portal's.
    fn appearance(&self) -> WindowAppearance {
        #[cfg(target_os = "linux")]
        let portal_dark = self.portal_dark.load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(target_os = "linux"))]
        let portal_dark = false;
        appearance_of(&self.window, portal_dark)
    }

    /// What the window honours now: an X11 window manager that never reported
    /// a minimize leaves the minimized mode unobserved.
    fn capabilities(&self) -> WindowCapabilities {
        #[cfg(target_os = "linux")]
        if self.minimize_unobserved.load(Ordering::Acquire) {
            let mut capabilities = self.capabilities;
            capabilities.observes_minimized = flui_platform::Availability::Unavailable;
            return capabilities;
        }
        self.capabilities
    }

    /// Hide now, from whichever thread this is called on: the visible effect
    /// of a close should not wait for the owner's next turn. The close itself
    /// — `WindowHandler::closed`, map removal, handler release, exit policy — is the
    /// owner's (see `close_lane`): running `WindowHandler::closed` here would call it on
    /// the caller's thread, where an embedder's owner-affine close handling
    /// (`flui-app` rejects ui_runtime dispatch off its owner thread) would
    /// silently refuse it, and skipping the map removal is precisely issue
    /// #919 — a hidden window that the exit policy still counts, so the
    /// process never exits.
    fn close_window(&self) {
        {
            // Closing is terminal for a deferred reveal: the flag is set and the
            // window hidden holding the lock a reveal checks it under, so a
            // reveal on another thread cannot map the window after this.
            let mut pending = self.reveal_pending.lock();
            self.mark_closed();
            *pending = false;
            self.window.set_visible(false);
            *self.is_visible.lock() = false;
        }
        self.close_lane.request_close_window(self.id);
    }

    /// Enter `mode`. Leaving fullscreen and minimized first keeps entering a
    /// mode from depending on what the window manager makes of two requests at
    /// once: it keeps a minimized window minimized under a maximize.
    fn enter_mode(&self, mode: WindowMode) -> Result<(), ApplyError> {
        if *self.reveal_pending.lock() || !*self.is_visible.lock() {
            self.minimize_on_map
                .store(mode == WindowMode::Minimized, Ordering::Release);
        }
        #[cfg(target_os = "linux")]
        self.seed_unmapped_mode(mode);
        match mode {
            WindowMode::Minimized => self.window.set_minimized(true),
            WindowMode::Maximized => {
                self.window.set_fullscreen(None);
                self.window.set_minimized(false);
                self.window.set_maximized(true);
            }
            WindowMode::Fullscreen => {
                self.window.set_minimized(false);
                self.window
                    .set_fullscreen(Some(Fullscreen::Borderless(None)));
            }
            WindowMode::Normal => {
                self.window.set_fullscreen(None);
                self.window.set_minimized(false);
                self.window.set_maximized(false);
            }
            other => {
                return Err(crate::window::state::not_carried_out(
                    &WindowCommand::SetMode(other),
                ));
            }
        }
        Ok(())
    }

    /// Name `mode` in the properties of a window that is not mapped. A window
    /// manager opens a window in the mode its properties name, and ignores the
    /// client message winit sends for a window it does not manage yet, so a
    /// mode asked for before the window is first shown would be lost.
    #[cfg(target_os = "linux")]
    fn seed_unmapped_mode(&self, mode: WindowMode) {
        let unmapped = *self.reveal_pending.lock() || !*self.is_visible.lock();
        if unmapped && let Some((probe, id)) = self.x11.get() {
            probe.set_mode_hint(*id, mode);
        }
    }

    /// Give a window that opens at a remembered outer rectangle, unmapped, the
    /// content size that makes its outer size that. The first window of a run
    /// has no frame measured yet, so the window manager is asked for the one
    /// it will add (`_NET_REQUEST_FRAME_EXTENTS`), waiting briefly for the
    /// answer: without it, un-maximizing the window would leave it larger than
    /// it was saved by the frame, and saving that again grows it every run.
    #[cfg_attr(
        not(target_os = "linux"),
        expect(
            clippy::unused_self,
            reason = "only an X11 window can ask for its frame"
        )
    )]
    fn fit_to_remembered_rectangle(&self) {
        #[cfg(target_os = "linux")]
        {
            // Once: the window is fitted when it is first mapped, not each time
            // it is shown again.
            let Some(outer) = self.outer_to_fit.lock().take() else {
                return;
            };
            if self.frame_inset.is_known() || !self.window.is_decorated() {
                return;
            }
            let Some((probe, id)) = self.x11.get() else {
                return;
            };
            let Some(frame) = probe.frame_extents(*id, FRAME_EXTENTS_WAIT) else {
                return;
            };
            self.frame_inset.record(frame);
            let inset = self.frame_inset.logical(self.window.scale_factor());
            let _ = self.window.request_inner_size(LogicalSize::new(
                (outer.width - inset.width).max(1.0),
                (outer.height - inset.height).max(1.0),
            ));
        }
    }

    /// Make the window visible, ending a wait for the first frame. Every change
    /// of the native window's visibility is made holding `reveal_pending`, so a
    /// reveal and a hide from two threads cannot interleave their halves. The
    /// caller publishes the state and reports the visibility once its own
    /// native work is done, so a callback that hides or minimizes the window
    /// again is not undone by the rest of a show.
    fn make_visible(&self) {
        let mut pending = self.reveal_pending.lock();
        if *pending || !*self.is_visible.lock() {
            // An occlusion belongs to the mapping that ended; X reports the
            // new one's. A window that stays mapped keeps the one it has.
            self.occluded.store(false, Ordering::Release);
        }
        *pending = false;
        self.map_window();
        *self.is_visible.lock() = true;
    }

    /// Map the window, which is unmapped and holds `reveal_pending`: fitted to
    /// the rectangle it was opened with, and minimized if that is what it was
    /// last asked to be, which a window manager only honours for a window it
    /// manages.
    fn map_window(&self) {
        self.fit_to_remembered_rectangle();
        self.window.set_visible(true);
        if self.minimize_on_map.swap(false, Ordering::AcqRel) {
            self.window.set_minimized(true);
        }
    }

    /// Hide the window. A hidden window has no first frame to wait for.
    /// Like [`Self::make_visible`], the caller publishes and reports.
    fn make_hidden(&self) {
        let mut pending = self.reveal_pending.lock();
        *pending = false;
        self.window.set_visible(false);
        *self.is_visible.lock() = false;
    }
}

/// What the window's snapshot holds that winit cannot read back.
struct RecordedState<'a> {
    title: &'a str,
    constraints: SizeConstraints,
    focused: bool,
    visible: bool,
    display: Option<DisplayId>,
    /// The mode the window system reports directly, where winit's flags fall
    /// short of it (X11 minimization).
    probed_mode: Option<WindowMode>,
    /// The facts beyond geometry; their capabilities are what the window
    /// honours now (an X11 window whose minimization goes unseen says so).
    reported: ReportedFacts,
}

/// What a winit window reports beyond its geometry. winit has no safe area
/// and never suspends a desktop window, and its focus is the activation (it
/// reports no other).
fn reported_facts(
    window: &Window,
    appearance: WindowAppearance,
    focused: bool,
    hovered: bool,
    capabilities: WindowCapabilities,
) -> ReportedFacts {
    ReportedFacts {
        metrics: WindowMetrics::new(device_size(window.inner_size()), window.scale_factor()).ok(),
        appearance,
        safe_area: flui_foundation::geometry::EdgeInsets::ZERO,
        execution: flui_platform::WindowExecutionState::Running,
        hovered,
        active: focused,
        capabilities,
    }
}

/// The user's theme as applied to `window`: winit's live theme where it has
/// one (Windows, macOS, a Wayland override), on Linux otherwise the Settings
/// portal's color-scheme (`portal_dark`).
fn appearance_of(window: &Window, portal_dark: bool) -> WindowAppearance {
    let dark = match window.theme() {
        Some(theme) => theme == winit::window::Theme::Dark,
        None => portal_dark,
    };
    if dark {
        WindowAppearance::Dark
    } else {
        WindowAppearance::Light
    }
}

/// A device-pixel extent as the signed one the frame measurement takes.
fn device_size(size: winit::dpi::PhysicalSize<u32>) -> Size<i32> {
    let clamp = |extent: u32| i32::try_from(extent).unwrap_or(i32::MAX);
    Size::new(clamp(size.width), clamp(size.height))
}

/// The window as winit and `facts` describe it.
fn sample_state(window: &Window, facts: &RecordedState<'_>) -> WindowState {
    let capabilities = facts.reported.capabilities;
    let scale = window.scale_factor();
    let logical = |size: winit::dpi::PhysicalSize<u32>| {
        Size::new(
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        )
    };
    let position = capabilities
        .observes_position
        .is_available()
        .then(|| window.outer_position().ok())
        .flatten()
        .map(|at| Point::new(f64::from(at.x) / scale, f64::from(at.y) / scale));
    let minimized =
        capabilities.observes_minimized.is_available() && window.is_minimized() == Some(true);
    let state = WindowState::new(
        WindowGeometry::new(position, logical(window.outer_size())),
        logical(window.inner_size()),
    )
    .with_mode(facts.probed_mode.unwrap_or_else(|| {
        mode_of(ModeFlags {
            minimized,
            fullscreen: window.fullscreen().is_some(),
            maximized: window.is_maximized(),
        })
    }))
    .with_visible(facts.visible)
    .with_focused(facts.focused)
    .with_title(Some(facts.title.to_owned()))
    .with_constraints(facts.constraints)
    .with_resizable(window.is_resizable())
    .with_decorated(window.is_decorated())
    .with_display(facts.display);
    facts.reported.onto(state)
}

impl Drop for WinitWindow {
    fn drop(&mut self) {
        // Teardown order: the window's handler may own this window's GPU
        // renderer, whose `wgpu::Surface` is built from the window's surface
        // target, which holds the winit window. Letting go of the handler
        // here drops the renderer, and with it the surface and the target,
        // while the winit window is still alive (on Wayland, destroying the
        // swapchain after the `wl_surface` uses the surface's freed
        // `wl_proxy`). `Drop::drop` runs before any field is dropped, so this
        // holds whatever the field order. The winit `CloseRequested` arm also
        // clears eagerly at close (the in-loop path); this is the guarantee
        // for a window whose last handle drops anywhere else.
        self.delivery.clear();
    }
}

/// A winit window's surface target (ADR-0254 §5).
///
/// It holds the winit window, which is `Send + Sync`, so the handles it
/// hands out stay backed while the renderer that owns the target lives; the
/// window's handler owns that renderer and lets go of it when the window
/// closes. Once the window's retirement started (its close completed, or the
/// loop released it at quit), both handle queries answer `Unavailable`.
struct WinitSurface {
    window: Arc<Window>,
    retirement: crate::owner::window_delivery::Retirement,
}

impl HasWindowHandle for WinitSurface {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        if self.retirement.is_retired() {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        self.window.window_handle()
    }
}

impl HasDisplayHandle for WinitSurface {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        if self.retirement.is_retired() {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        self.window.display_handle()
    }
}

impl flui_platform::SurfaceTarget for WinitSurface {
    /// On Wayland this arms the surface's frame callback, so the next
    /// `request_redraw` waits for the compositor; winit makes it nothing
    /// elsewhere.
    fn pre_present_notify(&self) {
        if !self.retirement.is_retired() {
            self.window.pre_present_notify();
        }
    }
}

/// A winit window's redraw requester: winit takes a redraw request from any
/// thread. It holds the winit window weakly, so a requester kept by a waker
/// never keeps a closed window on screen.
struct WinitRedraw {
    window: std::sync::Weak<Window>,
    retirement: crate::owner::window_delivery::Retirement,
}

impl flui_platform::RedrawRequester for WinitRedraw {
    fn request_redraw(&self) {
        if !self.retirement.is_retired()
            && let Some(window) = self.window.upgrade()
        {
            window.request_redraw();
        }
    }
}

impl PlatformWindow for WinitWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn request_redraw(&self) {
        if !self.delivery.retirement().is_retired() {
            self.window.request_redraw();
        }
    }

    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        Arc::new(WinitRedraw {
            window: Arc::downgrade(&self.window),
            retirement: self.delivery.retirement(),
        })
    }

    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        Arc::new(WinitSurface {
            window: Arc::clone(&self.window),
            retirement: self.delivery.retirement(),
        })
    }

    fn state(&self) -> WindowState {
        let published = self.window_state.state_with(|| self.sample());
        if published.changed {
            self.delivery.dispatch_state_changed();
        }
        published.state
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(ApplyError::Closed);
        }
        let command = command.against(&self.window_state.published_capabilities())?;
        match command {
            WindowCommand::SetTitle(title) => {
                self.window.set_title(&title);
                *self.title.lock() = title;
            }
            WindowCommand::SetPosition(at) => self
                .window
                .set_outer_position(LogicalPosition::new(at.x, at.y)),
            WindowCommand::SetSize(size) => {
                // A maximized or fullscreen window has no size of its own to set:
                // the window system either ignores the request or restores the
                // window to honour it, and which differs between them. So the
                // command is the second, everywhere: the window returns to
                // normal mode at the size asked for.
                if self.window.fullscreen().is_some() {
                    self.window.set_fullscreen(None);
                }
                if self.window.is_maximized() {
                    self.window.set_maximized(false);
                }
                // A window that cannot be resized yet answers later, through
                // `Resized`; the request itself is all this reports.
                let _ = self
                    .window
                    .request_inner_size(LogicalSize::new(size.width, size.height));
            }
            WindowCommand::SetConstraints(constraints) => {
                let limit = |size: Size<f64>| LogicalSize::new(size.width, size.height);
                self.window.set_min_inner_size(constraints.min.map(limit));
                self.window.set_max_inner_size(constraints.max.map(limit));
                *self.constraints.lock() = constraints;
            }
            WindowCommand::SetResizable(resizable) => self.window.set_resizable(resizable),
            WindowCommand::SetDecorated(decorated) => self.window.set_decorations(decorated),
            WindowCommand::SetMode(mode) => self.enter_mode(mode)?,
            // `admit` refused this where winit cannot unminimize or focus
            // (Wayland).
            WindowCommand::Show => {
                self.make_visible();
                self.window.set_minimized(false);
                self.window.focus_window();
            }
            WindowCommand::Hide => self.make_hidden(),
            WindowCommand::Activate => self.window.focus_window(),
            WindowCommand::Close => {
                self.close_window();
                return Ok(());
            }
            // `admit` refuses a command this window does not know.
            other => return Err(crate::window::state::not_carried_out(&other)),
        }
        // After the command's native work.
        self.refresh_state();
        Ok(())
    }

    fn reveal_after_first_frame(&self) {
        {
            let mut pending = self.reveal_pending.lock();
            if self.closed.load(Ordering::Acquire) || !std::mem::take(&mut *pending) {
                return;
            }
            self.map_window();
        }
        self.refresh_state();
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.window.set_cursor(cursor);
        Ok(())
    }

    /// winit takes no refresh-rate request. Neither do Wayland, X11 nor Win32
    /// windows; AppKit does from macOS 14 (`NSView.displayLink`), so there the
    /// reason is the platform's answer for this macOS.
    fn set_frame_rate_hint(
        &self,
        _hint: flui_platform::FrameRateHint,
    ) -> Result<(), flui_platform::Unsupported> {
        #[cfg(target_os = "macos")]
        let absence = crate::window::capabilities::frame_rate_absence(self.displays.version());
        #[cfg(not(target_os = "macos"))]
        let absence = flui_platform::CapabilityAbsence::NotOnThisPlatform;
        Err(absence.into())
    }

    /// The input regions are the system's edge gestures a region keeps out
    /// (`InputRegionKind::SystemGestureExclusion`), and no desktop window
    /// system winit runs on lets a window exclude them.
    fn set_input_regions(
        &self,
        _regions: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), flui_platform::Unsupported> {
        Err(flui_platform::Unsupported::new(
            flui_platform::UnsupportedReason::NotOnThisPlatform,
        ))
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::ime)
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::clone(&self.facets.haptics))
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    /// winit starts no OS drag session; its input capabilities say so.
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }
}

impl crate::host_window::CoreHostWindow for WinitWindow {
    fn window_delivery(&self) -> &WindowDelivery {
        &self.delivery
    }
}

impl HostWindow for WinitWindow {
    /// The window's own bridge — the capability the composition root's
    /// accessibility wire discovers.
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        Some(Arc::clone(&self.facets.accessibility) as _)
    }

    fn capture_text_sizing(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        self.facets.text.capture_sizing()
    }

    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<std::rc::Rc<dyn flui_platform::text_store::TextStoreHost>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::store_host)
    }
}

impl LoopWindow for WinitWindow {
    /// Map a window created hidden to open in a mode, now that the mode is set
    /// where the window manager reads it. A no-op for every other window.
    fn finish_opening(&self) {
        if !self.map_after_setup {
            return;
        }
        {
            let mut pending = self.reveal_pending.lock();
            *pending = false;
            self.map_window();
        }
        self.refresh_state();
    }

    fn inner(&self) -> &Arc<Window> {
        &self.window
    }

    /// The window may differ from its last snapshot: read it again, and raise
    /// `WindowHandler::state_changed` when the revision grew.
    fn refresh_state(&self) {
        self.window_state.invalidate();
        let _ = PlatformWindow::state(self);
    }

    fn set_focused(&self, focused: bool) {
        WinitWindow::set_focused(self, focused);
    }

    /// Record winit's `Occluded`. Kept apart from whether the window is shown,
    /// so an occlusion event that arrives after a hide cannot show it.
    fn set_occluded(&self, occluded: bool) {
        self.occluded.store(occluded, Ordering::Release);
    }
    fn set_visible(&self, visible: bool) {
        WinitWindow::set_visible(self, visible);
    }

    /// Whether the window's display differs from the last time this was
    /// asked, remembering the answer. The window event handler uses it to read
    /// the window again only on a real move.
    fn take_display_change(&self) -> bool {
        let current = self.display();
        let mut reported = self.reported_display.lock();
        let changed = *reported != current;
        *reported = current;
        changed
    }

    /// Whether no display has been reported for this window yet (or the last
    /// one reported was "none"). A Wayland window is on no output until its
    /// first frame enters one, and winit reports no event when that happens, so
    /// the event loop looks again each time it goes idle while this holds.
    #[cfg(target_os = "linux")]
    fn awaiting_display(&self) -> bool {
        self.reported_display.lock().is_none()
    }

    /// Record the portal's `color-scheme`; `true` when it changed what
    /// the snapshot's appearance reports.
    #[cfg(target_os = "linux")]
    fn observe_portal_appearance(&self, appearance: WindowAppearance) -> bool {
        let dark = flui_platform::Brightness::from(appearance).is_dark();
        let previous = self
            .portal_dark
            .swap(dark, std::sync::atomic::Ordering::Relaxed);
        previous != dark && self.window.theme().is_none()
    }

    /// The X11 id of this window, when a probe is attached.
    #[cfg(target_os = "linux")]
    fn x11_id(&self) -> Option<u32> {
        self.x11.get().map(|(_, id)| *id)
    }

    fn mark_closed(&self) {
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn shared_delivery(&self) -> &Arc<WindowDelivery> {
        &self.delivery
    }
}

/// Connects the view of a window manager that wakes `signal` when a window it
/// watches changes: the host hands [`WinitWindows`] the X11 probe's.
#[cfg(target_os = "linux")]
pub(crate) type ConnectWindowManager =
    fn(
        &Arc<crate::owner::signal::OwnerSignal>,
    ) -> Result<Arc<dyn WindowManagerView>, crate::window::WindowManagerError>;

/// What the winit host's windows share and open with: the frame measured
/// around decorated windows, so a remembered outer size can be fitted before
/// the next window exists, and on Linux the view of the window manager that
/// reads what winit cannot of an X11 window.
///
/// The host builds it and hands it to the loop, which holds it as
/// [`LoopWindows`]. Public where it is defined, as the host's table names it,
/// and nameable only inside the crate.
pub struct WinitWindows {
    frame_inset: Arc<FrameInset>,
    /// What every window hands out of the window-level positions.
    positions: WindowPositions<Arc<Window>>,
    /// Connects the window manager's view, with the first X11 window.
    #[cfg(target_os = "linux")]
    connect_window_manager: ConnectWindowManager,
    /// The window manager's view, started with the first X11 window; `None`
    /// inside when the server cannot be reached a second time.
    #[cfg(target_os = "linux")]
    pub(crate) window_manager: std::sync::OnceLock<Option<Arc<dyn WindowManagerView>>>,
}

impl std::fmt::Debug for WinitWindows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinitWindows").finish_non_exhaustive()
    }
}

impl WinitWindows {
    /// What the winit host's windows open with: each hands out `positions`.
    /// On Linux, `connect_window_manager` connects the view of the window
    /// manager when the first X11 window opens.
    pub(crate) fn new(
        positions: WindowPositions<Arc<Window>>,
        #[cfg(target_os = "linux")] connect_window_manager: ConnectWindowManager,
    ) -> Self {
        Self {
            frame_inset: Arc::new(FrameInset::default()),
            positions,
            #[cfg(target_os = "linux")]
            connect_window_manager,
            #[cfg(target_os = "linux")]
            window_manager: std::sync::OnceLock::new(),
        }
    }

    /// The shell windows opened from here will be on, before any exists.
    fn expected_shell(owner: &WinitLoop) -> WindowShell {
        #[cfg(target_os = "linux")]
        if owner.displays.is_wayland() {
            return WindowShell::Wayland;
        }
        #[cfg(not(target_os = "linux"))]
        let _ = owner;
        WindowShell::Desktop
    }

    /// The window manager's view, started on first use with the owner's
    /// `signal`. `None` when the platform is not running, or the X server
    /// cannot be reached again; windows then report what winit does.
    #[cfg(target_os = "linux")]
    fn window_manager(
        &self,
        signal: Option<&Arc<crate::owner::signal::OwnerSignal>>,
    ) -> Option<Arc<dyn WindowManagerView>> {
        let signal = signal?;
        self.window_manager
            .get_or_init(|| match (self.connect_window_manager)(signal) {
                Ok(view) => Some(view),
                Err(error) => {
                    tracing::debug!(%error, "window state is read from winit alone");
                    None
                }
            })
            .clone()
    }
}

impl LoopWindows for WinitWindows {
    fn build(
        &self,
        event_loop: &ActiveEventLoop,
        seed: WindowSeed<'_>,
        options: WindowOptions,
    ) -> Result<Rc<dyn LoopWindow>, OpenWindowError> {
        let display =
            PlacementDisplay::choose(seed.displays.snapshot().ok().as_ref(), options.display);
        let plan = OpenPlan::new(
            &options,
            shell_of(event_loop),
            display,
            self.frame_inset.logical(display.scale_factor),
        );
        let mut attributes = WindowAttributes::default()
            .with_title(options.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                plan.content_size.width,
                plan.content_size.height,
            ))
            .with_resizable(options.resizable)
            .with_decorations(options.decorated)
            .with_visible(options.visible && !plan.created_unmapped())
            .with_maximized(plan.mode == WindowMode::Maximized);
        if plan.mode == WindowMode::Fullscreen {
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        if let Some(at) = plan.position {
            attributes = attributes.with_position(winit::dpi::PhysicalPosition::new(at.x, at.y));
        }

        if let Some(min) = options.min_size {
            attributes =
                attributes.with_min_inner_size(winit::dpi::LogicalSize::new(min.width, min.height));
        }
        if let Some(max) = options.max_size {
            attributes =
                attributes.with_max_inner_size(winit::dpi::LogicalSize::new(max.width, max.height));
        }

        let raw_window = Arc::new(event_loop.create_window(attributes).map_err(|error| {
            OpenWindowError::Backend {
                message: format!("winit window creation failed: {error}"),
            }
        })?);
        let notices = seed.notices;
        #[cfg(target_os = "linux")]
        let signal = seed.signal;
        let window = Rc::new(WinitWindow::new(seed, raw_window, &options, &plan, self));
        notices.mark_stale_on_wake(window.state_cell());
        window
            .delivery
            .publish_through_owner(&window, WinitWindow::read_state);
        // Before the window is mapped: the window manager reads the mode a
        // window opens in from the properties the view writes.
        #[cfg(target_os = "linux")]
        window.attach_x11_probe(|| self.window_manager(signal));
        Ok(window)
    }

    /// The windows whose mode properties the X11 window manager changed.
    #[cfg(target_os = "linux")]
    fn take_unreported(&self) -> Vec<u32> {
        match self.window_manager.get() {
            Some(Some(view)) => view.take_changed(),
            _ => Vec::new(),
        }
    }
}

impl host::Window<WinitLoop> for WinitWindow {
    type Shared = WinitWindows;
    /// A window by the winit window it wraps.
    type Ref = Arc<Window>;

    #[expect(
        unsafe_code,
        reason = "the same-thread fast path dereferences the event loop the owner published"
    )]
    fn open(
        owner: &WinitLoop,
        _: &Arc<WinitWindows>,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        tracing::info!(?options, "Requesting window creation");

        // Same-thread fast path: called synchronously from inside `on_ready`
        // (see `WinitApp::resumed`), where a live `ActiveEventLoop` is
        // published for exactly this nested call. Create the window
        // directly instead of enqueuing — the lane's `user_event` consumer
        // cannot run until this call returns, and would
        // deadlock forever otherwise (see `owner/winit.rs`'s module doc).
        if let Some(event_loop_ptr) = ACTIVE_EVENT_LOOP.with(Cell::get) {
            tracing::debug!("Creating window on event-loop thread (same-thread fast path)");
            // SAFETY: `event_loop_ptr` is non-null only while
            // `with_active_event_loop` has published it on THIS thread, for
            // the exact nested `on_ready` call currently executing (see
            // `WinitApp::resumed`). The referent is `resumed`'s live
            // `&ActiveEventLoop` parameter, which outlives that whole call —
            // and this call is strictly nested inside it — so the reference
            // constructed here is valid for the borrow's entire duration.
            let event_loop = unsafe { event_loop_ptr.as_ref() };
            let window_id = owner.create_window_now(event_loop, options, handler)?;
            return owner.window_by_id(window_id);
        }

        let control = match owner.with_state(|state| {
            state
                .run_state
                .control_for_open_window(thread::current().id())
        }) {
            Ok(control) => control,
            // The loop has stopped: the owner is genuinely gone, and the
            // untouched options ride back so the caller can retry against a
            // fresh platform without rebuilding them.
            Err(OpenWindowStateError::Stopped) => {
                return Err(OpenWindowError::OwnerGone {
                    rejected: Some(Box::new(options)),
                });
            }
            // Lifecycle refusals: the loop is not (yet) in a state that can
            // service this call site — typed as `Unavailable`, distinct
            // from a dead owner.
            Err(state_error) => {
                return Err(OpenWindowError::Unavailable {
                    message: match state_error {
                        OpenWindowStateError::NotRunning => {
                            "open_window called before Platform::run — on the winit backend, \
                             create the first windows inside run()'s on_ready callback"
                        }
                        OpenWindowStateError::Starting => {
                            "open_window called from another thread before winit finished \
                             on_ready"
                        }
                        OpenWindowStateError::OwnerWouldBlock => {
                            "open_window cannot block the winit event-loop owner outside \
                             on_ready"
                        }
                        OpenWindowStateError::Stopped => unreachable!("BUG: handled above"),
                    }
                    .to_string(),
                });
            }
        };

        // Another thread, while the loop runs: the handler was built here
        // and lives here, and the owner would call it. That thread opens a
        // window through `PlatformProxy::open_window`, whose request builds
        // the handler on the owner.
        drop(control);
        drop(handler);
        Err(OpenWindowError::Unavailable {
            message: "open_window called off the winit event-loop thread: a window's handler \
                      lives on the owner thread, so another thread opens a window through \
                      PlatformProxy::open_window"
                .to_owned(),
        })
    }

    fn active(owner: &WinitLoop, _: &WinitWindows) -> Option<WindowId> {
        owner.with_state(|state| state.active_window)
    }

    fn capabilities(owner: &WinitLoop, _: &WinitWindows) -> WindowCapabilities {
        WinitWindows::expected_shell(owner).capabilities()
    }

    fn frame_inset(_: &WinitLoop, windows: &WinitWindows, scale_factor: f64) -> Size<f64> {
        windows.frame_inset.logical(scale_factor)
    }
}

// The window's tests on a real winit event loop, which open, unwind and close
// windows through the loop.
#[cfg(test)]
mod real_loop_tests;
