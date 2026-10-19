//! The winit backend's [`PlatformWindow`]: a thin wrapper over
//! `Arc<winit::window::Window>` carrying the platform-minted [`WindowId`],
//! the per-window callback table, and the route its programmatic `close()`
//! takes back to the event-loop owner (`ControlSender`, backend-private —
//! which is why this type lives here, in the backend, and not beside the
//! trait it implements).

use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Bounds, Point, Size};
use parking_lot::Mutex;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::{
    dpi::{LogicalPosition, LogicalSize},
    window::{Fullscreen, Window},
};

#[cfg(target_os = "linux")]
use super::super::linux::x11_window_probe::X11WindowProbe;
use super::control::ControlSender;
use super::display::WinitDisplaySource;
use super::window_plan::{FrameInset, OpenPlan, WindowShell};
use crate::shared::window_state::{WindowStateCell, mode_of};
#[cfg(all(target_os = "linux", feature = "a11y"))]
use crate::traits::PlatformAccessibility;
use crate::traits::{
    CursorError, DisplayId, HostWindow, PlatformTextInput, PlatformWindow, SizeConstraints,
    WindowAppearance, WindowCapabilities, WindowCommand, WindowCommandError, WindowGeometry,
    WindowId, WindowMode, WindowOptions, WindowState,
};

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
    callbacks: crate::shared::WindowCallbacks,
    /// This window's AT-SPI announcement, created with the window so a
    /// screen reader that attaches at any later point finds it on the bus.
    /// Construction succeeds (and stays inert) with no session bus at all —
    /// see [`UnixAccessibility::new`](crate::platforms::linux::UnixAccessibility::new).
    #[cfg(all(target_os = "linux", feature = "a11y"))]
    accessibility: Arc<crate::platforms::linux::UnixAccessibility>,
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
    displays: Arc<WinitDisplaySource>,
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
    x11: std::sync::OnceLock<(Arc<X11WindowProbe>, u32)>,
    /// Set when this is an X11 window and no probe could be attached: its
    /// minimization then goes unseen until some other event samples it.
    #[cfg(target_os = "linux")]
    minimize_unobserved: AtomicBool,
}

/// What the window system lets a program do to `window`: a Wayland surface
/// gets none of the placement, hiding and focus commands, every other shell
/// gets them all.
pub(super) fn shell_of(window: &impl HasDisplayHandle) -> WindowShell {
    match window.display_handle().map(|display| display.as_raw()) {
        Ok(raw_window_handle::RawDisplayHandle::Wayland(_)) => WindowShell::Wayland,
        _ => WindowShell::Desktop,
    }
}

/// [`PlatformTextInput`] for a winit window.
///
/// A thin wrapper around `Arc<winit::window::Window>` rather than an impl
/// directly on `WinitWindow`: `PlatformWindow::text_input` hands back an
/// `Arc<dyn PlatformTextInput>` from `&self`. Cloning the exact inner
/// `Arc<Window>` gives the capability independent ownership without cloning
/// or forwarding the platform-window object itself.
struct WinitTextInput {
    window: Arc<Window>,
}

