//! The winit window: [`WinitWindow`], the backend's [`PlatformWindow`], a thin
//! wrapper over `Arc<winit::window::Window>` carrying the platform-minted
//! [`WindowId`], the per-window callback table, and the route its
//! programmatic `close()` takes back to the event-loop owner
//! (`ControlSender`, crate-private); [`plan`], what the backend decides about
//! a window before and while it exists; and [`WinitWindows`], which opens the
//! windows the loop tracks (`owner::winit::LoopWindows`).

use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use cursor_icon::CursorIcon;
use flui_foundation::ClaimOutcome;
use flui_foundation::geometry::{Point, Size};
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
use crate::owner::call_point::install;
use crate::owner::handlers::CallbackWindow;
use crate::owner::winit::HostDisplays;
use crate::owner::winit::control::{ControlSendError, ControlSender};
use crate::owner::winit::{
    ACTIVE_EVENT_LOOP, LoopWindow, LoopWindows, OpenWindowStateError, WindowSeed, WinitLoop,
};
use crate::text::WindowTextInput;
#[cfg(target_os = "linux")]
use crate::window::WindowManagerView;
use crate::window::state::{ModeFlags, WindowStateCell, mode_of};
use crate::window::visibility::ReportedVisibility;
use flui_platform::{
    CursorError, DisplayId, PlatformTextInput, PlatformWindow, SizeConstraints, WindowAppearance,
    WindowCapabilities, WindowCommand, WindowCommandError, WindowEvent, WindowGeometry, WindowId,
    WindowMode, WindowOptions, WindowState,
};
use flui_semantics::platform::PlatformAccessibility;
use plan::{FrameInset, OpenPlan, PlacementDisplay, WindowShell};

/// How long a window that opens unmapped waits for the window manager to say
/// what frame it will have.
#[cfg(target_os = "linux")]
const FRAME_EXTENTS_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// Concrete winit window wrapper
///
/// Wraps `winit::window::Window` to implement `PlatformWindow`.
/// Includes per-window callbacks for event delivery using the causal FIFO
/// dispatch pattern for reentrancy safety.
pub struct WinitWindow {
    id: WindowId,
    closed: AtomicBool,
    window: Arc<Window>,
    is_focused: parking_lot::Mutex<bool>,
    is_visible: parking_lot::Mutex<bool>,
    callbacks: crate::owner::handlers::WindowCallbacks,
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
    /// The route a programmatic [`PlatformWindow::close`] takes to the
    /// event-loop owner (issue #919). The close teardown needs the live
    /// `ActiveEventLoop` (to end the loop when this was the last window),
    /// which only an owner-turn callback holds — so `close()` cannot run it
    /// in place; it posts a non-droppable per-window request and the owner
    /// runs the exact teardown the compositor's `CloseRequested` arm runs.
    close_lane: ControlSender,
    /// The platform's native display source, by which this window names its
    /// display.
    displays: Arc<dyn HostDisplays>,
    /// The display last reported through `on_display_changed`, so a hint fires
    /// when the window really moved.
    reported_display: parking_lot::Mutex<Option<DisplayId>>,
    /// The snapshot last published (ADR-0237). Shared with the platform so a
    /// wake from sleep marks it stale.
    window_state: Arc<WindowStateCell>,
    /// What the window system lets a program do to this window.
    shell: WindowShell,
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
    /// Whether the last published snapshot of this X11 window was minimized.
    /// A window manager minimizes by unmapping, and X sends no visibility
    /// event for a window that becomes unviewable, so winit raises no
    /// `Occluded`: the window is hidden from the published mode instead.
    #[cfg(target_os = "linux")]
    wm_minimized: AtomicBool,
    /// Whether winit last reported the window fully covered (`Occluded`).
    occluded: AtomicBool,
    /// What `on_visibility_status_change` last reported (at first, what the
    /// window opened as): a report is raised only when it changes.
    reported_visible: parking_lot::Mutex<ReportedVisibility>,
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
        // `WindowCallbacks` holds boxed closures that don't implement
        // `Debug`; print the focus/visibility flags only.
        f.debug_struct("WinitWindow")
            .field("is_focused", &*self.is_focused.lock())
            .field("is_visible", &*self.is_visible.lock())
            .finish_non_exhaustive()
    }
}

