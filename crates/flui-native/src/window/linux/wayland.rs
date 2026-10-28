//! The Wayland window: an `xdg_toplevel` on a `wl_surface`, sized through its
//! `wp_viewport`, as the native Wayland host opens it.
//!
//! Two halves:
//!
//! - [`configure`], the pure one: what a configure the compositor sends means
//!   for the window (its states, the size it takes, what the compositor lets
//!   it do), checked on every host;
//! - `toplevel`: `WaylandWindow`, the `PlatformWindow` over the protocol
//!   objects, and the dispatch of those objects on the owner's queue
//!   (`owner::linux::wayland::WaylandState`).
//!
//! # What the compositor decides
//!
//! A Wayland client neither places its window nor reads where it is; it
//! cannot hide, show or focus it either, and never hears that it was
//! minimized. The window's capabilities say so, and the commands are refused
//! at admission. What the compositor tells the window comes in a configure:
//! the toplevel's states (maximized, fullscreen, activated, tiled, suspended)
//! and, when it sizes the window itself, the size. The window acknowledges a
//! configure on the owner's next turn and publishes what it says.
//!
//! # Size and scale
//!
//! The window's size is logical. Where the compositor offers `wp_viewporter`,
//! the surface's destination is the logical size and the buffer is drawn at
//! the scale the compositor prefers for the surface (`wl_surface` version 6),
//! else at the scale of the output the surface entered. Any buffer size is
//! then valid while a resize is under way. Without the viewporter the buffer
//! is drawn at scale 1 and the compositor scales it.
//!
//! # The surface
//!
//! The `wl_surface` outlives the window's role: a surface target holds it, so
//! the raster thread's swapchain is never left with a destroyed surface. The
//! window destroys its `xdg_toplevel` and `xdg_surface` once its handler (and
//! the renderer it owns) let go, and the surface itself goes with the last
//! target.

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) use toplevel::{WaylandWindow, WaylandWindows};

pub(crate) mod configure {
    //! What a configure from the compositor means for a Wayland window: the
    //! toplevel's states, the size the window takes, and the capabilities the
    //! compositor announces (xdg-shell).

    #![forbid(unsafe_code)]

    use flui_foundation::geometry::Size;
    use flui_platform::{
        Availability, CapabilityAbsence, SizeConstraints, WindowCapabilities, WindowCommand,
        WindowMode,
    };

    /// `xdg_toplevel.state` values.
    const MAXIMIZED: u32 = 1;
    const FULLSCREEN: u32 = 2;
    const RESIZING: u32 = 3;
    const ACTIVATED: u32 = 4;
    const TILED_LEFT: u32 = 5;
    const TILED_BOTTOM: u32 = 8;
    const SUSPENDED: u32 = 9;

    /// `xdg_toplevel.wm_capabilities` values.
    const CAN_MAXIMIZE: u32 = 2;
    const CAN_FULLSCREEN: u32 = 3;
    const CAN_MINIMIZE: u32 = 4;