impl PlatformTextInput for WinitTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.window.set_ime_allowed(allowed);
    }

    fn set_ime_cursor_area(&self, area: Bounds<f64>) {
        use winit::dpi::{LogicalPosition, LogicalSize};

        self.window.set_ime_cursor_area(
            LogicalPosition::new(area.origin.x, area.origin.y),
            LogicalSize::new(area.size.width, area.size.height),
        );
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
    /// returns to.
    pub(super) fn new(
        id: WindowId,
        window: Arc<Window>,
        close_lane: ControlSender,
        displays: Arc<WinitDisplaySource>,
        options: &WindowOptions,
        plan: &OpenPlan,
        frame_inset: Arc<FrameInset>,
    ) -> Self {
        let shell = shell_of(&window);
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
            frame_inset,
            #[cfg(target_os = "linux")]
            x11: std::sync::OnceLock::new(),
            #[cfg(target_os = "linux")]
            minimize_unobserved: AtomicBool::new(false),
            window,
            is_focused: parking_lot::Mutex::new(true),
            is_visible: parking_lot::Mutex::new(options.visible),
            callbacks: crate::shared::WindowCallbacks::new(),
            #[cfg(all(target_os = "linux", feature = "a11y"))]
            accessibility: Arc::new(crate::platforms::linux::UnixAccessibility::new()),
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
    pub(super) fn attach_x11_probe(&self, probe: impl FnOnce() -> Option<Arc<X11WindowProbe>>) {
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
        probe.set_mode_hint(id, self.opens_in);
        let _ = self.x11.set((probe, id));
        // The snapshot made at creation knew nothing of the probe.
        self.refresh_state();
    }

    /// Map a window created hidden to open in a mode, now that the mode is set
    /// where the window manager reads it. A no-op for every other window.
    pub(super) fn finish_setup(&self) {
        if !self.map_after_setup {
            return;
        }
        {
            let mut pending = self.reveal_pending.lock();
            *pending = false;
            self.fit_to_remembered_rectangle();
            self.window.set_visible(true);
        }
        self.refresh_state();
    }

    /// The X11 id of this window, when a probe is attached.
    #[cfg(target_os = "linux")]
    pub(super) fn x11_id(&self) -> Option<u32> {
        self.x11.get().map(|(_, id)| *id)
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
    pub(super) fn state_cell(&self) -> &Arc<WindowStateCell> {
        &self.window_state
    }

    /// Whether no display has been reported for this window yet (or the last
    /// one reported was "none"). A Wayland window is on no output until its
    /// first frame enters one, and winit reports no event when that happens, so
    /// the event loop looks again each time it goes idle while this holds.
    #[cfg(target_os = "linux")]
    pub(super) fn awaiting_display(&self) -> bool {
        self.reported_display.lock().is_none()
    }

    /// Whether [`PlatformWindow::display_id`] answers differently from the last
    /// time this was asked, remembering the answer. The window event handler
    /// uses it to raise `on_display_changed` only on a real move.
    pub(super) fn take_display_change(&self) -> bool {
        let current = PlatformWindow::display_id(self);
        let mut reported = self.reported_display.lock();
        let changed = *reported != current;
        *reported = current;
        changed
    }

    /// Record the portal's `color-scheme`; `true` when it changed what
    /// [`PlatformWindow::appearance`] reports.
    #[cfg(target_os = "linux")]
    pub(super) fn observe_portal_appearance(&self, appearance: WindowAppearance) -> bool {
        let dark = flui_platform_api::Brightness::from(appearance).is_dark();
        let previous = self
            .portal_dark
            .swap(dark, std::sync::atomic::Ordering::Relaxed);
        previous != dark && self.window.theme().is_none()
    }

    pub(super) fn mark_closed(&self) {
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Get the underlying `Arc<Window>`
    pub fn inner(&self) -> &Arc<Window> {
        &self.window
    }

    /// Get a reference to the per-window callbacks
    pub fn callbacks(&self) -> &crate::shared::WindowCallbacks {
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

    /// The window may differ from its last snapshot: read it again, and raise
    /// `on_state_changed` when the revision grew.
    pub(super) fn refresh_state(&self) {
        self.window_state.invalidate();
        let _ = PlatformWindow::state(self);
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
                visible: *self.is_visible.lock(),
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
        state
    }

    /// Enter `mode`. Leaving fullscreen and minimized first keeps entering a
    /// mode from depending on what the window manager makes of two requests at
    /// once: it keeps a minimized window minimized under a maximize.
    fn enter_mode(&self, mode: WindowMode) -> Result<(), WindowCommandError> {
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
            _ => return Err(WindowCommandError::Unsupported),
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
    /// reveal and a hide from two threads cannot interleave their halves.
    fn make_visible(&self) {
        let mut pending = self.reveal_pending.lock();
        *pending = false;
        self.fit_to_remembered_rectangle();
        self.window.set_visible(true);
        *self.is_visible.lock() = true;
    }

    /// Hide the window. A hidden window has no first frame to wait for.
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
        mode_of(
            minimized,
            window.fullscreen().is_some(),
            window.is_maximized(),
        )
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
        *self.is_visible.lock()
    }

    fn state(&self) -> WindowState {
        let published = self.window_state.state_with(|| self.sample());
        if published.changed {
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
                crate::WindowShowError::Closed => WindowCommandError::Closed,
                crate::WindowShowError::Native { message } => {
                    WindowCommandError::Backend { message }
                }
                _ => WindowCommandError::Unsupported,
            })?,
            WindowCommand::Hide => self.make_hidden(),
            WindowCommand::Activate => self.window.focus_window(),
            // `admit` refuses a command this window does not know.
            _ => return Err(WindowCommandError::Unsupported),
        }
        self.refresh_state();
        Ok(())
    }

    fn show(&self) -> Result<(), crate::WindowShowError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(crate::WindowShowError::Closed);
        }
        // Winit cannot unminimize or focus Wayland windows.
        if !self.capabilities.shows {
            return Err(crate::WindowShowError::Unsupported);
        }
        self.make_visible();
        self.window.set_minimized(false);
        self.window.focus_window();
        Ok(())
    }

    fn reveal_after_first_frame(&self) {
        {
            let mut pending = self.reveal_pending.lock();
            if self.closed.load(Ordering::Acquire) || !std::mem::take(&mut *pending) {
                return;
            }
            self.fit_to_remembered_rectangle();
            self.window.set_visible(true);
        }
        self.refresh_state();
    }

    fn close(&self) {
        self.mark_closed();
        // Hide now, from whichever thread this is called on: the visible
        // effect of a close should not wait for the owner's next turn. The
        // close itself — `on_close`, map removal, callback clear, exit
        // policy — is the owner's (see `close_lane`): running `on_close`
        // here would fire it on the caller's thread, where an embedder's
        // owner-affine close handling (`flui-app` rejects ui_runtime dispatch
        // off its owner thread) would silently refuse it, and skipping the
        // map removal is precisely issue #919 — a hidden window that the
        // exit policy still counts, so the process never exits.
        self.window.set_visible(false);
        *self.is_visible.lock() = false;
        self.close_lane.request_close_window(self.id);
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.window.set_cursor(cursor);
        Ok(())
    }

    crate::shared::impl_window_callback_setters!(callbacks);

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
        Some(Arc::new(WinitTextInput {
            window: Arc::clone(&self.window),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    // No `haptics()` override: desktop winit targets have no haptic
    // hardware to drive, so the `PlatformWindow` trait default (`None`) is
    // the permanent correct answer here, not a stub awaiting a backend.
}

impl HostWindow for WinitWindow {
    /// The window's own AT-SPI bridge — the capability the composition
    /// root's accessibility wire discovers. Without this override the trait
    /// default (`None`) makes every real Linux window silently
    /// screen-reader-invisible while the headless fake works, which is
    /// exactly backwards.
    #[cfg(all(target_os = "linux", feature = "a11y"))]
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        Some(Arc::clone(&self.accessibility) as _)
    }
}