impl WinitWindow {
    /// Create a new WinitWindow wrapper, addressed by the platform-minted
    /// `id` the caller already allocated for it, with the owner lane its
    /// programmatic `close()` reports to. Backend-private: only this
    /// backend's own window creation can supply that lane, so nothing
    /// outside it can construct a window whose close would go nowhere.
    ///
    /// `plan` is what the window was created from, so its snapshot starts from
    /// the mode it opened in and, if it opened maximized, the rectangle it
    /// returns to; `windows` holds the frame inset it reports and the
    /// positions that make what it hands out.
    pub(crate) fn new(
        id: WindowId,
        window: Arc<Window>,
        close_lane: ControlSender,
        displays: Arc<dyn HostDisplays>,
        options: &WindowOptions,
        plan: &OpenPlan,
        windows: &WinitWindows,
    ) -> Self {
        let positions = &windows.positions;
        let shell = shell_of(&window);
        let facets = positions.for_window(&window);
        let capabilities = shell.capabilities();
        let constraints = SizeConstraints::new(options.min_size, options.max_size);
        let title = options.title.clone();
        let display = displays.id_of_window(&window);
        let initial = sample_state(
            &window,
            shell,
            &RecordedState {
                title: &title,
                constraints,
                focused: true,
                visible: options.visible,
                display,
                probed_mode: None,
            },
        );
        Self {
            id,
            closed: AtomicBool::new(false),
            window_state: Arc::new(WindowStateCell::new(initial)),
            shell,
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
            #[cfg(target_os = "linux")]
            wm_minimized: AtomicBool::new(false),
            window,
            is_focused: parking_lot::Mutex::new(true),
            is_visible: parking_lot::Mutex::new(options.visible),
            occluded: AtomicBool::new(false),
            reported_visible: parking_lot::Mutex::new(ReportedVisibility::new(options.visible)),
            callbacks: crate::owner::handlers::WindowCallbacks::new(),
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

    /// Get a reference to the per-window callbacks
    pub fn callbacks(&self) -> &crate::owner::handlers::WindowCallbacks {
        &self.callbacks
    }

    /// Update focus state
    pub fn set_focused(&self, focused: bool) {
        *self.is_focused.lock() = focused;
    }

    /// Update visibility state
    pub fn set_visible(&self, visible: bool) {
        *self.is_visible.lock() = visible;
    }

    /// The window as the operating system describes it now.
    ///
    /// Reads winit with no lock of the snapshot held, and keeps the rectangle
    /// a maximized or fullscreen window returns to: the outer geometry the
    /// window had the last time it was in normal mode.
    fn sample(&self) -> WindowState {
        let title = self.title.lock().clone();
        let mut state = sample_state(
            &self.window,
            self.shell,
            &RecordedState {
                title: &title,
                constraints: *self.constraints.lock(),
                focused: *self.is_focused.lock(),
                visible: *self.is_visible.lock() && !self.occluded.load(Ordering::Acquire),
                display: PlatformWindow::display_id(self),
                probed_mode: self.probed_mode(),
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

    /// Follow a published snapshot of an X11 window into or out of the
    /// minimized mode (see `wm_minimized`).
    #[cfg(target_os = "linux")]
    fn follow_wm_minimized(&self, state: &WindowState) {
        if self.x11.get().is_none() {
            return;
        }
        let minimized = state.mode == WindowMode::Minimized;
        if self.wm_minimized.swap(minimized, Ordering::AcqRel) != minimized {
            self.report_visibility();
        }
    }

    /// Enter `mode`. Leaving fullscreen and minimized first keeps entering a
    /// mode from depending on what the window manager makes of two requests at
    /// once: it keeps a minimized window minimized under a maximize.
    fn enter_mode(&self, mode: WindowMode) -> Result<(), WindowCommandError> {
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
            _ => {
                return Err(flui_platform::Unsupported::new(
                    flui_platform::UnsupportedReason::NotOnThisPlatform,
                )
                .into());
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
}

/// A device-pixel extent as the signed one the frame measurement takes.
fn device_size(size: winit::dpi::PhysicalSize<u32>) -> Size<i32> {
    let clamp = |extent: u32| i32::try_from(extent).unwrap_or(i32::MAX);
    Size::new(clamp(size.width), clamp(size.height))
}

/// The window as winit and `facts` describe it.
fn sample_state(window: &Window, shell: WindowShell, facts: &RecordedState<'_>) -> WindowState {
    let capabilities = shell.capabilities();
    let scale = window.scale_factor();
    let logical = |size: winit::dpi::PhysicalSize<u32>| {
        Size::new(
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        )
    };
    let position = capabilities
        .observes_position
        .then(|| window.outer_position().ok())
        .flatten()
        .map(|at| Point::new(f64::from(at.x) / scale, f64::from(at.y) / scale));
    let minimized = capabilities.observes_minimized && window.is_minimized() == Some(true);
    WindowState::new(
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
    .with_display(facts.display)
}

impl Drop for WinitWindow {
    fn drop(&mut self) {
        // Teardown-order invariant: a registered callback may own this
        // window's GPU renderer, whose `wgpu::Surface` is built from the
        // `Arc<dyn PlatformWindow>` clone the renderer owns (ADR-0063) —
        // that surface must be destroyed while the native window behind
        // that clone is still alive (on Wayland, destroying the swapchain
        // after the `wl_surface` is a use-after-free on the surface's
        // `wl_proxy`; observed as the post-quit SIGSEGV of issue #713).
        // `Drop::drop` runs before any field is dropped, so clearing the
        // callbacks here guarantees the renderer dies before `self.window`
        // regardless of field order.
        // The winit `CloseRequested` arm also clears eagerly at close (the
        // primary, in-loop path); this is the last-resort guarantee for a
        // window whose final `Arc` unwinds anywhere else.
        self.callbacks.clear();
    }
}

impl PlatformWindow for WinitWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<i32> {
        let size = self.window.inner_size();
        Size::new(size.width as i32, size.height as i32)
    }

    fn logical_size(&self) -> Size<f64> {
        let size = self.window.inner_size();
        let scale = self.window.scale_factor();
        Size::new(size.width as f64 / scale, size.height as f64 / scale)
    }

    fn appearance(&self) -> WindowAppearance {
        // winit's live theme where it has one (Windows, macOS, a Wayland
        // override); on Linux otherwise the Settings portal's color-scheme.
        match self.window.theme() {
            Some(winit::window::Theme::Dark) => WindowAppearance::Dark,
            Some(winit::window::Theme::Light) => WindowAppearance::Light,
            #[cfg(target_os = "linux")]
            None if self.portal_dark.load(std::sync::atomic::Ordering::Relaxed) => {
                WindowAppearance::Dark
            }
            None => WindowAppearance::Light,
        }
    }

    fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }

    fn display_id(&self) -> Option<DisplayId> {
        self.displays.id_of_window(&self.window)
    }

    fn pre_present_notify(&self) {
        // The Wayland frame-callback arm (see the trait doc); harmless
        // elsewhere. winit documents this as "call before presenting".
        self.window.pre_present_notify();
    }

    fn is_focused(&self) -> bool {
        *self.is_focused.lock()
    }

    fn is_visible(&self) -> bool {
        #[cfg(target_os = "linux")]
        if self.wm_minimized.load(Ordering::Acquire) {
            return false;
        }
        *self.is_visible.lock() && !self.occluded.load(Ordering::Acquire)
    }

    fn state(&self) -> WindowState {
        let published = self.window_state.state_with(|| self.sample());
        if published.changed {
            #[cfg(target_os = "linux")]
            self.follow_wm_minimized(&published.state);
            self.callbacks.dispatch_state_changed();
        }
        published.state
    }

    fn capabilities(&self) -> WindowCapabilities {
        #[cfg(target_os = "linux")]
        if self.minimize_unobserved.load(Ordering::Acquire) {
            let mut capabilities = self.capabilities;
            capabilities.observes_minimized = false;
            return capabilities;
        }
        self.capabilities
    }

    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(WindowCommandError::Closed);
        }
        command.admit(&self.capabilities)?;
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
            WindowCommand::Show => self.show().map_err(|error| match error {
                flui_platform::WindowShowError::Closed => WindowCommandError::Closed,
                flui_platform::WindowShowError::Native { message } => {
                    WindowCommandError::Backend { message }
                }
                flui_platform::WindowShowError::Unsupported(unsupported) => unsupported.into(),
                other => WindowCommandError::Backend {
                    message: other.to_string(),
                },
            })?,
            WindowCommand::Hide => self.make_hidden(),
            WindowCommand::Activate => self.window.focus_window(),
            // `admit` refuses a command this window does not know.
            _ => {
                return Err(flui_platform::Unsupported::new(
                    flui_platform::UnsupportedReason::NotOnThisPlatform,
                )
                .into());
            }
        }
        // The snapshot first, so a visibility report never runs ahead of
        // `state()`; both after the command's native work.
        self.refresh_state();
        self.report_visibility();
        Ok(())
    }

    fn show(&self) -> Result<(), flui_platform::WindowShowError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(flui_platform::WindowShowError::Closed);
        }
        // Winit cannot unminimize or focus Wayland windows.
        if !self.capabilities.shows {
            return Err(flui_platform::Unsupported::new(
                flui_platform::UnsupportedReason::NotOnThisPlatform,
            )
            .into());
        }
        self.make_visible();
        self.window.set_minimized(false);
        self.window.focus_window();
        self.refresh_state();
        self.report_visibility();
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

    fn close(&self) {
        // Hide now, from whichever thread this is called on: the visible
        // effect of a close should not wait for the owner's next turn. The
        // close itself — `on_close`, map removal, callback clear, exit
        // policy — is the owner's (see `close_lane`): running `on_close`
        // here would fire it on the caller's thread, where an embedder's
        // owner-affine close handling (`flui-app` rejects ui_runtime dispatch
        // off its owner thread) would silently refuse it, and skipping the
        // map removal is precisely issue #919 — a hidden window that the
        // exit policy still counts, so the process never exits.
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

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.window.set_cursor(cursor);
        Ok(())
    }

    crate::owner::handlers::impl_window_callback_setters!(callbacks);

    // GPU integration: `winit::window::Window` implements `HasWindowHandle`/
    // `HasDisplayHandle` directly — without these overrides both fall through
    // to the trait defaults (`Err(HandleError::Unavailable)`), which is what
    // made every wgpu surface creation on this backend fail regardless of
    // which GPU backend was compiled in.
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.window.window_handle()
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.window.display_handle()
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
}

impl crate::host_window::CoreHostWindow for WinitWindow {}

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

impl CallbackWindow for WinitWindow {
    fn callbacks(&self) -> &crate::owner::handlers::WindowCallbacks {
        WinitWindow::callbacks(self)
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
    /// `on_state_changed` when the revision grew.
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

    /// Raise `on_visibility_status_change` when [`PlatformWindow::is_visible`]
    /// answers differently from the last report. Showing and hiding, the
    /// occlusion events and the minimized mode all change it, in any order, and
    /// an `Occluded` can repeat what is already known or arrive late. The
    /// answer is read under the report's lock, so two reporters cannot leave an
    /// older one last, and the lock is released before the dispatch.
    fn report_visibility(&self) {
        let report = self
            .reported_visible
            .lock()
            .report(PlatformWindow::is_visible(self));
        if let Some(visible) = report {
            self.callbacks.dispatch_visibility_status_change(visible);
        }
    }

    /// Whether [`PlatformWindow::display_id`] answers differently from the last
    /// time this was asked, remembering the answer. The window event handler
    /// uses it to raise `on_display_changed` only on a real move.
    fn take_display_change(&self) -> bool {
        let current = PlatformWindow::display_id(self);
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
    /// [`PlatformWindow::appearance`] reports.
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
    ) -> Result<Arc<dyn LoopWindow>, OpenWindowError> {
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
        let window = Arc::new(WinitWindow::new(
            seed.id,
            raw_window,
            seed.close_lane,
            Arc::clone(seed.displays),
            &options,
            &plan,
            self,
        ));
        seed.notices.mark_stale_on_wake(window.state_cell());
        // Before the window is mapped: the window manager reads the mode a
        // window opens in from the properties the view writes.
        #[cfg(target_os = "linux")]
        window.attach_x11_probe(|| self.window_manager(seed.signal));
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
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
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
            let window_id = owner.create_window_now(event_loop, options)?;
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

        // The command crosses threads as data only. The owner creates the
        // window and completes this claim-slot request (ADR-0039 §3)
        // without exposing winit's thread-affine event-loop capability.
        let handle = control
            .request_open_window(options)
            .map_err(|error| match error {
                ControlSendError::Full { capacity, rejected } => {
                    OpenWindowError::LaneFull { capacity, rejected }
                }
                ControlSendError::OwnerGone { rejected } => OpenWindowError::OwnerGone {
                    rejected: Some(rejected),
                },
            })?;

        tracing::debug!("Waiting for window creation response");

        // This `handle` is freshly minted above and never offered to any
        // other caller, so `try_take` cannot have claimed it already —
        // `AlreadyClaimed` here would mean this fast path itself raced a
        // second claim attempt, which the code above never performs.
        // `OwnerGone` is real, though: the event-loop owner can disconnect
        // (quit, panic-unwind) while this request is in flight on the lane.
        let window = match handle.wait() {
            ClaimOutcome::Delivered(result) => result?,
            ClaimOutcome::AlreadyClaimed => {
                unreachable!("BUG: this handle is never polled by another caller before wait")
            }
            ClaimOutcome::OwnerGone => {
                return Err(OpenWindowError::OwnerGone { rejected: None });
            }
        };

        tracing::info!("Window created successfully");
        Ok(window)
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

    fn on_event(owner: &WinitLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        install(
            &owner.handler(|state| &mut state.handlers.window_event),
            callback,
        );
    }
}

// The window's tests on a real winit event loop, which open, unwind and close
// windows through the loop.
#[cfg(test)]
mod real_loop_tests;

pub(crate) mod plan {
    //! What the winit backend decides about a window before and while it
    //! exists, kept apart from winit so a table of inputs checks it.
    //!
    //! - [`WindowShell`] says what the window system lets a program do to a
    //!   window, and so which [`WindowCapabilities`] a window has. X11, Win32 and
    //!   AppKit let a program place, hide, show and focus a window and read where
    //!   it is; a Wayland compositor does none of that.
    //! - [`OpenPlan`] turns the creation options into what winit is asked for: a
    //!   physical position, a content size and an initial mode.
    //! - [`FrameInset`] is the frame around the content of a decorated window,
    //!   measured once a window with a frame is on screen, so a remembered outer
    //!   size can be fitted before the next window exists.

    #![forbid(unsafe_code)]

    use flui_foundation::geometry::{Point, Size};
    use flui_platform::{
        DisplayId, DisplaySnapshot, WindowCapabilities, WindowGeometry, WindowMode, WindowOptions,
        WindowReveal,
    };
    use parking_lot::Mutex;

    /// What the window system lets a program do to a window.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum WindowShell {
        /// X11, Win32 or AppKit: windows are placed, hidden, shown and focused by
        /// the program, and their position and minimization can be read.
        Desktop,
        /// A Wayland compositor: a program can neither place a window nor hide it,
        /// read where it is, tell whether it is minimized, nor take focus. A
        /// client can still minimize its window, but only the user can bring it
        /// back: asking for normal mode restores a maximized or fullscreen window
        /// and leaves a minimized one to the user.
        Wayland,
    }

    impl WindowShell {
        /// Which commands and observations a window on this shell has.
        pub(super) const fn capabilities(self) -> WindowCapabilities {
            match self {
                Self::Desktop => WindowCapabilities::ALL,
                Self::Wayland => {
                    let mut capabilities = WindowCapabilities::ALL;
                    capabilities.sets_position = false;
                    capabilities.hides = false;
                    capabilities.shows = false;
                    capabilities.activates = false;
                    capabilities.observes_position = false;
                    capabilities.observes_minimized = false;
                    capabilities.places_at_open = false;
                    capabilities
                }
            }
        }

        /// Whether the shell lets a window be created hidden and shown after its
        /// first frame. On Wayland a window has no buffer, and so is not shown,
        /// until its first frame is presented; nothing is deferred.
        pub(super) const fn defers_reveal(self) -> bool {
            matches!(self, Self::Desktop)
        }
    }

    /// The display a window is placed on: where its origin is in the global
    /// device-pixel space, and how many device pixels make a logical one.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) struct PlacementDisplay {
        pub origin: Point<i32>,
        pub scale_factor: f64,
    }

    impl PlacementDisplay {
        /// `geometry`, measured from this display's usable origin in its logical
        /// pixels, as the global logical rectangle a window state publishes.
        fn to_global(self, geometry: WindowGeometry) -> WindowGeometry {
            let scale = self.scale_factor;
            if !(scale.is_finite() && scale > 0.0) {
                return geometry;
            }
            let origin = Point::new(
                f64::from(self.origin.x) / scale,
                f64::from(self.origin.y) / scale,
            );
            WindowGeometry::new(
                geometry
                    .position
                    .map(|at| Point::new(at.x + origin.x, at.y + origin.y)),
                geometry.size,
            )
        }

        /// The display options measure their position on: the one `wanted` names
        /// if the snapshot holds it, else global coordinates (origin zero) at the
        /// primary display's scale.
        ///
        /// A named display's origin is that of its **usable area**, which is what
        /// `WindowPlacement` measures a remembered offset from: a window on a
        /// display with a panel along its top reopens below the panel, not under
        /// it.
        pub(crate) fn choose(
            snapshot: Option<&DisplaySnapshot>,
            wanted: Option<DisplayId>,
        ) -> Self {
            let named = snapshot
                .zip(wanted)
                .and_then(|(snapshot, id)| snapshot.get(id));
            match named {
                Some(display) => Self {
                    origin: display.usable_bounds.origin,
                    scale_factor: display.scale_factor,
                },
                None => Self {
                    origin: Point::new(0, 0),
                    scale_factor: snapshot
                        .and_then(DisplaySnapshot::primary)
                        .map_or(1.0, |display| display.scale_factor),
                },
            }
        }
    }

    /// What winit is asked for when a window is created.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) struct OpenPlan {
        /// The outer top-left corner in global device pixels; `None` lets the
        /// window manager place the window.
        pub position: Option<Point<i32>>,
        /// The content size in logical pixels.
        pub content_size: Size<f64>,
        /// The mode the window opens in. Minimized is not one: a window cannot be
        /// created minimized, so it opens in [`WindowMode::Normal`].
        pub mode: WindowMode,
        /// Whether the window is created hidden, to be shown after its first
        /// frame.
        pub hidden_until_first_frame: bool,
        /// Whether the window is created hidden and mapped as soon as it is set
        /// up: a window that opens maximized or fullscreen is mapped only after
        /// its mode is written into its properties, which a window manager reads
        /// when it maps a window, so it never appears in normal mode first.
        pub map_after_setup: bool,
        /// Whether the application asked for the window hidden (`visible: false`)
        /// on a shell where that means unmapped. Its opening mode is written into
        /// its properties like any other unmapped window's, and it stays unmapped
        /// until the application shows it.
        pub hidden_by_application: bool,
        /// The rectangle the window returns to from `mode`, in global logical
        /// pixels, when it opens
        /// maximized or fullscreen with one remembered.
        pub normal_geometry: Option<WindowGeometry>,
    }

    impl OpenPlan {
        /// Whether the window is created unmapped, to be mapped by the host: after
        /// its first frame, or once its opening mode is set.
        pub(crate) const fn created_unmapped(&self) -> bool {
            self.hidden_until_first_frame || self.map_after_setup
        }

        /// Whether the window is unmapped at creation for any reason, the
        /// application's included.
        #[cfg(any(target_os = "linux", test))]
        pub(super) const fn unmapped_until_shown(&self) -> bool {
            self.created_unmapped() || self.hidden_by_application
        }

        /// Plan the creation of a window on `shell` from `options`.
        ///
        /// `display` is the display the options measure their position on, or the
        /// primary one when they name none or it is gone. `inset` is the frame
        /// around the content of a decorated window, which turns a remembered
        /// outer size into a content size.
        ///
        /// A window that opens maximized or fullscreen is created with the
        /// rectangle it returns to, so un-maximizing it later goes there; the
        /// window manager keeps that rectangle as the window's normal one.
        pub(crate) fn new(
            options: &WindowOptions,
            shell: WindowShell,
            display: PlacementDisplay,
            inset: Size<f64>,
        ) -> Self {
            let capabilities = shell.capabilities();
            let mode = match options.mode {
                WindowMode::Maximized if capabilities.maximizes => WindowMode::Maximized,
                WindowMode::Fullscreen if capabilities.enters_fullscreen => WindowMode::Fullscreen,
                _ => WindowMode::Normal,
            };
            let returns_to = match mode {
                WindowMode::Maximized | WindowMode::Fullscreen => options.normal_geometry,
                _ => None,
            };
            // An undecorated window has no frame to take from a remembered outer
            // size, whatever the decorated windows before it measured.
            let inset = if options.decorated {
                inset
            } else {
                Size::new(0.0, 0.0)
            };
            let (at, content_size) = match returns_to {
                Some(geometry) => (
                    geometry.position,
                    Size::new(
                        (geometry.size.width - inset.width).max(1.0),
                        (geometry.size.height - inset.height).max(1.0),
                    ),
                ),
                None => (options.position, options.size),
            };
            let hidden_until_first_frame = options.visible
                && options.reveal == WindowReveal::AfterFirstFrame
                && shell.defers_reveal();
            let position = if capabilities.places_at_open {
                at.and_then(|at| device_position(at, display))
            } else {
                None
            };
            Self {
                position,
                content_size,
                mode,
                hidden_until_first_frame,
                map_after_setup: options.visible
                    && shell.defers_reveal()
                    && !hidden_until_first_frame
                    && mode != WindowMode::Normal,
                hidden_by_application: !options.visible && shell.defers_reveal(),
                normal_geometry: returns_to.map(|geometry| display.to_global(geometry)),
            }
        }
    }

    /// `at`, in logical pixels from `display`'s origin, as global device pixels.
    /// `None` for a position that is not a finite number of pixels an X11 or Win32
    /// coordinate can hold.
    fn device_position(at: Point<f64>, display: PlacementDisplay) -> Option<Point<i32>> {
        let device = |offset: f64, origin: i32| {
            let pixels = (offset * display.scale_factor).round() + f64::from(origin);
            // Not `as`: a value past the range saturates silently, and a placement
            // that far from every display is a bad one, not one to clamp.
            (pixels.is_finite() && pixels >= f64::from(i32::MIN) && pixels <= f64::from(i32::MAX))
                .then_some(pixels as i32)
        };
        Some(Point::new(
            device(at.x, display.origin.x)?,
            device(at.y, display.origin.y)?,
        ))
    }

    /// The frame a decorated window has around its content, in device pixels:
    /// the title bar and borders, measured from a window that is on screen.
    ///
    /// The memory is shared by the platform's windows. It holds the last
    /// measurement, and none until one decorated window has been seen with its
    /// frame.
    #[derive(Debug, Default)]
    pub(crate) struct FrameInset {
        measured: Mutex<Option<Size<i32>>>,
    }

    impl FrameInset {
        /// Record the sizes of a window the caller found in normal mode (a
        /// maximized or fullscreen window's frame is not the one a normal window
        /// gets) if they show a frame: the window is decorated, and its outer
        /// extent is at least its content's in both dimensions, with some
        /// difference.
        pub(super) fn observe(&self, decorated: bool, outer: Size<i32>, content: Size<i32>) {
            if !decorated || outer.width < content.width || outer.height < content.height {
                return;
            }
            self.record(Size::new(
                outer.width - content.width,
                outer.height - content.height,
            ));
        }

        /// Record a frame the window manager reported, in device pixels. A frame
        /// of nothing, or a negative one, is not a measurement.
        pub(super) fn record(&self, frame: Size<i32>) {
            if frame.width < 0 || frame.height < 0 || (frame.width == 0 && frame.height == 0) {
                return;
            }
            *self.measured.lock() = Some(frame);
        }

        /// Whether any frame was measured.
        #[cfg(any(target_os = "linux", test))]
        pub(super) fn is_known(&self) -> bool {
            self.measured.lock().is_some()
        }

        /// The frame in logical pixels on a display of `scale_factor`; zero before
        /// any was measured.
        pub(crate) fn logical(&self, scale_factor: f64) -> Size<f64> {
            let measured = *self.measured.lock();
            match measured {
                Some(frame) if scale_factor.is_finite() && scale_factor > 0.0 => Size::new(
                    f64::from(frame.width) / scale_factor,
                    f64::from(frame.height) / scale_factor,
                ),
                _ => Size::new(0.0, 0.0),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const DISPLAY: PlacementDisplay = PlacementDisplay {
            origin: Point { x: 1920, y: 0 },
            scale_factor: 1.0,
        };

        fn options() -> WindowOptions {
            WindowOptions::default()
                .with_size(Size::new(800.0, 600.0))
                .with_position(Some(Point::new(100.0, 50.0)))
        }

        fn plan(options: &WindowOptions, shell: WindowShell) -> OpenPlan {
            OpenPlan::new(options, shell, DISPLAY, Size::new(0.0, 0.0))
        }

        #[test]
        fn placement_display_is_measured_from_the_usable_area() {
            use flui_foundation::geometry::Bounds;
            use flui_platform::{DisplayInfo, DisplayKey, DisplayRegistry};

            let registry = DisplayRegistry::new();
            let id = |_| registry.mint(&DisplayKey::Unkeyed).expect("an id");
            // A 1920x1080 display at scale 2 with a 56-pixel panel along its top.
            let bounds = Bounds::new(Point::new(1920, 0), Size::new(1920, 1080));
            let usable = Bounds::new(Point::new(1920, 56), Size::new(1920, 1024));
            let main = DisplayInfo::new(id(0), "main", bounds, 2.0)
                .with_usable_bounds(usable)
                .with_primary(true);
            let other = DisplayInfo::new(id(1), "other", bounds, 1.0);
            let (main_id, other_id) = (main.id, other.id);
            let snapshot = DisplaySnapshot::empty()
                .successor(vec![main, other])
                .expect("a snapshot");

            // (case, wanted display, display placed on)
            let table = [
                (
                    "a named display: its usable origin and scale",
                    Some(main_id),
                    PlacementDisplay {
                        origin: Point::new(1920, 56),
                        scale_factor: 2.0,
                    },
                ),
                (
                    "a named display with no panel",
                    Some(other_id),
                    PlacementDisplay {
                        origin: Point::new(1920, 0),
                        scale_factor: 1.0,
                    },
                ),
                (
                    "none named: global, at the primary scale",
                    None,
                    PlacementDisplay {
                        origin: Point::new(0, 0),
                        scale_factor: 2.0,
                    },
                ),
            ];
            for (case, wanted, expected) in table {
                assert_eq!(
                    PlacementDisplay::choose(Some(&snapshot), wanted),
                    expected,
                    "{case}"
                );
            }
            let gone = registry.mint(&DisplayKey::Unkeyed).expect("an id");
            assert_eq!(
                PlacementDisplay::choose(Some(&snapshot), Some(gone)).origin,
                Point::new(0, 0),
                "a display that is gone"
            );
            assert_eq!(
                PlacementDisplay::choose(None, Some(main_id)),
                PlacementDisplay {
                    origin: Point::new(0, 0),
                    scale_factor: 1.0
                },
                "no snapshot"
            );

            // A window 100 logical pixels below the top of the display is saved as
            // an offset from the usable area and reopens where it was.
            let main_display = PlacementDisplay::choose(Some(&snapshot), Some(main_id));
            let options = options().with_position(Some(Point::new(10.0, 22.0)));
            let planned = OpenPlan::new(
                &options,
                WindowShell::Desktop,
                main_display,
                Size::new(0.0, 0.0),
            );
            assert_eq!(planned.position, Some(Point::new(1940, 100)));
        }

        #[test]
        fn window_shell_capabilities() {
            let desktop = WindowShell::Desktop.capabilities();
            assert_eq!(desktop, WindowCapabilities::ALL);

            let wayland = WindowShell::Wayland.capabilities();
            // (field, value on Wayland)
            let table = [
                ("sets_title", wayland.sets_title, true),
                ("sets_position", wayland.sets_position, false),
                ("sets_size", wayland.sets_size, true),
                ("sets_constraints", wayland.sets_constraints, true),
                ("sets_resizable", wayland.sets_resizable, true),
                ("sets_decorated", wayland.sets_decorated, true),
                ("minimizes", wayland.minimizes, true),
                ("maximizes", wayland.maximizes, true),
                ("enters_fullscreen", wayland.enters_fullscreen, true),
                ("hides", wayland.hides, false),
                ("shows", wayland.shows, false),
                ("activates", wayland.activates, false),
                ("observes_position", wayland.observes_position, false),
                ("observes_minimized", wayland.observes_minimized, false),
                ("places_at_open", wayland.places_at_open, false),
            ];
            for (field, actual, expected) in table {
                assert_eq!(actual, expected, "{field}");
            }
        }

        #[test]
        fn open_plan_places_by_display_origin_and_scale() {
            // (case, position, display, device position)
            let scaled = PlacementDisplay {
                origin: Point::new(-2560, 100),
                scale_factor: 1.5,
            };
            let table = [
                (
                    "on the origin display",
                    Some(Point::new(100.0, 50.0)),
                    DISPLAY,
                    Some(Point::new(2020, 50)),
                ),
                (
                    "scaled, left of the origin",
                    Some(Point::new(10.0, 10.0)),
                    scaled,
                    Some(Point::new(-2545, 115)),
                ),
                (
                    "fractions round",
                    Some(Point::new(0.4, 0.6)),
                    DISPLAY,
                    Some(Point::new(1920, 1)),
                ),
                ("no position", None, DISPLAY, None),
                (
                    "infinite",
                    Some(Point::new(f64::INFINITY, 0.0)),
                    DISPLAY,
                    None,
                ),
                (
                    "not a number",
                    Some(Point::new(0.0, f64::NAN)),
                    DISPLAY,
                    None,
                ),
                (
                    "past the coordinate range",
                    Some(Point::new(3.0e9, 0.0)),
                    DISPLAY,
                    None,
                ),
                (
                    "before the coordinate range",
                    Some(Point::new(-3.0e9, 0.0)),
                    DISPLAY,
                    None,
                ),
            ];
            for (case, position, display, expected) in table {
                let options = options().with_position(position);
                let planned =
                    OpenPlan::new(&options, WindowShell::Desktop, display, Size::new(0.0, 0.0));
                assert_eq!(planned.position, expected, "{case}");
            }
        }

        #[test]
        fn open_plan_leaves_placement_to_a_shell_that_cannot_place() {
            let planned = plan(&options(), WindowShell::Wayland);
            assert_eq!(planned.position, None);
            assert_eq!(planned.content_size, Size::new(800.0, 600.0));
        }

        #[test]
        fn open_plan_modes() {
            // (case, mode, shell, mode planned)
            let table = [
                (
                    "normal",
                    WindowMode::Normal,
                    WindowShell::Desktop,
                    WindowMode::Normal,
                ),
                (
                    "maximized",
                    WindowMode::Maximized,
                    WindowShell::Desktop,
                    WindowMode::Maximized,
                ),
                (
                    "fullscreen",
                    WindowMode::Fullscreen,
                    WindowShell::Desktop,
                    WindowMode::Fullscreen,
                ),
                (
                    "minimized opens normal",
                    WindowMode::Minimized,
                    WindowShell::Desktop,
                    WindowMode::Normal,
                ),
                (
                    "maximized on Wayland",
                    WindowMode::Maximized,
                    WindowShell::Wayland,
                    WindowMode::Maximized,
                ),
                (
                    "fullscreen on Wayland",
                    WindowMode::Fullscreen,
                    WindowShell::Wayland,
                    WindowMode::Fullscreen,
                ),
                (
                    "minimized on Wayland",
                    WindowMode::Minimized,
                    WindowShell::Wayland,
                    WindowMode::Normal,
                ),
            ];
            for (case, mode, shell, expected) in table {
                let planned = plan(&options().with_mode(mode), shell);
                assert_eq!(planned.mode, expected, "{case}");
            }
        }

        #[test]
        fn open_plan_opens_a_maximized_window_at_the_rectangle_it_returns_to() {
            let returns_to =
                WindowGeometry::new(Some(Point::new(300.0, 200.0)), Size::new(1000.0, 700.0));
            let frame = Size::new(2.0, 30.0);
            let maximized = options()
                .with_mode(WindowMode::Maximized)
                .with_normal_geometry(Some(returns_to));

            let planned = OpenPlan::new(&maximized, WindowShell::Desktop, DISPLAY, frame);
            assert_eq!(planned.position, Some(Point::new(2220, 200)));
            assert_eq!(
                planned.content_size,
                Size::new(998.0, 670.0),
                "outer less the frame"
            );
            // The state publishes global positions, whatever display the options
            // measured theirs from.
            assert_eq!(
                planned.normal_geometry,
                Some(WindowGeometry::new(
                    Some(Point::new(2220.0, 200.0)),
                    Size::new(1000.0, 700.0)
                ))
            );

            // On a scaled display the usable origin is in device pixels.
            let scaled = PlacementDisplay {
                origin: Point::new(1920, 56),
                scale_factor: 2.0,
            };
            let relative =
                WindowGeometry::new(Some(Point::new(100.0, 50.0)), Size::new(1000.0, 700.0));
            let on_scaled = options()
                .with_mode(WindowMode::Maximized)
                .with_normal_geometry(Some(relative));
            let planned = OpenPlan::new(
                &on_scaled,
                WindowShell::Desktop,
                scaled,
                Size::new(0.0, 0.0),
            );
            assert_eq!(
                planned
                    .normal_geometry
                    .and_then(|geometry| geometry.position),
                Some(Point::new(1060.0, 78.0))
            );

            // A normal window ignores the rectangle it has no use for.
            let normal = options().with_normal_geometry(Some(returns_to));
            let planned = OpenPlan::new(&normal, WindowShell::Desktop, DISPLAY, frame);
            assert_eq!(planned.position, Some(Point::new(2020, 50)));
            assert_eq!(planned.content_size, Size::new(800.0, 600.0));
            assert_eq!(planned.normal_geometry, None);

            // A rectangle smaller than the frame still has a content area.
            let tiny = WindowGeometry::new(None, Size::new(1.0, 1.0));
            let tiny = options()
                .with_mode(WindowMode::Fullscreen)
                .with_normal_geometry(Some(tiny));
            let planned = OpenPlan::new(&tiny, WindowShell::Desktop, DISPLAY, frame);
            assert_eq!(planned.content_size, Size::new(1.0, 1.0));
            assert_eq!(
                planned.position, None,
                "its rectangle has no position either"
            );
        }

        #[test]
        fn open_plan_takes_no_frame_from_an_undecorated_window() {
            let returns_to =
                WindowGeometry::new(Some(Point::new(300.0, 200.0)), Size::new(1000.0, 700.0));
            let frame = Size::new(2.0, 30.0);
            // (case, decorated, content size)
            let table = [
                ("decorated", true, Size::new(998.0, 670.0)),
                ("undecorated", false, Size::new(1000.0, 700.0)),
            ];
            for (case, decorated, expected) in table {
                let options = options()
                    .with_decorated(decorated)
                    .with_mode(WindowMode::Maximized)
                    .with_normal_geometry(Some(returns_to));
                let planned = OpenPlan::new(&options, WindowShell::Desktop, DISPLAY, frame);
                assert_eq!(planned.content_size, expected, "{case}");
            }
        }

        #[test]
        fn open_plan_maps_a_window_that_opens_in_a_mode_after_setting_it_up() {
            // (case, visible, reveal, mode, shell, hidden until first frame, mapped after setup)
            let table = [
                (
                    "normal at open",
                    true,
                    WindowReveal::AtOpen,
                    WindowMode::Normal,
                    WindowShell::Desktop,
                    false,
                    false,
                ),
                (
                    "maximized at open",
                    true,
                    WindowReveal::AtOpen,
                    WindowMode::Maximized,
                    WindowShell::Desktop,
                    false,
                    true,
                ),
                (
                    "fullscreen at open",
                    true,
                    WindowReveal::AtOpen,
                    WindowMode::Fullscreen,
                    WindowShell::Desktop,
                    false,
                    true,
                ),
                (
                    "maximized after the first frame",
                    true,
                    WindowReveal::AfterFirstFrame,
                    WindowMode::Maximized,
                    WindowShell::Desktop,
                    true,
                    false,
                ),
                (
                    "maximized, opened hidden",
                    false,
                    WindowReveal::AtOpen,
                    WindowMode::Maximized,
                    WindowShell::Desktop,
                    false,
                    false,
                ),
                (
                    "maximized on Wayland",
                    true,
                    WindowReveal::AtOpen,
                    WindowMode::Maximized,
                    WindowShell::Wayland,
                    false,
                    false,
                ),
            ];
            for (case, visible, reveal, mode, shell, hidden, mapped) in table {
                let options = options()
                    .with_visible(visible)
                    .with_reveal(reveal)
                    .with_mode(mode);
                let planned = plan(&options, shell);
                assert_eq!(planned.hidden_until_first_frame, hidden, "{case}");
                assert_eq!(planned.map_after_setup, mapped, "{case}");
                assert_eq!(
                    planned.hidden_by_application,
                    !visible && shell == WindowShell::Desktop,
                    "{case}"
                );
                assert_eq!(
                    planned.unmapped_until_shown(),
                    hidden || mapped || (!visible && shell == WindowShell::Desktop),
                    "{case}"
                );
            }
        }
        #[test]
        fn open_plan_defers_the_reveal_only_where_it_can() {
            // (case, visible, reveal, shell, hidden until the first frame)
            let table = [
                (
                    "desktop, after first frame",
                    true,
                    WindowReveal::AfterFirstFrame,
                    WindowShell::Desktop,
                    true,
                ),
                (
                    "desktop, at open",
                    true,
                    WindowReveal::AtOpen,
                    WindowShell::Desktop,
                    false,
                ),
                (
                    "desktop, opened hidden",
                    false,
                    WindowReveal::AfterFirstFrame,
                    WindowShell::Desktop,
                    false,
                ),
                (
                    "wayland shows with its first buffer",
                    true,
                    WindowReveal::AfterFirstFrame,
                    WindowShell::Wayland,
                    false,
                ),
            ];
            for (case, visible, reveal, shell, expected) in table {
                let options = options().with_visible(visible).with_reveal(reveal);
                assert_eq!(
                    plan(&options, shell).hidden_until_first_frame,
                    expected,
                    "{case}"
                );
            }
        }

        #[test]
        fn frame_inset_is_measured_from_a_framed_window_on_screen() {
            let inset = FrameInset::default();
            assert_eq!(
                inset.logical(1.0),
                Size::new(0.0, 0.0),
                "nothing measured yet"
            );
            assert!(!inset.is_known());

            // (case, decorated, outer, content)
            let ignored = [
                (
                    "undecorated",
                    false,
                    Size::new(810, 640),
                    Size::new(800, 600),
                ),
                (
                    "no difference",
                    true,
                    Size::new(800, 600),
                    Size::new(800, 600),
                ),
                (
                    "outer narrower than content",
                    true,
                    Size::new(790, 640),
                    Size::new(800, 600),
                ),
                (
                    "outer shorter than content",
                    true,
                    Size::new(810, 590),
                    Size::new(800, 600),
                ),
            ];
            for (case, decorated, outer, content) in ignored {
                inset.observe(decorated, outer, content);
                assert_eq!(inset.logical(1.0), Size::new(0.0, 0.0), "{case}");
            }
            for (case, frame) in [
                ("a frame of nothing", Size::new(0, 0)),
                ("a negative width", Size::new(-1, 10)),
                ("a negative height", Size::new(10, -1)),
            ] {
                inset.record(frame);
                assert!(!inset.is_known(), "{case}");
            }

            inset.observe(true, Size::new(1620, 1260), Size::new(1600, 1200));
            assert_eq!(inset.logical(1.0), Size::new(20.0, 60.0));
            assert!(inset.is_known());
            assert_eq!(
                inset.logical(2.0),
                Size::new(10.0, 30.0),
                "device pixels over scale"
            );
            assert_eq!(
                inset.logical(0.0),
                Size::new(0.0, 0.0),
                "a scale that divides by zero"
            );
            assert_eq!(inset.logical(f64::NAN), Size::new(0.0, 0.0));

            // A frame of one dimension only (a bar and no borders) is a frame, and
            // a frame the window manager reports replaces a measured one.
            inset.observe(true, Size::new(800, 630), Size::new(800, 600));
            assert_eq!(inset.logical(1.0), Size::new(0.0, 30.0));
            inset.record(Size::new(8, 38));
            assert_eq!(inset.logical(1.0), Size::new(8.0, 38.0));
        }
    }
}