    /// The values of a protocol array of `u32`s, which the compositor writes
    /// in the host's byte order; a trailing partial value is ignored.
    fn values(raw: &[u8]) -> impl Iterator<Item = u32> + '_ {
        raw.as_chunks::<4>().0.iter().copied().map(u32::from_ne_bytes)
    }

    /// The states of a toplevel, as its last configure named them.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct ToplevelStates {
        /// The compositor maximized the window.
        pub(crate) maximized: bool,
        /// The window covers its output.
        pub(crate) fullscreen: bool,
        /// The user is resizing the window.
        pub(crate) resizing: bool,
        /// The window has the compositor's activation: its keyboard focus.
        pub(crate) activated: bool,
        /// The window is tiled against an edge of its output.
        pub(crate) tiled: bool,
        /// The compositor does not show the window (version 6).
        pub(crate) suspended: bool,
    }

    impl ToplevelStates {
        /// The states a configure's array names. A value this window does
        /// not know (a newer protocol's) is ignored.
        pub(crate) fn decode(raw: &[u8]) -> Self {
            let mut states = Self::default();
            for state in values(raw) {
                match state {
                    MAXIMIZED => states.maximized = true,
                    FULLSCREEN => states.fullscreen = true,
                    RESIZING => states.resizing = true,
                    ACTIVATED => states.activated = true,
                    TILED_LEFT..=TILED_BOTTOM => states.tiled = true,
                    SUSPENDED => states.suspended = true,
                    _ => {}
                }
            }
            states
        }

        /// Whether the compositor sizes the window itself: a maximized,
        /// fullscreen or tiled window takes the size it is given.
        pub(crate) const fn sized_by_compositor(self) -> bool {
            self.maximized || self.fullscreen || self.tiled
        }

        /// The window's mode. A Wayland client never hears that its window
        /// was minimized, so the mode is never `Minimized`.
        pub(crate) const fn mode(self) -> WindowMode {
            if self.fullscreen {
                WindowMode::Fullscreen
            } else if self.maximized {
                WindowMode::Maximized
            } else {
                WindowMode::Normal
            }
        }
    }

    /// What the compositor lets a toplevel do, from `wm_capabilities`
    /// (version 5).
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct WmCapabilities {
        /// The compositor maximizes a window on request.
        pub(crate) maximize: bool,
        /// The compositor makes a window fullscreen on request.
        pub(crate) fullscreen: bool,
        /// The compositor minimizes a window on request.
        pub(crate) minimize: bool,
    }

    impl WmCapabilities {
        /// The capabilities an announcement's array names.
        pub(crate) fn decode(raw: &[u8]) -> Self {
            let mut capabilities = Self::default();
            for capability in values(raw) {
                match capability {
                    CAN_MAXIMIZE => capabilities.maximize = true,
                    CAN_FULLSCREEN => capabilities.fullscreen = true,
                    CAN_MINIMIZE => capabilities.minimize = true,
                    _ => {}
                }
            }
            capabilities
        }
    }

    /// What a Wayland window honours, given what its compositor announced
    /// (`None` before version 5, or before it said): the title, the size and
    /// its limits, the modes the compositor offers, and closing. Resizability
    /// is emulated with equal limits; placement, hiding, showing and position
    /// are not the client's on Wayland; decorations and activation are
    /// protocols this host does not speak yet (`xdg-decoration`,
    /// `xdg-activation`).
    pub(crate) fn capabilities(announced: Option<WmCapabilities>) -> WindowCapabilities {
        let mut capabilities = WindowCapabilities::ALL
            .unavailable(
                &WindowCommand::SetPosition(flui_foundation::geometry::Point::new(0.0, 0.0)),
                CapabilityAbsence::NotOnThisPlatform,
            )
            .unavailable(&WindowCommand::Hide, CapabilityAbsence::NotOnThisPlatform)
            .unavailable(&WindowCommand::Show, CapabilityAbsence::NotOnThisPlatform)
            .unavailable(
                &WindowCommand::SetMaterial(flui_platform::WindowMaterial::System),
                CapabilityAbsence::NotOnThisPlatform,
            )
            .unavailable(
                &WindowCommand::SetDecorated(true),
                CapabilityAbsence::NotYetOnThisHost,
            )
            .unavailable(&WindowCommand::Activate, CapabilityAbsence::NotYetOnThisHost);
        capabilities.sets_resizable = Availability::Emulated;
        capabilities.observes_position = Availability::Unavailable;
        capabilities.observes_minimized = Availability::Unavailable;
        capabilities.places_at_open = Availability::Unavailable;
        capabilities.frame_rate_hint = Availability::Unavailable;
        if let Some(announced) = announced {
            if !announced.maximize {
                capabilities = capabilities.unavailable(
                    &WindowCommand::SetMode(WindowMode::Maximized),
                    CapabilityAbsence::NotOnThisDevice,
                );
            }
            if !announced.fullscreen {
                capabilities = capabilities.unavailable(
                    &WindowCommand::SetMode(WindowMode::Fullscreen),
                    CapabilityAbsence::NotOnThisDevice,
                );
            }
            if !announced.minimize {
                capabilities = capabilities.unavailable(
                    &WindowCommand::SetMode(WindowMode::Minimized),
                    CapabilityAbsence::NotOnThisDevice,
                );
            }
        }
        capabilities
    }

    /// `size` within `constraints`, the maximum before the minimum, and at
    /// least one logical pixel each way.
    pub(crate) fn clamp(size: Size<f64>, constraints: SizeConstraints) -> Size<f64> {
        let mut width = size.width;
        let mut height = size.height;
        if let Some(max) = constraints.max {
            width = width.min(max.width);
            height = height.min(max.height);
        }
        if let Some(min) = constraints.min {
            width = width.max(min.width);
            height = height.max(min.height);
        }
        Size::new(width.max(1.0), height.max(1.0))
    }

    /// A logical extent as the protocol's `int`: rounded, and held within
    /// what the protocol can carry; zero for a value that is no size.
    pub(crate) fn protocol_extent(extent: f64) -> i32 {
        if !extent.is_finite() || extent <= 0.0 {
            return 0;
        }
        let rounded = extent.round();
        if rounded >= f64::from(i32::MAX) {
            i32::MAX
        } else {
            // In range and integral: the cast is exact.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "checked against i32::MAX and rounded above"
            )]
            let extent = rounded as i32;
            extent
        }
    }

    /// The content size of a window as its client and compositor decide it.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) struct Sizing {
        /// The content size, in logical pixels.
        content: Size<f64>,
        /// The size the window has in normal mode, which it returns to.
        normal: Size<f64>,
        /// A size asked for while the compositor sized the window, taken
        /// once it stops.
        pending: Option<Size<f64>>,
        /// The states of the configure last acknowledged.
        states: ToplevelStates,
    }

    impl Sizing {
        /// A window that opens at `size`.
        pub(crate) const fn new(size: Size<f64>) -> Self {
            Self {
                content: size,
                normal: size,
                pending: None,
                states: ToplevelStates {
                    maximized: false,
                    fullscreen: false,
                    resizing: false,
                    activated: false,
                    tiled: false,
                    suspended: false,
                },
            }
        }

        /// The content size now.
        pub(crate) const fn content(&self) -> Size<f64> {
            self.content
        }

        /// The states of the configure last acknowledged.
        pub(crate) const fn states(&self) -> ToplevelStates {
            self.states
        }

        /// Take a configure of `width` by `height` (zero: the client decides)
        /// with `states`. A window the compositor sizes takes the size it is
        /// given; one in normal mode takes a size asked for meanwhile, else
        /// the compositor's suggestion within `constraints`, else returns to
        /// the size it had in normal mode. Returns whether the content size
        /// changed.
        pub(crate) fn configure(
            &mut self,
            width: i32,
            height: i32,
            states: ToplevelStates,
            constraints: SizeConstraints,
        ) -> bool {
            let before = self.content;
            let suggested = (width > 0 && height > 0)
                .then(|| Size::new(f64::from(width), f64::from(height)));
            if states.sized_by_compositor() {
                if let Some(size) = suggested {
                    self.content = size;
                }
            } else if let Some(size) = self.pending.take() {
                self.content = clamp(size, constraints);
            } else if let Some(size) = suggested {
                self.content = clamp(size, constraints);
            } else if self.states.sized_by_compositor() {
                self.content = self.normal;
            }
            if !states.sized_by_compositor() {
                self.normal = self.content;
            }
            self.states = states;
            self.content != before
        }

        /// The client asks for `size`. In normal mode it is the size at once;
        /// while the compositor sizes the window it is taken when the
        /// compositor stops, and this returns `true`: the window asks to
        /// leave the mode.
        pub(crate) fn request(&mut self, size: Size<f64>, constraints: SizeConstraints) -> bool {
            if self.states.sized_by_compositor() {
                self.pending = Some(size);
                true
            } else {
                self.content = clamp(size, constraints);
                self.normal = self.content;
                false
            }
        }

        /// New limits: a window in normal mode is held within them.
        pub(crate) fn constrain(&mut self, constraints: SizeConstraints) {
            if !self.states.sized_by_compositor() {
                self.content = clamp(self.content, constraints);
                self.normal = self.content;
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn array(values: &[u32]) -> Vec<u8> {
            values.iter().flat_map(|value| value.to_ne_bytes()).collect()
        }

        /// A configure's states read as the protocol numbers them, a value of
        /// a newer protocol is ignored, and the mode follows: fullscreen over
        /// maximized, never minimized.
        #[test]
        fn a_configure_names_the_toplevel_states() {
            let states = ToplevelStates::decode(&array(&[ACTIVATED, MAXIMIZED, 77]));
            assert!(states.activated && states.maximized);
            assert!(!states.fullscreen && !states.tiled && !states.suspended);
            assert_eq!(states.mode(), WindowMode::Maximized);
            let both = ToplevelStates::decode(&array(&[MAXIMIZED, FULLSCREEN]));
            assert_eq!(both.mode(), WindowMode::Fullscreen);
            let tiled = ToplevelStates::decode(&array(&[TILED_BOTTOM, SUSPENDED]));
            assert!(tiled.tiled && tiled.suspended && tiled.sized_by_compositor());
            assert_eq!(tiled.mode(), WindowMode::Normal);
            // A torn array keeps its whole values.
            let mut torn = array(&[ACTIVATED]);
            torn.push(1);
            assert!(ToplevelStates::decode(&torn).activated);
        }

        /// The size a window takes from each configure: the compositor's for
        /// a maximized or fullscreen window, the client's own otherwise, a
        /// size asked for while maximized once it is not, and the normal size
        /// back when the compositor leaves it to the client.
        #[test]
        fn a_window_takes_the_size_its_configure_gives() {
            let maximized = ToplevelStates {
                maximized: true,
                ..ToplevelStates::default()
            };
            let normal = ToplevelStates::default();
            let none = SizeConstraints::NONE;
            let mut sizing = Sizing::new(Size::new(800.0, 600.0));

            assert!(!sizing.configure(0, 0, normal, none), "0x0: the client decides");
            assert_eq!(sizing.content(), Size::new(800.0, 600.0));

            assert!(sizing.configure(1920, 1080, maximized, none));
            assert_eq!(sizing.content(), Size::new(1920.0, 1080.0));

            assert!(sizing.request(Size::new(640.0, 480.0), none), "leave the mode first");
            assert_eq!(sizing.content(), Size::new(1920.0, 1080.0), "not yet");
            assert!(sizing.configure(1900, 1000, normal, none));
            assert_eq!(
                sizing.content(),
                Size::new(640.0, 480.0),
                "the size asked for wins over the compositor's suggestion"
            );

            assert!(sizing.configure(1920, 1080, maximized, none));
            assert!(sizing.configure(0, 0, normal, none));
            assert_eq!(
                sizing.content(),
                Size::new(640.0, 480.0),
                "back to the normal size"
            );

            let limits = SizeConstraints::new(Some(Size::new(200.0, 150.0)), Some(Size::new(1000.0, 700.0)));
            assert!(sizing.configure(1200, 100, normal, limits));
            assert_eq!(sizing.content(), Size::new(1000.0, 150.0), "within the limits");
            assert!(!sizing.request(Size::new(50.0, 50.0), limits));
            assert_eq!(sizing.content(), Size::new(200.0, 150.0));
            // Negative sizes are no size.
            assert!(!sizing.configure(-5, 40, normal, limits));
            assert_eq!(sizing.states(), normal);

            // New limits hold a window in normal mode, never one the
            // compositor sizes.
            sizing.constrain(SizeConstraints::new(Some(Size::new(300.0, 300.0)), None));
            assert_eq!(sizing.content(), Size::new(300.0, 300.0));
            assert!(sizing.configure(1920, 1080, maximized, none));
            sizing.constrain(SizeConstraints::new(None, Some(Size::new(100.0, 100.0))));
            assert_eq!(sizing.content(), Size::new(1920.0, 1080.0));
        }

        /// The capabilities follow what the compositor announces, and a
        /// command the client can never carry out on Wayland, or that this
        /// host does not carry out yet, is refused with its own reason.
        #[test]
        fn the_capabilities_follow_the_compositor() {
            let unannounced = capabilities(None);
            assert!(unannounced.maximizes.is_available());
            assert!(unannounced.minimizes.is_available());
            assert_eq!(unannounced.sets_resizable, Availability::Emulated);
            for (command, reason) in [
                (
                    WindowCommand::SetPosition(flui_foundation::geometry::Point::new(1.0, 1.0)),
                    flui_platform::UnsupportedReason::NotOnThisPlatform,
                ),
                (
                    WindowCommand::Hide,
                    flui_platform::UnsupportedReason::NotOnThisPlatform,
                ),
                (
                    WindowCommand::SetDecorated(false),
                    flui_platform::UnsupportedReason::NotYetOnThisHost,
                ),
                (
                    WindowCommand::Activate,
                    flui_platform::UnsupportedReason::NotYetOnThisHost,
                ),
            ] {
                let refused = command
                    .clone()
                    .admit(&unannounced)
                    .expect_err("refused at admission");
                assert_eq!(
                    refused.unsupported().map(|unsupported| unsupported.reason),
                    Some(reason),
                    "{command:?}"
                );
            }
            let announced = capabilities(Some(WmCapabilities::decode(&array(&[CAN_FULLSCREEN]))));
            assert!(!announced.maximizes.is_available());
            assert!(!announced.minimizes.is_available());
            assert!(announced.enters_fullscreen.is_available());
            assert_eq!(
                WindowCommand::SetMode(WindowMode::Maximized)
                    .admit(&announced)
                    .expect_err("the compositor offers no maximize")
                    .unsupported()
                    .map(|unsupported| unsupported.reason),
                Some(flui_platform::UnsupportedReason::NotOnThisDevice)
            );
        }

        /// An extent reaches the protocol rounded and within `int`, and what
        /// is no size reaches it as zero, never as a wrapped number.
        #[test]
        fn an_extent_reaches_the_protocol_within_its_range() {
            assert_eq!(protocol_extent(640.4), 640);
            assert_eq!(protocol_extent(640.5), 641);
            assert_eq!(protocol_extent(1e12), i32::MAX);
            assert_eq!(protocol_extent(f64::NAN), 0);
            assert_eq!(protocol_extent(f64::INFINITY), 0);
            assert_eq!(protocol_extent(-3.0), 0);
        }
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod toplevel {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Weak};

    use cursor_icon::CursorIcon;
    use flui_foundation::geometry::{EdgeInsets, Size, WindowMetrics};
    use flui_platform::input_capabilities::InputCapabilities;
    use flui_platform::{
        AdmittedCommand, ApplyError, CloseDecision, CursorError, PlatformTextInput,
        PlatformWindow, SizeConstraints, Unsupported, UnsupportedReason, WindowCapabilities,
        WindowCommand, WindowExecutionState, WindowGeometry, WindowHandler, WindowId, WindowMode,
        WindowOptions, WindowState,
    };
    use flui_semantics::platform::PlatformAccessibility;
    use parking_lot::Mutex;
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
        RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
    };
    use wayland_client::protocol::wl_callback::{self, WlCallback};
    use wayland_client::protocol::wl_surface::{self, WlSurface};
    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols::wp::viewporter::client::wp_viewport::{self, WpViewport};
    use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
    use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};

    use super::configure::{
        Sizing, ToplevelStates, WmCapabilities, capabilities, clamp, protocol_extent,
    };
    use crate::host::{self, WindowFacets, WindowPositions};
    use crate::host_window::{CoreHostWindow, HostWindow, OwnerThreadToken};
    use crate::owner::OpenWindowError;
    use crate::owner::linux::wayland::{
        LoopWaker, LoopWindow, PortalSlot, WaylandDisplays, WaylandLoop, WaylandState,
        WindowSeed, WindowTable,
    };
    use crate::owner::window_delivery::{Publication, Retirement, WindowDelivery};
    use crate::text::WindowTextInput;
    use crate::window::state::{ReportedFacts, WindowStateCell, not_carried_out};

    /// What the compositor said of one window since the owner last looked,
    /// recorded by the dispatch of the window's objects on the owner's queue,
    /// and what other threads asked of it. The user data of every object of
    /// the window, so it is `Send + Sync`; nothing here runs a handler.
    pub(crate) struct ToplevelRecord {
        protocol: Mutex<Told>,
        /// A frame was asked for.
        redraw: AtomicBool,
        /// A frame callback is out: the compositor has not shown the last
        /// frame yet, and the next waits for it.
        frame_pending: AtomicBool,
        /// The handler granted a close request it was asked after the fact.
        close_granted: AtomicBool,
    }

    /// What the compositor told the window.
    #[derive(Debug, Clone, Default)]
    struct Told {
        /// The toplevel's configure, waiting for its `xdg_surface.configure`.
        toplevel: Option<(i32, i32, ToplevelStates)>,
        /// The size the last toplevel configure gave, `0` where it left
        /// the size to the window.
        size: (i32, i32),
        /// The states of the last toplevel configure.
        states: ToplevelStates,
        /// A complete configure, waiting to be acknowledged.
        configure: Option<Configure>,
        /// The compositor asked the window to close.
        close_requested: bool,
        /// The outputs the surface is on, by global, in the order entered.
        outputs: Vec<u32>,
        /// The buffer scale the compositor prefers for the surface.
        preferred_scale: Option<i32>,
        /// What the compositor lets the toplevel do.
        wm: Option<WmCapabilities>,
    }

    /// A configure, complete with its serial.
    #[derive(Debug, Clone, Copy)]
    struct Configure {
        serial: u32,
        width: i32,
        height: i32,
        states: ToplevelStates,
    }

    impl ToplevelRecord {
        fn new() -> Self {
            Self {
                protocol: Mutex::new(Told::default()),
                redraw: AtomicBool::new(false),
                frame_pending: AtomicBool::new(false),
                close_granted: AtomicBool::new(false),
            }
        }

        /// What the owner carries out now: the configure and the close
        /// request are taken, the rest is read.
        fn take(&self) -> Told {
            let mut told = self.protocol.lock();
            let taken = told.clone();
            told.configure = None;
            told.close_requested = false;
            taken
        }
    }

    /// A window's `wl_surface`, destroyed when the last holder lets go: the
    /// window, or a surface target whose swapchain still draws to it.
    struct SurfaceHandle(WlSurface);

    impl Drop for SurfaceHandle {
        fn drop(&mut self) {
            self.0.destroy();
        }
    }

    /// The role objects of a window, destroyed once its handler let go.
    struct Roles {
        xdg_surface: XdgSurface,
        toplevel: XdgToplevel,
        viewport: Option<WpViewport>,
    }

    impl Roles {
        fn destroy(self) {
            if let Some(viewport) = self.viewport {
                viewport.destroy();
            }
            self.toplevel.destroy();
            self.xdg_surface.destroy();
        }
    }

    /// One window as its window-level positions are made for it
    /// (`host::Window::Ref`): its surface. Public where it is defined, as the
    /// host's table names it, and nameable only inside the crate.
    pub struct SurfaceRef {
        /// The window's surface.
        #[expect(
            dead_code,
            reason = "no window-level position of the Wayland table reads the surface yet"
        )]
        surface: WlSurface,
    }

    impl std::fmt::Debug for SurfaceRef {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SurfaceRef").finish_non_exhaustive()
        }
    }

    /// What the owner thread keeps of a window and changes.
    struct Applied {
        title: String,
        sizing: Sizing,
        constraints: SizeConstraints,
        resizable: bool,
        /// The compositor configured the window at least once.
        configured: bool,
        /// The window asked for normal mode out of fullscreen, and leaves a
        /// maximized mode the compositor puts it back in.
        leaving_maximized: bool,
        told: Told,
    }

    /// The Wayland window: an `xdg_toplevel` the owner thread holds. See the
    /// module docs. Public where it is defined, as the host's table names
    /// it, and nameable only inside the crate.
    pub struct WaylandWindow {
        id: WindowId,
        record: Arc<ToplevelRecord>,
        delivery: Arc<WindowDelivery>,
        state: Arc<WindowStateCell>,
        facets: WindowFacets,
        surface: Arc<SurfaceHandle>,
        roles: RefCell<Option<Roles>>,
        connection: Connection,
        queue: QueueHandle<WaylandState>,
        displays: Arc<dyn WaylandDisplays>,
        portal: PortalSlot,
        table: Weak<WindowTable>,
        waker: Weak<LoopWaker>,
        viewported: bool,
        applied: RefCell<Applied>,
        closed: Cell<bool>,
        /// What a test draws the window's frames with.
        #[cfg(any(test, feature = "testing"))]
        shm: Option<wayland_client::protocol::wl_shm::WlShm>,
    }

    impl std::fmt::Debug for WaylandWindow {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WaylandWindow")
                .field("id", &self.id)
                .finish_non_exhaustive()
        }
    }

    /// The window's state as `applied` and the compositor describe it.
    struct Sample<'a> {
        applied: &'a Applied,
        displays: &'a dyn WaylandDisplays,
        portal: &'a PortalSlot,
        hovered: bool,
        viewported: bool,
        closed: bool,
    }

    impl Sample<'_> {
        fn state(&self) -> WindowState {
            let applied = self.applied;
            let output = applied
                .told
                .outputs
                .first()
                .and_then(|name| self.displays.output(*name));
            // The viewport lets the buffer be drawn at any scale; without it
            // the buffer is the surface's size.
            let scale = if self.viewported {
                applied
                    .told
                    .preferred_scale
                    .filter(|scale| *scale > 0)
                    .map(f64::from)
                    .or_else(|| output.map(|output| output.scale))
                    .unwrap_or(1.0)
            } else {
                1.0
            };
            let content = applied.sizing.content();
            let states = applied.sizing.states();
            let state = WindowState::new(WindowGeometry::new(None, content), content)
                .with_mode(states.mode())
                .with_visible(applied.configured && !states.suspended && !self.closed)
                .with_focused(states.activated)
                .with_title(Some(applied.title.clone()))
                .with_constraints(applied.constraints)
                .with_resizable(applied.resizable)
                // The compositor draws no frame for the window until the
                // host speaks `xdg-decoration`.
                .with_decorated(false)
                .with_display(output.map(|output| output.display));
            ReportedFacts {
                metrics: WindowMetrics::from_logical(content, scale).ok(),
                appearance: self.portal.appearance(),
                safe_area: EdgeInsets::ZERO,
                execution: WindowExecutionState::Running,
                hovered: self.hovered,
                active: states.activated,
                capabilities: capabilities(applied.told.wm),
            }
            .onto(state)
        }
    }

    impl WaylandWindow {
        /// Open a window with `options` from what the loop `owner` seeded,
        /// handing out what `windows` makes of the table's positions.
        fn open(
            owner: &WaylandLoop,
            windows: &WaylandWindows,
            options: &WindowOptions,
            handler: Rc<dyn WindowHandler>,
        ) -> Result<Rc<Self>, OpenWindowError> {
            let WindowSeed {
                id,
                protocol,
                displays,
                portal,
                table,
                signal,
                waker,
                notices,
            } = owner.seed()?;
            let record = Arc::new(ToplevelRecord::new());
            let queue = protocol.queue.clone();
            let surface = protocol
                .compositor
                .create_surface(&queue, Arc::clone(&record));
            let xdg_surface = protocol
                .wm_base
                .get_xdg_surface(&surface, &queue, Arc::clone(&record));
            let toplevel = xdg_surface.get_toplevel(&queue, Arc::clone(&record));
            let viewport = protocol
                .viewporter
                .as_ref()
                .map(|viewporter| viewporter.get_viewport(&surface, &queue, ()));
            toplevel.set_title(options.title.clone());
            let constraints = SizeConstraints::new(options.min_size, options.max_size);
            let size = clamp(options.size, constraints);
            match options.mode {
                WindowMode::Maximized => toplevel.set_maximized(),
                WindowMode::Fullscreen => toplevel.set_fullscreen(None),
                // A client cannot open its window minimized: it is minimized
                // once mapped, which needs a frame.
                _ => {}
            }
            let roles = Roles {
                xdg_surface,
                toplevel,
                viewport,
            };
            send_limits(&roles.toplevel, size, constraints, options.resizable);
            if let Some(viewport) = &roles.viewport {
                set_destination(viewport, size);
            }
            // The first commit carries no buffer: the compositor answers it
            // with the first configure, which the window waits for.
            surface.commit();
            let surface = Arc::new(SurfaceHandle(surface));
            let applied = Applied {
                title: options.title.clone(),
                sizing: Sizing::new(size),
                constraints,
                resizable: options.resizable,
                configured: false,
                leaving_maximized: false,
                told: Told::default(),
            };
            let window = Rc::new(Self {
                id,
                facets: windows.positions.for_window(&SurfaceRef {
                    surface: surface.0.clone(),
                }),
                delivery: Arc::new(WindowDelivery::new(handler, InputCapabilities::UNAVAILABLE)),
                state: Arc::new(WindowStateCell::new(WindowState::new(
                    WindowGeometry::new(None, size),
                    size,
                ))),
                record,
                surface,
                roles: RefCell::new(Some(roles)),
                connection: protocol.connection,
                queue,
                viewported: protocol.viewporter.is_some(),
                displays,
                portal,
                table,
                waker,
                applied: RefCell::new(applied),
                closed: Cell::new(false),
                #[cfg(any(test, feature = "testing"))]
                shm: protocol.shm,
            });
            // From here a failure retires the window like any other close.
            if let Err(error) = owner.roundtrip() {
                window.release();
                return Err(error);
            }
            window.carry_out_protocol();
            // The window's first snapshot holds the first configure; the
            // handler hears nothing before its capability snapshot.
            let _ = window.state.state_with(|| window.sample());
            notices.mark_stale_on_wake(&window.state);
            window
                .delivery
                .publish_through_owner(&window, WaylandWindow::read_state);
            window.delivery.resume_through_signal(&signal);
            if let Err(error) = owner.register(id, Rc::clone(&window) as Rc<dyn LoopWindow>) {
                window.release();
                return Err(error);
            }
            Ok(window)
        }

        /// The window's state as the compositor and the owner describe it now.
        fn sample(&self) -> WindowState {
            let applied = self.applied.borrow();
            Sample {
                applied: &applied,
                displays: self.displays.as_ref(),
                portal: &self.portal,
                hovered: self.delivery.hovered(),
                viewported: self.viewported,
                closed: self.closed.get(),
            }
            .state()
        }

        /// Read the window again for its state publisher, announcing
        /// nothing: `WindowDelivery::dispatch_state_changed` does.
        fn read_state(&self) -> Publication {
            self.state.invalidate();
            self.state.state_with(|| self.sample()).outcome()
        }

        /// Read the window again and tell its handler when the snapshot
        /// changed.
        fn refresh_state(&self) {
            self.state.invalidate();
            let _ = PlatformWindow::state(self);
        }

        /// Carry out what the compositor told the window since the last
        /// turn: acknowledge a configure and take its size. No handler runs.
        /// Returns whether the compositor asked the window to close.
        fn carry_out_protocol(&self) -> bool {
            let told = self.record.take();
            let close_requested = told.close_requested;
            let configure = told.configure;
            let (resized, activated) = {
                let mut applied = self.applied.borrow_mut();
                let first = !applied.configured;
                let resized = configure.is_some_and(|configure| {
                    if let Some(roles) = self.roles.borrow().as_ref() {
                        roles.xdg_surface.ack_configure(configure.serial);
                    }
                    applied.configured = true;
                    let constraints = applied.constraints;
                    let resized = applied.sizing.configure(
                        configure.width,
                        configure.height,
                        configure.states,
                        constraints,
                    );
                    resized || first
                });
                applied.told = told;
                (resized, applied.sizing.states().activated)
            };
            if resized {
                self.size_surface();
                // A configured or resized window needs a frame at its size.
                self.record.redraw.store(true, Ordering::Release);
            }
            if configure.is_some() && self.viewported {
                // The acknowledged configure takes effect with a commit. The
                // viewport already gives the surface the configured size, so
                // the commit is valid with the last buffer, and the
                // compositor's next configure is measured from this one, not
                // from whatever the window last drew.
                self.surface.0.commit();
            }
            if let Some(configure) = configure {
                self.leave_restored_maximized(configure.states);
            }
            if let Some(table) = self.table.upgrade() {
                table.activation(self.id, activated);
            }
            close_requested
        }

        /// A compositor that maximized the window before it went fullscreen
        /// maximizes it again as it leaves: a window on its way to normal mode
        /// leaves that too, once the configure out of fullscreen took effect.
        fn leave_restored_maximized(&self, states: ToplevelStates) {
            {
                let mut applied = self.applied.borrow_mut();
                if !applied.leaving_maximized || states.fullscreen {
                    return;
                }
                applied.leaving_maximized = false;
            }
            if states.maximized
                && let Some(roles) = self.roles.borrow().as_ref()
            {
                roles.toplevel.unset_maximized();
            }
        }

        /// The surface's destination: the content size.
        fn size_surface(&self) {
            let content = self.applied.borrow().sizing.content();
            if let Some(viewport) = self.roles.borrow().as_ref().and_then(|roles| roles.viewport.as_ref()) {
                set_destination(viewport, content);
            }
        }

        /// Send the window's size limits, which a commit applies.
        fn send_limits(&self) {
            let applied = self.applied.borrow();
            if let Some(roles) = self.roles.borrow().as_ref() {
                send_limits(
                    &roles.toplevel,
                    applied.sizing.content(),
                    applied.constraints,
                    applied.resizable,
                );
            }
            self.surface.0.commit();
        }

        /// Ask the compositor to stop sizing the window: out of fullscreen
        /// and out of maximized. A compositor that puts a window maximized
        /// before fullscreen back in it as it leaves fullscreen is asked again
        /// once it has (`leaving_maximized`).
        fn leave_compositor_modes(&self) {
            {
                let mut applied = self.applied.borrow_mut();
                applied.leaving_maximized = applied.sizing.states().fullscreen;
            }
            if let Some(roles) = self.roles.borrow().as_ref() {
                roles.toplevel.unset_fullscreen();
                roles.toplevel.unset_maximized();
            }
        }

        /// Ask the handler whether the window closes, as the compositor asked
        /// it to.
        fn ask_close(&self) {
            let record = Arc::downgrade(&self.record);
            let waker = self.waker.clone();
            let decision = self.delivery.dispatch_close_request(move || {
                // Granted after the fact: the owner closes it on its next turn.
                if let Some(record) = record.upgrade() {
                    record.close_granted.store(true, Ordering::Release);
                }
                if let Some(waker) = waker.upgrade() {
                    waker.wake();
                }
            });
            if decision == CloseDecision::Close {
                self.close();
            }
        }

        /// Close the window: it leaves the loop, its handler hears `closed`,
        /// lets go of what it holds (the renderer and its surface), and then
        /// the role objects are destroyed.
        fn close(&self) {
            if self.closed.replace(true) {
                return;
            }
            if let Some(table) = self.table.upgrade() {
                table.forget(self.id);
            }
            self.facets.accessibility.detach();
            self.facets.text.detach();
            self.delivery.dispatch_close();
            self.retire();
        }

        /// Let go of the handler, then destroy the role objects.
        fn retire(&self) {
            let roles = self.roles.borrow_mut().take();
            self.delivery.clear_then(move || {
                if let Some(roles) = roles {
                    roles.destroy();
                }
            });
        }
    }

    /// The size limits `toplevel` is held to: `constraints`, or `size` both
    /// ways for a window that is not resizable.
    fn send_limits(
        toplevel: &XdgToplevel,
        size: Size<f64>,
        constraints: SizeConstraints,
        resizable: bool,
    ) {
        let extent = |size: Option<Size<f64>>| {
            size.map_or((0, 0), |size| {
                (protocol_extent(size.width), protocol_extent(size.height))
            })
        };
        let (min, max) = if resizable {
            (extent(constraints.min), extent(constraints.max))
        } else {
            (extent(Some(size)), extent(Some(size)))
        };
        toplevel.set_min_size(min.0, min.1);
        toplevel.set_max_size(max.0, max.1);
    }

    /// `viewport`'s destination: `size`, at least a pixel each way.
    fn set_destination(viewport: &WpViewport, size: Size<f64>) {
        viewport.set_destination(
            protocol_extent(size.width).max(1),
            protocol_extent(size.height).max(1),
        );
    }

    impl Drop for WaylandWindow {
        fn drop(&mut self) {
            // The handler may own the renderer whose swapchain draws to the
            // surface: it lets go first, while the role objects still exist.
            self.delivery.clear();
            if let Some(roles) = self.roles.get_mut().take() {
                roles.destroy();
            }
        }
    }

    /// A Wayland window's surface target (ADR-0254 §5).
    ///
    /// It holds the window's surface and the connection, so the handles it
    /// hands out name a live `wl_surface` on a connected `wl_display` for as
    /// long as it lives. Once the window's retirement started (its close, or
    /// the loop's end), both answer `Unavailable`.
    struct WaylandSurface {
        surface: Arc<SurfaceHandle>,
        connection: Connection,
        queue: QueueHandle<WaylandState>,
        record: Arc<ToplevelRecord>,
        retirement: Retirement,
    }

    impl HasWindowHandle for WaylandSurface {
        #[expect(
            unsafe_code,
            reason = "a raw window handle is borrowed from the surface this target keeps alive"
        )]
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            if self.retirement.is_retired() {
                return Err(HandleError::Unavailable);
            }
            let surface = NonNull::new(self.surface.0.id().as_ptr().cast::<c_void>())
                .ok_or(HandleError::Unavailable)?;
            let raw = RawWindowHandle::Wayland(WaylandWindowHandle::new(surface));
            // SAFETY: `surface` is the `wl_proxy` of the `wl_surface` this
            // target's `SurfaceHandle` owns, which destroys it only when the
            // last holder drops; this target is one, so the proxy outlives
            // the borrow of `self` the handle carries.
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    impl HasDisplayHandle for WaylandSurface {
        #[expect(
            unsafe_code,
            reason = "a raw display handle is borrowed from the connection this target keeps alive"
        )]
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            if self.retirement.is_retired() {
                return Err(HandleError::Unavailable);
            }
            let backend: wayland_backend::client::Backend = self.connection.backend();
            let display = NonNull::new(backend.display_ptr().cast::<c_void>())
                .ok_or(HandleError::Unavailable)?;
            let raw = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display));
            // SAFETY: `display` is the `wl_display` of the connection this
            // target holds a clone of; libwayland disconnects it only when
            // the last clone of the connection drops, after this borrow.
            Ok(unsafe { DisplayHandle::borrow_raw(raw) })
        }
    }

    impl flui_platform::SurfaceTarget for WaylandSurface {
        /// Arms the surface's frame callback with the commit the present
        /// makes: the next frame the window delivers waits until the
        /// compositor has shown this one. Returns at once; one callback is
        /// out at a time.
        fn pre_present_notify(&self) {
            if self.retirement.is_retired() || self.record.frame_pending.swap(true, Ordering::AcqRel)
            {
                return;
            }
            let _callback = self.surface.0.frame(&self.queue, Arc::clone(&self.record));
        }
    }

    /// A Wayland window's redraw requester: it marks the frame asked for
    /// and wakes the owner thread. It holds the window's record weakly, so a
    /// requester a waker keeps never keeps a closed window.
    struct WaylandRedraw {
        record: Weak<ToplevelRecord>,
        waker: Weak<LoopWaker>,
        retirement: Retirement,
    }

    impl flui_platform::RedrawRequester for WaylandRedraw {
        fn request_redraw(&self) {
            if self.retirement.is_retired() {
                return;
            }
            if let Some(record) = self.record.upgrade() {
                record.redraw.store(true, Ordering::Release);
                if let Some(waker) = self.waker.upgrade() {
                    waker.wake();
                }
            }
        }
    }

    impl PlatformWindow for WaylandWindow {
        fn id(&self) -> WindowId {
            self.id
        }

        fn request_redraw(&self) {
            if self.delivery.retirement().is_retired() {
                return;
            }
            self.record.redraw.store(true, Ordering::Release);
            // Outside a turn of the loop (a test driving it) the wake is what
            // reaches it.
            if let Some(waker) = self.waker.upgrade() {
                waker.wake();
            }
        }

        fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
            Arc::new(WaylandRedraw {
                record: Arc::downgrade(&self.record),
                waker: self.waker.clone(),
                retirement: self.delivery.retirement(),
            })
        }

        fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
            Arc::new(WaylandSurface {
                surface: Arc::clone(&self.surface),
                connection: self.connection.clone(),
                queue: self.queue.clone(),
                record: Arc::clone(&self.record),
                retirement: self.delivery.retirement(),
            })
        }

        fn state(&self) -> WindowState {
            let published = self.state.state_with(|| self.sample());
            if published.changed {
                self.delivery.dispatch_state_changed();
            }
            published.state
        }

        fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
            if self.closed.get() {
                return Err(ApplyError::Closed);
            }
            let command = command.against(&self.state.published_capabilities())?;
            match command {
                WindowCommand::Close => {
                    self.close();
                    return Ok(());
                }
                WindowCommand::SetTitle(title) => {
                    if let Some(roles) = self.roles.borrow().as_ref() {
                        roles.toplevel.set_title(title.clone());
                    }
                    self.applied.borrow_mut().title = title;
                }
                WindowCommand::SetSize(size) => {
                    let leave = {
                        let mut applied = self.applied.borrow_mut();
                        let constraints = applied.constraints;
                        applied.sizing.request(size, constraints)
                    };
                    if leave {
                        self.leave_compositor_modes();
                    }
                    self.size_surface();
                    self.send_limits();
                    self.record.redraw.store(true, Ordering::Release);
                }
                WindowCommand::SetConstraints(constraints) => {
                    {
                        let mut applied = self.applied.borrow_mut();
                        applied.constraints = constraints;
                        applied.sizing.constrain(constraints);
                    }
                    self.size_surface();
                    self.send_limits();
                    self.record.redraw.store(true, Ordering::Release);
                }
                WindowCommand::SetResizable(resizable) => {
                    self.applied.borrow_mut().resizable = resizable;
                    self.send_limits();
                }
                WindowCommand::SetMode(WindowMode::Normal) => self.leave_compositor_modes(),
                WindowCommand::SetMode(mode) => {
                    let fullscreen = {
                        let mut applied = self.applied.borrow_mut();
                        applied.leaving_maximized = false;
                        applied.sizing.states().fullscreen
                    };
                    let roles = self.roles.borrow();
                    let Some(toplevel) = roles.as_ref().map(|roles| &roles.toplevel) else {
                        return Err(ApplyError::Closed);
                    };
                    match mode {
                        WindowMode::Minimized => toplevel.set_minimized(),
                        WindowMode::Maximized => {
                            if fullscreen {
                                toplevel.unset_fullscreen();
                            }
                            toplevel.set_maximized();
                        }
                        WindowMode::Fullscreen => toplevel.set_fullscreen(None),
                        other => return Err(not_carried_out(&WindowCommand::SetMode(other))),
                    }
                }
                // `admit` refuses what a Wayland window cannot do.
                other => return Err(not_carried_out(&other)),
            }
            // After the command's requests: what the window decides itself
            // shows now, what the compositor decides with its configure.
            self.refresh_state();
            Ok(())
        }

        /// The cursor is the input feature's, which this host does not carry
        /// yet (`cursor-shape`).
        fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
            Err(CursorError::Unsupported(Unsupported::new(
                UnsupportedReason::NotYetOnThisHost,
            )))
        }

        /// A Wayland compositor takes no refresh-rate request.
        fn set_frame_rate_hint(
            &self,
            _hint: flui_platform::FrameRateHint,
        ) -> Result<(), Unsupported> {
            Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
        }

        /// The input regions are the system's edge gestures a region keeps
        /// out, which no Wayland compositor lets a window exclude.
        fn set_input_regions(
            &self,
            _regions: &[flui_platform::input_regions::InputRegion],
        ) -> Result<(), Unsupported> {
            Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
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

        /// No drag session starts from a Wayland window yet (`wl_data_device`).
        fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
            None
        }
    }

    impl CoreHostWindow for WaylandWindow {
        fn window_delivery(&self) -> &WindowDelivery {
            &self.delivery
        }
    }

    impl HostWindow for WaylandWindow {
        fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
            Some(Arc::clone(&self.facets.accessibility) as _)
        }

        fn capture_text_sizing(
            &self,
            _owner: OwnerThreadToken,
        ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
            self.facets.text.capture_sizing()
        }

        fn text_store_host(
            &self,
            _owner: OwnerThreadToken,
        ) -> Option<Rc<dyn flui_platform::text_store::TextStoreHost>> {
            self.facets
                .text
                .window_text_input()
                .and_then(WindowTextInput::store_host)
        }
    }

    impl LoopWindow for WaylandWindow {
        fn settle(&self) {
            if self.closed.get() {
                return;
            }
            let close_requested = self.carry_out_protocol();
            self.refresh_state();
            if close_requested {
                self.ask_close();
            }
            if self.record.close_granted.swap(false, Ordering::AcqRel) {
                self.close();
            }
            if self.frame_due() {
                self.record.redraw.store(false, Ordering::Release);
                self.delivery.dispatch_request_frame();
            }
        }

        fn frame_due(&self) -> bool {
            !self.closed.get()
                && self.applied.borrow().configured
                && self.record.redraw.load(Ordering::Acquire)
                && !self.record.frame_pending.load(Ordering::Acquire)
        }

        fn request_frame(&self) {
            self.record.redraw.store(true, Ordering::Release);
        }

        fn release(&self) {
            if self.closed.replace(true) {
                return;
            }
            self.retire();
        }
    }

    /// What the Wayland host's windows share and open with: what every
    /// window hands out of the window-level positions. Public where it is
    /// defined, as the host's table names it, and nameable only inside the
    /// crate.
    pub struct WaylandWindows {
        positions: WindowPositions<SurfaceRef>,
    }

    impl std::fmt::Debug for WaylandWindows {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WaylandWindows").finish_non_exhaustive()
        }
    }

    impl WaylandWindows {
        /// What the Wayland host's windows open with: each hands out
        /// `positions`.
        pub(crate) fn new(positions: WindowPositions<SurfaceRef>) -> Self {
            Self { positions }
        }
    }

    impl host::Window<WaylandLoop> for WaylandWindow {
        type Shared = WaylandWindows;
        /// A window by its surface.
        type Ref = SurfaceRef;

        fn open(
            owner: &WaylandLoop,
            shared: &Arc<WaylandWindows>,
            options: WindowOptions,
            handler: Rc<dyn WindowHandler>,
        ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
            Self::open(owner, shared, &options, handler).map(|window| window as Rc<dyn HostWindow>)
        }

        fn active(owner: &WaylandLoop, _: &WaylandWindows) -> Option<WindowId> {
            owner.active_window()
        }

        /// What a window has before its compositor said what it offers.
        fn capabilities(_: &WaylandLoop, _: &WaylandWindows) -> WindowCapabilities {
            capabilities(None)
        }

        /// The compositor draws no frame around a window until the host
        /// speaks `xdg-decoration`.
        fn frame_inset(_: &WaylandLoop, _: &WaylandWindows, _: f64) -> Size<f64> {
            Size::new(0.0, 0.0)
        }
    }

    // ---- The dispatch of a window's objects on the owner's queue: each
    // records what the compositor said, and runs no handler.

    impl Dispatch<WlSurface, Arc<ToplevelRecord>> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlSurface,
            event: wl_surface::Event,
            record: &Arc<ToplevelRecord>,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            let mut told = record.protocol.lock();
            match event {
                wl_surface::Event::Enter { output } => {
                    if let Some(name) = output.data::<u32>()
                        && !told.outputs.contains(name)
                    {
                        told.outputs.push(*name);
                    }
                }
                wl_surface::Event::Leave { output } => {
                    if let Some(name) = output.data::<u32>() {
                        told.outputs.retain(|entered| entered != name);
                    }
                }
                wl_surface::Event::PreferredBufferScale { factor } => {
                    told.preferred_scale = Some(factor);
                }
                _ => {}
            }
        }
    }

    impl Dispatch<XdgSurface, Arc<ToplevelRecord>> for WaylandState {
        fn event(
            _: &mut Self,
            _: &XdgSurface,
            event: xdg_surface::Event,
            record: &Arc<ToplevelRecord>,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let xdg_surface::Event::Configure { serial } = event {
                let mut told = record.protocol.lock();
                let last = told.states;
                let (width, height, states) = told.toplevel.take().unwrap_or((0, 0, last));
                told.configure = Some(Configure {
                    serial,
                    width,
                    height,
                    states,
                });
            }
        }
    }

    impl Dispatch<XdgToplevel, Arc<ToplevelRecord>> for WaylandState {
        fn event(
            _: &mut Self,
            _: &XdgToplevel,
            event: xdg_toplevel::Event,
            record: &Arc<ToplevelRecord>,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            let mut told = record.protocol.lock();
            match event {
                xdg_toplevel::Event::Configure {
                    width,
                    height,
                    states,
                } => {
                    let states = ToplevelStates::decode(&states);
                    told.states = states;
                    told.size = (width, height);
                    told.toplevel = Some((width, height, states));
                }
                xdg_toplevel::Event::Close => told.close_requested = true,
                xdg_toplevel::Event::WmCapabilities { capabilities } => {
                    told.wm = Some(WmCapabilities::decode(&capabilities));
                }
                _ => {}
            }
        }
    }

    impl Dispatch<WlCallback, Arc<ToplevelRecord>> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WlCallback,
            event: wl_callback::Event,
            record: &Arc<ToplevelRecord>,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wl_callback::Event::Done { .. } = event {
                record.frame_pending.store(false, Ordering::Release);
            }
        }
    }

    impl Dispatch<WpViewport, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WpViewport,
            _: wp_viewport::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // A viewport has no events.
        }
    }

    /// What the compositor said of a window, for a test that reads it apart
    /// from the window's snapshot.
    #[cfg(any(test, feature = "testing"))]
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct CompositorSaid {
        /// The size the last configure gave, `0` where it left the size to
        /// the window.
        pub(crate) size: (i32, i32),
        /// The states the last configure named.
        pub(crate) states: ToplevelStates,
        /// The window acknowledged a configure.
        pub(crate) configured: bool,
        /// The buffer scale the compositor prefers for the surface.
        pub(crate) preferred_scale: Option<i32>,
        /// The output the surface entered first, by global.
        pub(crate) output: Option<u32>,
    }

    #[cfg(any(test, feature = "testing"))]
    impl WaylandWindow {
        /// What the compositor said of the window, as its last turn
        /// recorded it.
        pub(crate) fn compositor_said(&self) -> CompositorSaid {
            let applied = self.applied.borrow();
            CompositorSaid {
                size: applied.told.size,
                states: applied.told.states,
                configured: applied.configured,
                preferred_scale: applied.told.preferred_scale,
                output: applied.told.outputs.first().copied(),
            }
        }

        /// Draw one blank frame at the window's metrics and commit it, the
        /// way a renderer presents: `false` where the compositor has no
        /// `wl_shm`, the window has no drawable area or is closed.
        pub(crate) fn present_blank(&self) -> bool {
            use std::os::fd::AsFd;

            use wayland_client::protocol::wl_shm;

            let (Some(shm), Some(metrics), false) = (
                self.shm.as_ref(),
                PlatformWindow::state(self).metrics,
                self.closed.get(),
            ) else {
                return false;
            };
            let size = metrics.physical();
            let (width, height) = (size.width, size.height);
            let Some(stride) = width.checked_mul(4) else {
                return false;
            };
            let Some(bytes) = stride.checked_mul(height) else {
                return false;
            };
            let Ok(file) = tempfile::tempfile() else {
                return false;
            };
            if file.set_len(u64::from(bytes.unsigned_abs())).is_err() {
                return false;
            }
            let pool = shm.create_pool(file.as_fd(), bytes, &self.queue, ());
            let buffer = pool.create_buffer(
                0,
                width,
                height,
                stride,
                wl_shm::Format::Argb8888,
                &self.queue,
                (),
            );
            pool.destroy();
            let surface = &self.surface.0;
            surface.attach(Some(&buffer), 0, 0);
            surface.damage_buffer(0, 0, width, height);
            surface.commit();
            true
        }
    }

    #[cfg(any(test, feature = "testing"))]
    impl Dispatch<wayland_client::protocol::wl_shm::WlShm, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &wayland_client::protocol::wl_shm::WlShm,
            _: wayland_client::protocol::wl_shm::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // The formats it announces: ARGB8888 is every compositor's.
        }
    }

    #[cfg(any(test, feature = "testing"))]
    impl Dispatch<wayland_client::protocol::wl_shm_pool::WlShmPool, ()> for WaylandState {
        fn event(
            _: &mut Self,
            _: &wayland_client::protocol::wl_shm_pool::WlShmPool,
            _: wayland_client::protocol::wl_shm_pool::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // A pool has no events.
        }
    }

    #[cfg(any(test, feature = "testing"))]
    impl Dispatch<wayland_client::protocol::wl_buffer::WlBuffer, ()> for WaylandState {
        fn event(
            _: &mut Self,
            buffer: &wayland_client::protocol::wl_buffer::WlBuffer,
            event: wayland_client::protocol::wl_buffer::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // A blank frame is drawn once: the compositor is done with it.
            if let wayland_client::protocol::wl_buffer::Event::Release = event {
                buffer.destroy();
            }
        }
    }
}
