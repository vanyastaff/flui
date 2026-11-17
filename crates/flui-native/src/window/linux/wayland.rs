//! The Wayland window: an `xdg_toplevel` on a `wl_surface`, sized through its
//! `wp_viewport`, as the native Wayland host opens it.
//!
//! Two halves:
//!
//! - the pure ones, checked on every host: [`configure`], what a configure
//!   the compositor sends means for the window (its states, the size it
//!   takes, what the compositor lets it do); [`frame`], when it delivers a
//!   frame; [`decoration`], who draws its frame; [`client_frame`], the frame
//!   it draws itself where the compositor draws none; [`scale`], the scale its
//!   buffer is drawn at; [`presented`], when the compositor showed a commit;
//!   `foreign`, how a portal names it (read by tests until the window reference carries it);
//!   [`activation`], the token it asks to be activated with;
//! - `toplevel`: `WaylandWindow`, the `PlatformWindow` over the protocol
//!   objects, and the dispatch of those objects on the owner's queue
//!   (`owner::linux::wayland::WaylandState`).
//!
//! # What the compositor decides
//!
//! A Wayland client neither places its window nor reads where it is; it
//! cannot hide or show it either, and never hears that it was minimized. The
//! window's capabilities say so, and the commands are refused at admission.
//! It cannot focus it: an activation is a request with a token, which the
//! compositor grants or turns into a request for the user's attention
//! ([`activation`]). What the compositor tells the window comes in a configure:
//! the toplevel's states (maximized, fullscreen, activated, tiled, suspended)
//! and, when it sizes the window itself, the size. The window acknowledges a
//! configure on the owner's next turn and publishes what it says. A window is
//! shown once the compositor maps it, with its first frame, which the
//! surface's entering an output says; one opened with `visible: false` stays
//! unshown until it draws, since no command shows it later.
//!
//! # Size and scale
//!
//! The window's size is logical. Where the compositor offers `wp_viewporter`,
//! the surface's destination is the logical size and the buffer is drawn at
//! the scale the compositor prefers for the surface: a fractional one
//! (`wp_fractional_scale_v1`), else a whole one (`wl_surface` version 6),
//! else that of the output the surface entered ([`scale`]). Any buffer size
//! is then valid while a resize is under way. Without the viewporter the
//! buffer is drawn at scale 1 and the compositor scales it.
//!
//! # Outputs and occlusion
//!
//! The window is on the first output its surface entered that it has not
//! left, so a window the compositor moves to another output names that
//! output's display, whatever its scale. A window the compositor suspends
//! (covered, on a hidden workspace, its outputs off) is occluded and is
//! given no frame until it is shown again ([`frame`]).
//!
//! # The frame and the portals
//!
//! Where the compositor offers `xdg-decoration`, the window asks it for a
//! frame as its options and commands say, and has the frame the compositor
//! answers with. Where the compositor offers none (Weston, GNOME), or answers
//! that the frame is the client's, a window that wants a frame draws its own
//! on subsurfaces around its content: a title bar with its title and buttons,
//! and margins its edges are resized from ([`client_frame`]). Its geometry is
//! then the title bar and the content, its outer size includes the title bar,
//! and the size a configure gives loses the title bar to the content. A
//! window whose application draws the title bar (`WindowFrame::AppTitleBar`)
//! asks the compositor for no frame and draws the margins alone; the presses
//! on its content are answered from the chrome regions (ADR-0286 §11).
//!
//! Where the compositor offers `xdg-foreign`, the window is exported as it
//! opens, so a portal can parent its dialog to it; the handle reaches the
//! capability crates with the contract's window reference, and only tests
//! read it until then (`foreign`).
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

pub(crate) mod activation;
pub(crate) mod client_frame;
pub(crate) mod configure;
pub(crate) mod decoration;
pub(crate) mod frame;
pub(crate) mod presented;
pub(crate) mod scale;

// Read by tests alone until the contract's window reference carries the
// portal parent to the capability crates; that reader drops this `cfg`.
#[cfg(any(test, feature = "testing"))]
pub(crate) mod foreign {
    //! A Wayland window as another client names it (`xdg-foreign`): the
    //! handle the compositor gives the window's export, which a portal takes
    //! as the parent of the dialog it shows.

    #![forbid(unsafe_code)]

    /// A window as an XDG Desktop Portal names the parent of its dialog
    /// (XDG Desktop Portal, "Window identifiers"): `wayland:<handle>` for a
    /// Wayland window, from its export's handle.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct PortalParent(String);

    impl PortalParent {
        /// The parent of a Wayland window whose export the compositor handed
        /// `handle`.
        pub(crate) fn wayland(handle: &str) -> Self {
            Self(format!("wayland:{handle}"))
        }

        /// The identifier a portal takes.
        pub(crate) fn as_str(&self) -> &str {
            &self.0
        }
    }

    #[cfg(test)]
    mod tests {
        /// A portal names a Wayland parent by its export's handle, prefixed
        /// with the protocol.
        #[test]
        fn a_portal_names_a_window_by_its_export() {
            assert_eq!(
                super::PortalParent::wayland("a1b2-c3").as_str(),
                "wayland:a1b2-c3"
            );
        }
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod toplevel {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::rc::Rc;
    use std::rc::Weak as RcWeak;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Weak};

    use flui_foundation::geometry::{DevicePixelRatio, Size};
    use flui_platform::input_capabilities::InputCapabilities;
    use flui_platform::{
        AdmittedCommand, ApplyError, CloseDecision, Cursor, CursorError, CursorGrab,
        PlatformTextInput, PlatformWindow, SizeConstraints, Unsupported, UnsupportedReason,
        WindowCapabilities, WindowCommand, WindowExecutionState, WindowFrame, WindowGeometry,
        WindowHandler, WindowId, WindowInteraction, WindowLevel, WindowMode, WindowOptions,
        WindowState,
    };
    use flui_semantics::platform::PlatformAccessibility;
    use parking_lot::Mutex;
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
        RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
    };
    use wayland_client::WEnum;
    use wayland_client::protocol::wl_callback::{self, WlCallback};
    use wayland_client::protocol::wl_surface::{self, WlSurface};
    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{
        self, WpFractionalScaleV1,
    };
    use wayland_protocols::wp::presentation_time::client::wp_presentation_feedback::{
        self, WpPresentationFeedback,
    };
    use wayland_protocols::wp::viewporter::client::wp_viewport::{self, WpViewport};
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_token_v1::{
        self, XdgActivationTokenV1,
    };
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;
    use wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::{
        self, ZxdgToplevelDecorationV1,
    };
    use wayland_protocols::xdg::foreign::zv2::client::zxdg_exported_v2::{self, ZxdgExportedV2};
    use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
    use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};

    use web_time::Instant;

    use super::activation::{TokenRequest, Tokens, token_requests};
    use super::client_frame::surfaces::{ClientFrame, FrameGlobals, FrameRecord, TitleFonts};
    use super::client_frame::{self, Action, Frame, Look, Offers};
    use super::configure::{
        Activation, Sizing, ToplevelStates, WmCapabilities, capabilities, clamp, pointer_reported,
        protocol_extent,
    };
    use super::decoration::{Decoration, Drawn, FrameDrawer};
    #[cfg(any(test, feature = "testing"))]
    use super::foreign::PortalParent;
    use super::frame::{Pacing, Sight, first_live, waiting_on_callback};
    use super::presented;
    use super::scale::{self, BufferScale, ScaleTold, ToldScale, buffer_scale};
    use crate::executor::NativeExecutor;
    use crate::host::{self, ScreenBounds, WindowFacets, WindowPositions};
    use crate::host_window::{CoreHostWindow, HostWindow, OwnerThreadToken};
    use crate::owner::OpenWindowError;
    use crate::owner::frame_timing::{PresentationReports, rebased};
    use crate::owner::linux::PortalSlot;
    use crate::owner::linux::wayland::activation::ActivationToken;
    use crate::owner::linux::wayland::{
        DragWindow, LastInput, LiveOutputs, LoopWaker, LoopWindow, OutputFacts, OutputName,
        PointerOffer, PresentationTime, SeatWindow, WaylandDisplays, WaylandDrag, WaylandInput,
        WaylandLoop, WaylandState, WindowSeed, WindowTable,
    };
    use crate::owner::window_delivery::{Publication, Retirement, WindowDelivery};
    use crate::text::WindowTextInput;
    use crate::window::linux::class::AppClass;
    use crate::window::state::{
        ReportedFacts, SampledGeometry, WindowStateCell, not_carried_out, system_frame_or_none,
    };

    /// What the compositor said of one window since the owner last looked,
    /// recorded by the dispatch of the window's objects on the owner's queue,
    /// and what other threads asked of it. The user data of every object of
    /// the window, so it is `Send + Sync`; nothing here runs a handler.
    pub(crate) struct ToplevelRecord {
        protocol: Mutex<Told>,
        /// A frame was asked for.
        redraw: AtomicBool,
        /// When the frame callback that is out was armed: the compositor has
        /// not shown the last frame yet, and the next waits for it, for the
        /// grace at most (`frame::CALLBACK_GRACE`).
        callback_armed: Mutex<Option<Instant>>,
        /// The presentation feedbacks the surface asked for, until the
        /// compositor answers each or the window forgets it.
        feedbacks: Mutex<presented::CommitFeedbacks>,
        /// The handler granted a close request it was asked after the fact.
        close_granted: AtomicBool,
        /// The activation tokens the compositor and the launcher gave the
        /// window, which it has not handed back yet.
        tokens: Mutex<Tokens>,
        /// The compositor answered a frame callback: it has shown the window
        /// at least once, so the window can be activated.
        frame_shown: AtomicBool,
        /// How many activation tokens came, and how many the window handed
        /// back, for a test (`testing::wayland::compositor_report`).
        #[cfg(any(test, feature = "testing"))]
        tokens_counted: Mutex<(u32, u32)>,
        /// A test has every configure carry `suspended`, as a compositor of
        /// xdg-shell version 6 would (`testing::wayland::suspend`).
        #[cfg(any(test, feature = "testing"))]
        suspended_by_test: AtomicBool,
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
        /// The last toplevel configure's states as the protocol sent them,
        /// before they were decoded.
        #[cfg(any(test, feature = "testing"))]
        raw_states: Vec<u8>,
        /// How many `xdg_surface.configure` events came.
        #[cfg(any(test, feature = "testing"))]
        configures: u32,
        /// A complete configure, waiting to be acknowledged.
        configure: Option<Configure>,
        /// The compositor asked the window to close.
        close_requested: bool,
        /// The outputs the surface is on, in the order entered.
        outputs: Vec<OutputName>,
        /// The buffer scale the compositor prefers for the surface: the last
        /// one that was a ratio a screen can have (the hosts' one rule, at the
        /// event; a refused one leaves it).
        preferred_scale: ToldScale<i32>,
        /// The fractional scale the compositor prefers, as a numerator over
        /// 120, held the same way.
        fractional_scale: ToldScale<u32>,
        /// Who draws the frame, as the compositor last said.
        frame: Option<FrameDrawer>,
        /// The compositor answered the window's last frame request since the
        /// owner last looked: with a frame mode, or with a configure that
        /// carried none, which leaves the mode as it was (wlroots sends no
        /// mode it already sent).
        frame_answered: bool,
        /// The window asked for a frame mode and the compositor has not
        /// answered yet.
        frame_asked: bool,
        /// The last frame mode as the protocol sent it.
        #[cfg(any(test, feature = "testing"))]
        raw_frame_mode: Option<u32>,
        /// The handle of the window's export (`xdg-foreign`), which only
        /// tests read so far.
        #[cfg(any(test, feature = "testing"))]
        exported: Option<String>,
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
                callback_armed: Mutex::new(None),
                feedbacks: Mutex::default(),
                close_granted: AtomicBool::new(false),
                tokens: Mutex::new(Tokens::default()),
                frame_shown: AtomicBool::new(false),
                #[cfg(any(test, feature = "testing"))]
                tokens_counted: Mutex::new((0, 0)),
                #[cfg(any(test, feature = "testing"))]
                suspended_by_test: AtomicBool::new(false),
            }
        }

        /// What the owner carries out now: the configure and the close
        /// request are taken, the rest is read.
        fn take(&self) -> Told {
            let mut told = self.protocol.lock();
            let taken = told.clone();
            told.configure = None;
            told.close_requested = false;
            told.frame_answered = false;
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
        /// The frame's negotiation, where the compositor offers one.
        decoration: Option<ZxdgToplevelDecorationV1>,
        /// The fractional scale's reports, where the compositor gives them.
        fractional_scale: Option<WpFractionalScaleV1>,
        /// The export another client names the window by.
        exported: Option<ZxdgExportedV2>,
    }

    impl Roles {
        /// Destroy every object, the decoration before its toplevel (the
        /// protocol's order) and the export first, so the handle names no
        /// window once its role is gone.
        fn destroy(self) {
            if let Some(exported) = self.exported {
                exported.destroy();
            }
            if let Some(decoration) = self.decoration {
                decoration.destroy();
            }
            if let Some(fractional_scale) = self.fractional_scale {
                fractional_scale.destroy();
            }
            if let Some(viewport) = self.viewport {
                viewport.destroy();
            }
            self.toplevel.destroy();
            self.xdg_surface.destroy();
        }
    }

    /// One window as its window-level positions are made for it
    /// (`host::Window::Ref`). No position of the Wayland table needs anything
    /// of the window yet, so it carries nothing; the text and drag positions
    /// add what they read. Public where it is defined, as the host's table
    /// names it, and nameable only inside the crate.
    pub struct SurfaceRef(());

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
        /// The window last asked for a frame.
        decoration_requested: bool,
        /// The compositor answered that request.
        decoration_answered: bool,
        /// The title bar of the application's the window asked for, which asks
        /// the compositor for no frame and keeps the margins of its own.
        app_title_bar: Option<flui_platform::AppTitleBar>,
        /// The title bar's height the size limits were last sent with.
        limits_header: f64,
        /// The window geometry last sent, once the window drew a frame of its
        /// own: from then on it is kept current.
        geometry: Option<client_frame::Rect>,
        told: Told,
    }

    impl Applied {
        /// The window's frame as asked for and answered; `draws_own` where
        /// the window can draw a frame of its own.
        const fn decoration(&self, offered: bool, draws_own: bool) -> Decoration {
            Decoration {
                offered,
                draws_own,
                requested: self.decoration_requested,
                told: self.told.frame,
                answered: self.decoration_answered,
            }
        }

        /// The frame the window draws itself with `states` and its content
        /// now; `None` where it draws none. Around a title bar of the
        /// application's it is the margins alone.
        fn own_frame(
            &self,
            decoration: Decoration,
            states: ToplevelStates,
            viewported: bool,
        ) -> Option<Frame> {
            let app_margins = decoration.app_margins(self.app_title_bar.is_some());
            (app_margins || decoration.drawn() == Drawn::Client).then_some(Frame {
                content: self.sizing.content(),
                states,
                resizable: self.resizable,
                offers: Offers::from(self.told.wm),
                title_bar: !app_margins,
                viewported,
            })
        }

        /// The height of the title bar the window draws itself above its
        /// content, in logical pixels; zero where it draws none.
        fn own_header(&self, decoration: Decoration, viewported: bool) -> f64 {
            self.own_frame(decoration, self.sizing.states(), viewported)
                .map_or(0.0, |frame| frame.header())
        }

        /// The frame the window reports: the application's title bar where
        /// the window draws the margins around it, otherwise whether it has
        /// a frame at all.
        fn reported_frame(&self, decoration: Decoration) -> WindowFrame {
            match self.app_title_bar {
                Some(bar) if decoration.app_margins(true) => WindowFrame::AppTitleBar(bar),
                _ => system_frame_or_none(decoration.decorated()),
            }
        }
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
        table: RcWeak<WindowTable>,
        waker: Weak<LoopWaker>,
        viewported: bool,
        /// The compositor offers to draw the window's frame.
        offers_decorations: bool,
        /// What the window draws a frame of its own with; `None` where the
        /// compositor offers no subsurfaces or shared memory.
        frame_globals: Option<FrameGlobals>,
        /// The frame the window draws itself, made the first time it draws
        /// one.
        client_frame: RefCell<Option<ClientFrame>>,
        /// What the pointer does over that frame, which the seats' input
        /// records.
        frame_record: Rc<FrameRecord>,
        /// The fonts its title is drawn with, shared by the host's windows.
        title_fonts: TitleFonts,
        /// The loop's background work, which reads the host's fonts.
        executor: NativeExecutor,
        /// `wp_presentation`, through which the window's surface target asks
        /// when each commit is shown; `None` where the compositor tells
        /// nothing.
        presentation: Option<PresentationTime>,
        /// What the window asks the compositor to activate it through; `None`
        /// where it takes no activation request.
        activation: Option<XdgActivationV1>,
        /// The last user input, which an activation token names.
        last_input: LastInput,
        live: LiveOutputs,
        /// The seats' input, which the window joined as it opened.
        input: Arc<dyn WaylandInput>,
        /// Drag and drop, which the window joined as it opened.
        drag: Arc<dyn WaylandDrag>,
        applied: RefCell<Applied>,
        closed: Cell<bool>,
        /// Whether the accessibility bridge was last told the window is the
        /// active one.
        bridge_focus: Cell<bool>,
        /// Where the accessibility bridge was last told the window is.
        bridge_bounds: Cell<Option<ScreenBounds>>,
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
        live: &'a LiveOutputs,
        hovered: bool,
        viewported: bool,
        offers_decorations: bool,
        draws_own: bool,
        reports_presentation: bool,
        activation: Activation,
        /// What the compositor offers for the window's pointer.
        pointer: PointerOffer,
        /// What the window holds of the pointer, as the seats' input holds it.
        cursor_grab: CursorGrab,
        closed: bool,
        /// The compositor answered a frame callback of the window's: it
        /// mapped the window at least once.
        shown: bool,
    }

    impl Sample<'_> {
        /// The output the window is on, and the scale its buffer is drawn at.
        fn output_and_scale(&self) -> (Option<OutputFacts>, BufferScale) {
            let applied = self.applied;
            let entered = first_live(&applied.told.outputs, |output| self.live.contains(output));
            let output = entered.and_then(|output| self.displays.output(output));
            let scale = buffer_scale(
                self.viewported,
                ScaleTold {
                    fractional: applied.told.fractional_scale.ratio(),
                    preferred: applied.told.preferred_scale.ratio(),
                    output: output.map(|output| output.scale),
                },
                // The content is the one surface drawn at the scale: the
                // frame's title bar spans its width, and its margins are a
                // pixel the viewport stretches (`client_frame::margin_size`).
                applied.sizing.content(),
            );
            (output, scale)
        }

        /// The read these facts make; a size that is no extent costs it its
        /// geometry and nothing else.
        fn state(&self) -> crate::window::state::Sample {
            let applied = self.applied;
            let entered = first_live(&applied.told.outputs, |output| self.live.contains(output));
            let (output, scale) = self.output_and_scale();
            let decoration = applied.decoration(self.offers_decorations, self.draws_own);
            let content = applied.sizing.content();
            let states = applied.sizing.states();
            // The outer size is the window's geometry: the content, and the
            // title bar of a frame it draws itself.
            let header = applied.own_header(decoration, self.viewported);
            let outer = Size::new(content.width, content.height + header);
            let geometry = WindowGeometry::try_new(None, outer).and_then(|outer| {
                Ok(SampledGeometry {
                    outer,
                    content: flui_platform::WindowExtent::try_new(content)?,
                    normal: None,
                })
            });
            crate::window::state::Sample::read(geometry, |state| {
                let state = state
                    .with_mode(crate::window::state::mode_of(states.mode_flags()))
                    // A toplevel has no hidden state of its own: it is shown
                    // until it closes.
                    .with_visible(!self.closed)
                    .with_title(Some(applied.title.clone()))
                    .with_constraints(applied.constraints)
                    .with_resizable(applied.resizable)
                    .with_frame(applied.reported_frame(decoration))
                    .with_display(output.map(|output| output.display));
                ReportedFacts {
                    metrics: scale::metrics(content, scale),
                    brightness: self.portal.appearance(),
                    insets: flui_platform::WindowInsets::ZERO,
                    execution: WindowExecutionState::Running,
                    // What the compositor says it draws: a surface it maps (a
                    // configured window with a frame) enters an output, and one
                    // it does not draw (covered, on a hidden workspace, its
                    // outputs off, minimized) is suspended. A configured window
                    // the compositor has not shown yet enters no output until it
                    // draws its first frame, so it is not occluded: it would
                    // otherwise never be given that frame.
                    occluded: Sight {
                        configured: applied.configured,
                        shown: self.shown,
                        on_output: entered.is_some(),
                        suspended: states.suspended,
                    }
                    .occluded(),
                    hovered: self.hovered,
                    // xdg-shell has one `activated` state for both.
                    focus: flui_platform::WindowFocus::reported(states.activated, states.activated),
                    // The configure says when the user resizes the window by
                    // its frame; a move the compositor runs for the user is
                    // not read yet (docs/gaps.md).
                    interaction: if states.resizing {
                        WindowInteraction::UserResizing
                    } else {
                        WindowInteraction::Unknown
                    },
                    capabilities: pointer_reported(
                        presented::reported(
                            capabilities(
                                applied.told.wm,
                                decoration.absence(),
                                decoration.app_title_bar_absence(),
                                states,
                                self.activation,
                            ),
                            self.reports_presentation,
                        ),
                        self.pointer,
                    ),
                    cursor_grab: self.cursor_grab,
                    // xdg-shell keeps no window above or below the others.
                    level: WindowLevel::Normal,
                }
                .onto(state)
            })
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
            // Refused before an id is taken, for what a command is refused for.
            let flui_platform::OpeningGeometry {
                content,
                constraints,
                ..
            } = options.geometry()?;
            let WindowSeed {
                id,
                protocol,
                displays,
                portal,
                table,
                signal,
                waker,
                notices,
                frames,
                input,
                drag,
                executor,
            } = owner.seed()?;
            let record = Arc::new(ToplevelRecord::new());
            let queue = protocol.queue.clone();
            let frame_globals = match (&protocol.subcompositor, &protocol.shm) {
                (Some(subcompositor), Some(shm)) => Some(FrameGlobals {
                    compositor: protocol.compositor.clone(),
                    subcompositor: subcompositor.clone(),
                    shm: shm.clone(),
                    viewporter: protocol.viewporter.clone(),
                    queue: queue.clone(),
                }),
                _ => None,
            };
            let surface = protocol
                .compositor
                .create_surface(&queue, Arc::clone(&record));
            let xdg_surface =
                protocol
                    .wm_base
                    .get_xdg_surface(&surface, &queue, Arc::clone(&record));
            let toplevel = xdg_surface.get_toplevel(&queue, Arc::clone(&record));
            let viewport = protocol
                .viewporter
                .as_ref()
                .map(|viewporter| viewporter.get_viewport(&surface, &queue, ()));
            // A title bar of the application's the window cannot take (it
            // draws no frame of its own) opens with the system's frame; one it
            // takes asks the compositor for none and keeps its own margins.
            let frame = options
                .frame
                .at_open(&<Self as host::Window<WaylandLoop>>::capabilities(
                    owner, windows,
                ));
            let decorated_at_open = frame.has_system_title_bar();
            let app_title_bar = match frame {
                WindowFrame::AppTitleBar(bar) => Some(bar),
                _ => None,
            };
            // Asked before the first commit, so the first configure says who
            // draws the frame.
            let decoration = protocol.decoration.as_ref().map(|manager| {
                let decoration =
                    manager.get_toplevel_decoration(&toplevel, &queue, Arc::clone(&record));
                decoration.set_mode(frame_mode(decorated_at_open));
                decoration
            });
            let fractional_scale = protocol
                .fractional_scale
                .as_ref()
                .map(|manager| manager.get_fractional_scale(&surface, &queue, Arc::clone(&record)));
            // The handle comes with the round trip below, so a portal can
            // name the window as soon as it opened.
            let exported = protocol
                .exporter
                .as_ref()
                .map(|exporter| exporter.export_toplevel(&surface, &queue, Arc::clone(&record)));
            // Before the first commit, so the shell matches the window to its
            // desktop entry from its first configure.
            toplevel.set_app_id(windows.class.name().to_owned());
            toplevel.set_title(options.title.clone());
            let size = clamp(content.get(), constraints);
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
                decoration,
                fractional_scale,
                exported,
            };
            // A compositor that offers no frames leaves the frame to the
            // window from the start: its limits carry the title bar already.
            let applied = Applied {
                title: options.title.clone(),
                sizing: Sizing::new(size),
                constraints,
                resizable: options.resizable,
                configured: false,
                leaving_maximized: false,
                decoration_requested: decorated_at_open,
                app_title_bar,
                decoration_answered: false,
                limits_header: 0.0,
                geometry: None,
                told: Told::default(),
            };
            let applied = Applied {
                limits_header: applied
                    .own_frame(
                        applied.decoration(protocol.decoration.is_some(), frame_globals.is_some()),
                        ToplevelStates {
                            fullscreen: options.mode == WindowMode::Fullscreen,
                            ..ToplevelStates::default()
                        },
                        protocol.viewporter.is_some(),
                    )
                    .map_or(0.0, |frame| frame.header()),
                ..applied
            };
            send_limits(
                &roles.toplevel,
                size,
                constraints,
                options.resizable,
                applied.limits_header,
            );
            if let Some(viewport) = &roles.viewport {
                set_destination(viewport, size);
            }
            // The first commit carries no buffer: the compositor answers it
            // with the first configure, which the window waits for.
            surface.commit();
            let surface = Arc::new(SurfaceHandle(surface));
            let window = Rc::new(Self {
                id,
                facets: windows.positions.for_window(&SurfaceRef(())),
                delivery: Arc::new(WindowDelivery::counting_frames(
                    handler,
                    InputCapabilities::UNAVAILABLE,
                    frames,
                )),
                // Until the first configure, below: the size asked for, held
                // to the limits asked for.
                state: Arc::new(WindowStateCell::new({
                    let asked = flui_platform::WindowExtent::try_new(size)
                        .expect("BUG: a checked size held to checked limits is an extent");
                    WindowState::new(WindowGeometry::unplaced(asked), asked)
                })),
                record,
                surface,
                roles: RefCell::new(Some(roles)),
                connection: protocol.connection,
                queue,
                viewported: protocol.viewporter.is_some(),
                offers_decorations: protocol.decoration.is_some(),
                frame_globals,
                client_frame: RefCell::new(None),
                frame_record: Rc::new(FrameRecord::default()),
                title_fonts: windows.title_fonts.clone(),
                executor,
                presentation: protocol.presentation,
                activation: protocol.activation,
                last_input: protocol.last_input,
                live: protocol.live,
                displays,
                portal,
                table,
                waker,
                input,
                drag,
                applied: RefCell::new(applied),
                closed: Cell::new(false),
                bridge_focus: Cell::new(false),
                bridge_bounds: Cell::new(None),
                #[cfg(any(test, feature = "testing"))]
                shm: protocol.shm,
            });
            // The input gives the window its capabilities before the handler
            // hears its snapshot, and its input from the compositor's first
            // event.
            let state = Arc::downgrade(&window.state);
            window.drag.attach(DragWindow {
                id,
                surface: window.surface.0.clone(),
                delivery: Arc::clone(&window.delivery),
            });
            window.input.attach(SeatWindow {
                id,
                surface: window.surface.0.clone(),
                delivery: Arc::clone(&window.delivery),
                scale: Arc::new(move || {
                    state
                        .upgrade()
                        .and_then(|state| state.published().metrics)
                        .map_or(1.0, |metrics| metrics.scale().get())
                }),
                frame: Rc::clone(&window.frame_record) as _,
                text: Arc::clone(&window.facets.text).seat_text(),
            });
            // From here a failure retires the window like any other close.
            if let Err(error) = owner.roundtrip() {
                window.release();
                return Err(error);
            }
            window.carry_out_protocol();
            // The window's first snapshot holds the first configure; the
            // handler hears nothing before its capability snapshot.
            let _ = window.state.refresh(&window.delivery, || window.sample());
            notices.mark_stale_on_wake(&window.state);
            window
                .delivery
                .publish_through_owner(&window, WaylandWindow::read_state);
            window.delivery.resume_through_signal(&signal);
            match owner.register(id, Rc::clone(&window) as Rc<dyn LoopWindow>) {
                // The launcher's token activates the first window held once
                // it is shown; a window that failed to open leaves it.
                Ok(launch_token) => {
                    if let Some(token) = launch_token {
                        window.record.tokens.lock().grant(token);
                    }
                }
                Err(error) => {
                    window.release();
                    return Err(error);
                }
            }
            Ok(window)
        }

        /// The window's state as the compositor and the owner describe it now.
        fn sample(&self) -> crate::window::state::Sample {
            let applied = self.applied.borrow();
            Sample {
                applied: &applied,
                displays: self.displays.as_ref(),
                portal: &self.portal,
                live: &self.live,
                hovered: self.delivery.hovered(),
                viewported: self.viewported,
                offers_decorations: self.offers_decorations,
                draws_own: self.frame_globals.is_some(),
                reports_presentation: self.presentation.is_some(),
                activation: if self.activation.is_some() {
                    Activation::Offered
                } else {
                    Activation::Missing
                },
                pointer: self.input.pointer_offer(),
                cursor_grab: self.input.pointer_grab(self.id),
                closed: self.closed.get(),
                shown: self.record.frame_shown.load(Ordering::Acquire),
            }
            .state()
        }

        /// The frame as asked for and answered now.
        fn decoration(&self, applied: &Applied) -> Decoration {
            applied.decoration(self.offers_decorations, self.frame_globals.is_some())
        }

        /// The scale the window's buffers are drawn at now.
        fn buffer_scale(&self, applied: &Applied) -> BufferScale {
            Sample {
                applied,
                displays: self.displays.as_ref(),
                portal: &self.portal,
                live: &self.live,
                hovered: false,
                viewported: self.viewported,
                offers_decorations: self.offers_decorations,
                draws_own: self.frame_globals.is_some(),
                reports_presentation: false,
                activation: Activation::Missing,
                pointer: PointerOffer::default(),
                cursor_grab: CursorGrab::None,
                closed: false,
                shown: false,
            }
            .output_and_scale()
            .1
        }

        /// Show the frame the window draws itself as it stands now: made the
        /// first time one is drawn, laid out for the content, the states and
        /// what the compositor offers, and drawn again where it changed; and
        /// the window geometry and the size limits that follow from it. Where
        /// the window draws none, a frame it drew is hidden. Returns whether
        /// the content surface needs a commit to show it. No handler runs.
        fn sync_frame(&self) -> bool {
            let Some(globals) = &self.frame_globals else {
                return false;
            };
            if self.closed.get() {
                return false;
            }
            let (frame, scale, title, resizable) = {
                let applied = self.applied.borrow();
                let frame = applied.own_frame(
                    self.decoration(&applied),
                    applied.sizing.states(),
                    self.viewported,
                );
                (
                    frame,
                    self.buffer_scale(&applied),
                    applied.title.clone(),
                    applied.resizable,
                )
            };
            let mut committed = false;
            {
                let mut client_frame = self.client_frame.borrow_mut();
                if client_frame.is_none() && frame.is_some() {
                    let fonts = self.title_fonts.get(&self.executor, &self.waker);
                    *client_frame = Some(ClientFrame::new(
                        &self.surface.0,
                        globals.clone(),
                        Rc::clone(&self.frame_record),
                        &fonts,
                    ));
                }
                if let Some(client_frame) = client_frame.as_mut() {
                    let look = Look {
                        dark: self.portal.appearance().is_dark(),
                        active: frame.is_some_and(|frame| frame.states.activated),
                    };
                    committed = client_frame.update(frame, scale, look, &title);
                }
            }
            // The geometry, once the window drew a frame, is kept current:
            // the compositor holds the last one sent.
            let geometry = frame.map(|frame| frame.geometry()).or_else(|| {
                let applied = self.applied.borrow();
                applied.geometry.map(|_| {
                    let content = applied.sizing.content();
                    client_frame::Rect {
                        x: 0.0,
                        y: 0.0,
                        width: content.width,
                        height: content.height,
                    }
                })
            });
            let header = frame.map_or(0.0, |frame| frame.header());
            let (geometry_changed, limits_changed) = {
                let mut applied = self.applied.borrow_mut();
                let geometry_changed = geometry.is_some() && applied.geometry != geometry;
                if geometry_changed {
                    applied.geometry = geometry;
                }
                let limits_changed = applied.limits_header != header;
                applied.limits_header = header;
                (geometry_changed, limits_changed)
            };
            if let (true, Some(geometry), Some(roles)) =
                (geometry_changed, geometry, self.roles.borrow().as_ref())
            {
                roles.xdg_surface.set_window_geometry(
                    client_frame::rounded(geometry.x),
                    client_frame::rounded(geometry.y),
                    protocol_extent(geometry.width).max(1),
                    protocol_extent(geometry.height).max(1),
                );
            }
            if limits_changed {
                let applied = self.applied.borrow();
                if let Some(roles) = self.roles.borrow().as_ref() {
                    send_limits(
                        &roles.toplevel,
                        applied.sizing.content(),
                        applied.constraints,
                        resizable,
                        header,
                    );
                }
            }
            committed || geometry_changed || limits_changed
        }

        /// Carry out what the pointer asked of the window's own frame: a
        /// move, a resize or the window menu with the seat and serial of the
        /// press that asked, a mode, or a close its handler is asked about.
        /// Runs the handler (for a close) with nothing borrowed.
        fn carry_out_frame(&self) {
            let Some(asked) = self.frame_record.take_asked() else {
                return;
            };
            if asked.action == Action::Close {
                #[cfg(any(test, feature = "testing"))]
                self.frame_record.carried(asked.action, asked.serial);
                self.ask_close();
                return;
            }
            let roles = self.roles.borrow();
            let Some(toplevel) = roles.as_ref().map(|roles| &roles.toplevel) else {
                return;
            };
            match asked.action {
                Action::Move => toplevel._move(&asked.seat, asked.serial),
                Action::Resize(edge) => {
                    if let Ok(edge) = xdg_toplevel::ResizeEdge::try_from(edge.protocol()) {
                        toplevel.resize(&asked.seat, asked.serial, edge);
                    }
                }
                Action::Menu { x, y } => toplevel.show_window_menu(&asked.seat, asked.serial, x, y),
                Action::Minimize => toplevel.set_minimized(),
                Action::Maximize => toplevel.set_maximized(),
                Action::Restore => toplevel.unset_maximized(),
                Action::Close => {}
            }
            #[cfg(any(test, feature = "testing"))]
            self.frame_record.carried(asked.action, asked.serial);
        }

        /// Read the window again for its state publisher, announcing
        /// nothing: `WindowDelivery::dispatch_state_changed` does.
        fn read_state(&self) -> Publication {
            self.state.refresh(&self.delivery, || self.sample())
        }

        /// The compositor reported a change: read the window again and tell
        /// its handler when the snapshot changed.
        fn refresh_state(&self) {
            self.state.publish_report(&self.delivery, || self.sample());
        }

        /// Carry out what the compositor told the window since the last
        /// turn: acknowledge a configure and take its size. No handler runs.
        /// Returns whether the compositor asked the window to close.
        fn carry_out_protocol(&self) -> bool {
            // An output the compositor removed while the surface was on it
            // sent no `leave`: it is left here.
            self.record
                .protocol
                .lock()
                .outputs
                .retain(|output| self.live.contains(*output));
            let told = self.record.take();
            let close_requested = told.close_requested;
            let configure = told.configure;
            let (resized, activated) = {
                let mut applied = self.applied.borrow_mut();
                let first = !applied.configured;
                if told.frame_answered {
                    applied.decoration_answered = true;
                }
                applied.told = told;
                let decoration = self.decoration(&applied);
                let resized = configure.is_some_and(|configure| {
                    if let Some(roles) = self.roles.borrow().as_ref() {
                        roles.xdg_surface.ack_configure(configure.serial);
                    }
                    applied.configured = true;
                    let constraints = applied.constraints;
                    // The configure sizes the window geometry: a title bar
                    // the window draws itself comes off the content's height.
                    let (width, height) = applied
                        .own_frame(decoration, configure.states, self.viewported)
                        .map_or((configure.width, configure.height), |frame| {
                            frame.content_of(configure.width, configure.height)
                        });
                    let resized =
                        applied
                            .sizing
                            .configure(width, height, configure.states, constraints);
                    resized || first
                });
                (resized, applied.sizing.states().activated)
            };
            if resized {
                self.size_surface();
                // A configured or resized window needs a frame at its size.
                self.record.redraw.store(true, Ordering::Release);
            }
            let frame_changed = self.sync_frame();
            if (configure.is_some() || frame_changed) && self.viewported {
                // The acknowledged configure takes effect with a commit. The
                // viewport already gives the surface the configured size, so
                // the commit is valid with the last buffer, and the
                // compositor's next configure is measured from this one, not
                // from whatever the window last drew. The frame's pieces,
                // synchronized with it, show with the same commit.
                self.surface.0.commit();
            } else if frame_changed {
                // Without a viewport the content's next buffer shows them.
                self.record.redraw.store(true, Ordering::Release);
            }
            if let Some(configure) = configure {
                self.leave_restored_maximized(configure.states);
            }
            if let Some(table) = self.table.upgrade() {
                table.activation(self.id, activated);
            }
            self.tell_bridge(activated);
            self.tell_bridge_bounds();
            self.hand_back_tokens();
            close_requested
        }

        /// Ask the compositor for a token to activate the window with, naming
        /// the last user input the seats reported; the window hands it back
        /// once it comes ([`Self::hand_back_tokens`]). With no input to name,
        /// the token asks for the user's attention rather than the focus.
        fn ask_activation(&self) -> Result<(), ApplyError> {
            let Some(activation) = &self.activation else {
                return Err(not_carried_out(&WindowCommand::Activate));
            };
            let token = activation.get_activation_token(&self.queue, Arc::clone(&self.record));
            let requests =
                token_requests(self.last_input.latest(), Proxy::is_alive, Proxy::is_alive);
            for request in requests {
                match request {
                    TokenRequest::SetSerial { serial, seat } => token.set_serial(serial, &seat),
                    TokenRequest::SetSurface(surface) => token.set_surface(&surface),
                    TokenRequest::Commit => token.commit(),
                }
            }
            Ok(())
        }

        /// Hand the compositor back the tokens the window was given, each
        /// once, once it has shown the window: a surface it never mapped is
        /// not activated. A window whose compositor takes no activation drops
        /// the launcher's token.
        fn hand_back_tokens(&self) {
            let mapped = {
                let applied = self.applied.borrow();
                applied.configured
                    && (!applied.told.outputs.is_empty()
                        || self.record.frame_shown.load(Ordering::Acquire))
            };
            let due = self.record.tokens.lock().take_due(mapped);
            if let Some(activation) = &self.activation {
                #[cfg(any(test, feature = "testing"))]
                {
                    let used = u32::try_from(due.len()).unwrap_or(u32::MAX);
                    let mut counted = self.record.tokens_counted.lock();
                    counted.1 = counted.1.saturating_add(used);
                }
                for token in due {
                    activation.activate(token.into_string(), &self.surface.0);
                }
            }
        }

        /// Tell the accessibility bridge whether the compositor activates the
        /// window, when that changed since it was last told: a screen reader
        /// follows the active window.
        fn tell_bridge(&self, activated: bool) {
            if self.bridge_focus.replace(activated) != activated {
                self.facets.accessibility.focus_changed(activated);
            }
        }

        /// Tell the accessibility bridge where the content lies in the
        /// window, when that changed since it was last told. The compositor
        /// tells a client nothing of where its window is, so the window is
        /// at the origin and the content below the title bar it draws
        /// itself. Nothing is told before the content has a drawable size.
        fn tell_bridge_bounds(&self) {
            let bounds = {
                let applied = self.applied.borrow();
                let header = applied.own_header(self.decoration(&applied), self.viewported);
                let scale = self.buffer_scale(&applied);
                scale::metrics(applied.sizing.content(), scale)
                    .and_then(|metrics| scale::unplaced_bounds(metrics, header))
            };
            if let Some(bounds) = bounds
                && self.bridge_bounds.replace(Some(bounds)) != Some(bounds)
            {
                self.facets.accessibility.bounds_changed(bounds);
            }
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

        /// What decides whether the window delivers its frame now.
        fn pacing(&self) -> Pacing {
            let applied = self.applied.borrow();
            Pacing {
                asked: self.record.redraw.load(Ordering::Acquire),
                configured: applied.configured,
                suspended: applied.sizing.states().suspended,
                armed: *self.record.callback_armed.lock(),
            }
        }

        /// The surface's destination: the content size.
        fn size_surface(&self) {
            let content = self.applied.borrow().sizing.content();
            if let Some(viewport) = self
                .roles
                .borrow()
                .as_ref()
                .and_then(|roles| roles.viewport.as_ref())
            {
                set_destination(viewport, content);
            }
        }

        /// Show what a command changed of the frame the window draws itself
        /// (its title, its buttons, whether there is one) with a commit.
        fn show_frame_change(&self) {
            if self.sync_frame() {
                if self.viewported {
                    self.surface.0.commit();
                } else {
                    self.record.redraw.store(true, Ordering::Release);
                }
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
                    applied.limits_header,
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

        /// Leave the seats' input, take down the frame the window drew, let
        /// go of the handler, then destroy the role objects.
        fn retire(&self) {
            // No later request names a serial the closed window took.
            self.last_input.surface_gone(&self.surface.0);
            self.input.detach(self.id);
            self.drag.detach(self.id);
            if let Some(client_frame) = self.client_frame.borrow_mut().take() {
                client_frame.destroy();
            }
            let roles = self.roles.borrow_mut().take();
            self.delivery.clear_then(move || {
                if let Some(roles) = roles {
                    roles.destroy();
                }
            });
        }
    }

    /// The size limits `toplevel` is held to: `constraints`, or `size` both
    /// ways for a window that is not resizable, each the content's plus the
    /// `header` of a title bar the window draws itself, as the limits bound
    /// the window geometry.
    fn send_limits(
        toplevel: &XdgToplevel,
        size: Size<f64>,
        constraints: SizeConstraints,
        resizable: bool,
        header: f64,
    ) {
        let extent = |size: Option<Size<f64>>| {
            size.map_or((0, 0), |size: Size<f64>| {
                let height = protocol_extent(size.height);
                (
                    protocol_extent(size.width),
                    if height > 0 {
                        height.saturating_add(client_frame::rounded(header))
                    } else {
                        height
                    },
                )
            })
        };
        let (min, max) = if resizable {
            (
                extent(constraints.min.map(flui_platform::WindowExtent::get)),
                extent(constraints.max.map(flui_platform::WindowExtent::get)),
            )
        } else {
            (extent(Some(size)), extent(Some(size)))
        };
        toplevel.set_min_size(min.0, min.1);
        toplevel.set_max_size(max.0, max.1);
    }

    /// The frame mode a window asks the compositor for.
    const fn frame_mode(decorated: bool) -> zxdg_toplevel_decoration_v1::Mode {
        if decorated {
            zxdg_toplevel_decoration_v1::Mode::ServerSide
        } else {
            zxdg_toplevel_decoration_v1::Mode::ClientSide
        }
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
            if let Some(client_frame) = self.client_frame.get_mut().take() {
                client_frame.destroy();
            }
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
        /// Where each commit's presentation is asked for and reported;
        /// `None` where the compositor tells nothing.
        presentation: Option<PresentedTo>,
    }

    /// What a surface's presentation feedback reports to: the clock its
    /// times are on, and the window's reports, which its next frame request
    /// carries.
    #[derive(Clone)]
    pub(crate) struct PresentedTo {
        time: PresentationTime,
        reports: PresentationReports,
    }

    /// One commit's presentation feedback: where it reports, and the frame
    /// the commit shows, which its report names. The user data of each
    /// `wp_presentation_feedback`, so a report goes to the frame of its own
    /// commit, never to the newest one made.
    pub(crate) struct PresentedCommit {
        to: PresentedTo,
        /// The window's record, whose feedbacks keep the frame by `number`.
        record: Arc<ToplevelRecord>,
        /// The feedback's number among the surface's
        /// ([`presented::CommitFeedbacks`]).
        number: u64,
    }

    /// A frame callback's user data: the window's record, and the feedback
    /// request it confirms committed when it fires
    /// ([`presented::CommitFeedbacks::confirm`]), `None` where the window
    /// asks for none.
    pub(crate) struct ArmedCallback {
        record: Arc<ToplevelRecord>,
        confirms: Option<u64>,
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
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            if self.retirement.is_retired() {
                return Err(HandleError::Unavailable);
            }
            wayland_display(&self.connection)
        }
    }

    /// The Wayland host's display connection (ADR-0305 §2): the connection
    /// its windows are made on, so its handle is every window target's.
    pub(crate) struct WaylandDisplay(pub(crate) Connection);

    impl HasDisplayHandle for WaylandDisplay {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            wayland_display(&self.0)
        }
    }

    impl flui_platform::DisplayConnection for WaylandDisplay {}

    /// The `wl_display` of `connection`, borrowed for as long as the
    /// connection is.
    #[expect(
        unsafe_code,
        reason = "a raw display handle is borrowed from the connection it is made from"
    )]
    fn wayland_display(connection: &Connection) -> Result<DisplayHandle<'_>, HandleError> {
        let backend: wayland_backend::client::Backend = connection.backend();
        let display =
            NonNull::new(backend.display_ptr().cast::<c_void>()).ok_or(HandleError::Unavailable)?;
        let raw = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display));
        // SAFETY: `display` is the `wl_display` of `connection`; libwayland
        // disconnects it only when the last clone of the connection drops,
        // and the borrow cannot outlive the `&Connection` it is made from.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }

    impl flui_platform::SurfaceTarget for WaylandSurface {
        /// Arms the surface's frame callback with the commit the present
        /// makes: the next frame the window delivers waits until the
        /// compositor has shown this one, or the grace after which a present
        /// that never committed is given up (`frame::CALLBACK_GRACE`). Returns
        /// at once; one callback is out at a time. Where the compositor offers
        /// `wp_presentation`, every present of a frame also asks when its
        /// commit is shown ([`presented`]), with the frame the report will
        /// name; a present that shows no frame asks nothing.
        fn pre_present_notify(
            &self,
            frame: Option<flui_foundation::FrameStamp>,
            _: flui_platform::SwapChainStatistics,
        ) {
            if self.retirement.is_retired() {
                return;
            }
            if let Some(presented) = &self.presentation {
                presented.reports.presenting(frame);
                let number = self.record.feedbacks.lock().request(frame);
                if let Some(number) = number {
                    let _feedback = presented.time.manager.feedback(
                        &self.surface.0,
                        &self.queue,
                        PresentedCommit {
                            to: presented.clone(),
                            record: Arc::clone(&self.record),
                            number,
                        },
                    );
                }
            }
            let now = Instant::now();
            {
                let mut armed = self.record.callback_armed.lock();
                if waiting_on_callback(*armed, now) {
                    return;
                }
                *armed = Some(now);
            }
            let confirms = self.record.feedbacks.lock().arm();
            let _callback = self.surface.0.frame(
                &self.queue,
                ArmedCallback {
                    record: Arc::clone(&self.record),
                    confirms,
                },
            );
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

        /// Wayland states no gesture distances; the consumer keeps its own.
        fn gesture_geometry(
            &self,
        ) -> Result<Option<flui_platform::GestureGeometry>, flui_platform::PreferenceQueryError>
        {
            Ok(None)
        }

        /// A Wayland client learns neither where its toplevels stand nor
        /// which is on top.
        fn window_at(&self, _: flui_foundation::geometry::Point<f64>) -> flui_platform::WindowAt {
            flui_platform::WindowAt::Unknown
        }

        /// A toplevel is mapped by the commit of its first frame, so there is
        /// no reveal to defer.
        fn reveal_after_first_frame(&self) {}

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
                presentation: self.presentation.clone().map(|time| PresentedTo {
                    time,
                    reports: self.delivery.presentation_reports(),
                }),
            })
        }

        /// A revision this read publishes is announced at the owner's turn,
        /// never from the read.
        fn state(&self) -> WindowState {
            self.state.read(&self.delivery, || self.sample())
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
                    self.show_frame_change();
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
                    self.show_frame_change();
                }
                // Where the compositor offers frames its configure says
                // whether it draws one; a frame it leaves to the window, and
                // every frame where it offers none, the window draws itself.
                WindowCommand::SetFrame(frame) => {
                    let decorated = frame.has_system_title_bar();
                    let roles = self.roles.borrow();
                    if let Some(decoration) =
                        roles.as_ref().and_then(|roles| roles.decoration.as_ref())
                    {
                        decoration.set_mode(frame_mode(decorated));
                        drop(roles);
                        // What the compositor said before this request
                        // answers the one before it.
                        {
                            let mut told = self.record.protocol.lock();
                            told.frame_answered = false;
                            told.frame_asked = true;
                        }
                        self.applied.borrow_mut().decoration_answered = false;
                    } else if self.frame_globals.is_none() {
                        return Err(not_carried_out(&WindowCommand::SetFrame(frame)));
                    } else {
                        drop(roles);
                    }
                    {
                        let mut applied = self.applied.borrow_mut();
                        applied.decoration_requested = decorated;
                        applied.app_title_bar = match frame {
                            WindowFrame::AppTitleBar(bar) => Some(bar),
                            _ => None,
                        };
                    }
                    self.show_frame_change();
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
                // Admitted only where the compositor takes activation requests.
                WindowCommand::Activate => self.ask_activation()?,
                // The seats' input constrains the pointer of the seat whose
                // keyboard is in the window; one in no seat's keyboard holds
                // nothing.
                WindowCommand::SetCursorGrab(grab) => self
                    .input
                    .hold_pointer(self.id, grab)
                    .map_err(ApplyError::Platform)?,
                // xdg-shell has no levels: every window is at the normal one.
                WindowCommand::SetLevel(WindowLevel::Normal) => {}
                // `admit` refuses what a Wayland window cannot do.
                other => return Err(not_carried_out(&other)),
            }
            // After the command's requests: what the window decides itself
            // shows now, what the compositor decides with its configure.
            self.refresh_state();
            Ok(())
        }

        /// The cursor is the seats' input's, which shows it while the pointer
        /// is over the window: a system cursor through `cursor-shape` or the
        /// theme, an image as a buffer on the pointer's cursor surface, a
        /// hidden one as no surface.
        fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError> {
            self.input.set_cursor(self.id, cursor)
        }

        /// A Wayland compositor takes no refresh-rate request.
        fn set_frame_rate_hint(
            &self,
            _hint: flui_platform::FrameRateHint,
        ) -> Result<(), Unsupported> {
            Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
        }

        /// The chrome regions answer the presses on a title bar of the
        /// application's (ADR-0286 §11); the system's edge gestures a region
        /// keeps out are none a Wayland compositor lets a window exclude.
        fn set_input_regions(
            &self,
            regions: &[flui_platform::input_regions::InputRegion],
        ) -> Result<(), Unsupported> {
            if self.closed.get() {
                return Err(Unsupported::new(UnsupportedReason::NoWindow));
            }
            let scale = self.state.published().metrics.map_or(
                DevicePixelRatio::ONE,
                flui_foundation::geometry::WindowMetrics::scale,
            );
            self.frame_record.set_regions(regions, scale);
            Ok(())
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

        /// Sessions start from the window under the pointer grab it holds
        /// (`wl_data_source`).
        fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
            Some(Arc::clone(&self.drag).source(self.id))
        }
    }

    // Read by tests alone until the contract's window reference carries it;
    // that reader drops this `cfg`.
    #[cfg(any(test, feature = "testing"))]
    impl WaylandWindow {
        /// The window as an XDG Desktop Portal names the parent of the dialog
        /// it shows: the handle the compositor gave the window's export
        /// (`xdg-foreign`), so the dialog stays above the window and moves
        /// with it.
        ///
        /// `None` where the compositor offers no `zxdg_exporter_v2`, before
        /// it answered the export, and once the window closed, when its
        /// handle names nothing.
        pub(crate) fn portal_parent(&self) -> Option<PortalParent> {
            if self.closed.get() {
                return None;
            }
            self.record
                .protocol
                .lock()
                .exported
                .as_deref()
                .map(PortalParent::wayland)
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
            if !self.closed.get() {
                self.carry_out_frame();
            }
            if self.record.close_granted.swap(false, Ordering::AcqRel) {
                self.close();
            }
            if self.frame_due() {
                self.record.redraw.store(false, Ordering::Release);
                // A callback past its grace is given up: the present that
                // armed it committed nothing.
                *self.record.callback_armed.lock() = None;
                self.delivery.dispatch_request_frame();
            }
        }

        fn frame_due(&self) -> bool {
            !self.closed.get() && self.pacing().due(Instant::now())
        }

        fn frame_wait(&self) -> Option<Instant> {
            if self.closed.get() {
                return None;
            }
            self.pacing().gives_up_at()
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

        fn activate_forwarded(&self, token: Option<ActivationToken>) {
            if self.closed.get() {
                return;
            }
            match token {
                // Handed back once the window is shown, as the launcher's
                // token of the first window is.
                Some(token) => {
                    self.record.tokens.lock().grant(token);
                    self.hand_back_tokens();
                }
                None => {
                    if let Err(error) = self.ask_activation() {
                        tracing::debug!(
                            ?error,
                            "the compositor takes no activation for a forwarded launch"
                        );
                    }
                }
            }
        }
    }

    /// What the Wayland host's windows share and open with: what every
    /// window hands out of the window-level positions. Public where it is
    /// defined, as the host's table names it, and nameable only inside the
    /// crate.
    pub struct WaylandWindows {
        positions: WindowPositions<SurfaceRef>,
        /// The fonts the windows draw their own titles with.
        title_fonts: TitleFonts,
        /// The class every window carries as its app id.
        class: AppClass,
    }

    impl std::fmt::Debug for WaylandWindows {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WaylandWindows").finish_non_exhaustive()
        }
    }

    impl WaylandWindows {
        /// What the Wayland host's windows open with: each hands out
        /// `positions` and carries `class` as its app id.
        pub(crate) fn new(positions: WindowPositions<SurfaceRef>, class: AppClass) -> Self {
            Self {
                positions,
                title_fonts: TitleFonts::default(),
                class,
            }
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

        /// What a window has before its compositor said what it offers.
        fn capabilities(owner: &WaylandLoop, _: &WaylandWindows) -> WindowCapabilities {
            let decoration = Decoration {
                offered: owner.offers_decorations(),
                draws_own: owner.offers_frame_surfaces(),
                requested: true,
                told: None,
                answered: false,
            };
            pointer_reported(
                presented::reported(
                    capabilities(
                        None,
                        decoration.absence(),
                        decoration.app_title_bar_absence(),
                        ToplevelStates::default(),
                        if owner.offers_activation() {
                            Activation::Offered
                        } else {
                            Activation::Missing
                        },
                    ),
                    owner.offers_presentation(),
                ),
                owner.pointer_offer(),
            )
        }

        /// A frame the compositor draws lies outside the window's geometry;
        /// where it offers none, a window draws its own title bar above its
        /// content. Where it offers frames, its answer comes with the
        /// window, so the inset is the frame it is asked for: none.
        fn frame_inset(
            owner: &WaylandLoop,
            _: &WaylandWindows,
            _: flui_foundation::geometry::DevicePixelRatio,
        ) -> Size<f64> {
            if !owner.offers_decorations() && owner.offers_frame_surfaces() {
                Size::new(0.0, client_frame::HEADER)
            } else {
                Size::new(0.0, 0.0)
            }
        }

        /// The connection every window of the host is made on.
        fn display_connection(
            owner: &WaylandLoop,
            _: &WaylandWindows,
        ) -> Arc<dyn flui_platform::DisplayConnection> {
            Arc::new(WaylandDisplay(owner.connection().clone()))
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
            let mut refused = None;
            match event {
                wl_surface::Event::Enter { output } => {
                    if let Some(name) = output.data::<OutputName>()
                        && !told.outputs.contains(name)
                    {
                        told.outputs.push(*name);
                    }
                }
                wl_surface::Event::Leave { output } => {
                    if let Some(name) = output.data::<OutputName>() {
                        told.outputs.retain(|entered| entered != name);
                    }
                }
                wl_surface::Event::PreferredBufferScale { factor } => {
                    // By the hosts' one rule, as the fractional scale is: a
                    // refused one leaves the scale held.
                    refused = told.preferred_scale.tell(
                        factor,
                        f64::from(factor),
                        "wl_surface preferred_buffer_scale",
                    );
                }
                _ => {}
            }
            // Reported with the lock let go: a subscriber is user code.
            drop(told);
            if let Some(refused) = refused {
                refused.report();
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
                // A configure after a frame request that named no mode keeps
                // the mode: that is the compositor's answer.
                if std::mem::take(&mut told.frame_asked) {
                    told.frame_answered = true;
                }
                #[cfg(any(test, feature = "testing"))]
                {
                    told.configures = told.configures.saturating_add(1);
                }
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
                    let decoded = ToplevelStates::decode(&states);
                    #[cfg(any(test, feature = "testing"))]
                    let decoded = ToplevelStates {
                        suspended: decoded.suspended
                            || record.suspended_by_test.load(Ordering::Acquire),
                        ..decoded
                    };
                    #[cfg(any(test, feature = "testing"))]
                    {
                        told.raw_states = states;
                    }
                    let states = decoded;
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

    impl Dispatch<WlCallback, ArmedCallback> for WaylandState {
        /// The compositor showed the commit the callback was armed with: that
        /// commit happened, so the feedbacks asked up to it are committed.
        fn event(
            _: &mut Self,
            _: &WlCallback,
            event: wl_callback::Event,
            armed: &ArmedCallback,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wl_callback::Event::Done { .. } = event {
                let record = &armed.record;
                *record.callback_armed.lock() = None;
                if let Some(through) = armed.confirms {
                    record.feedbacks.lock().confirm(through);
                }
                record.frame_shown.store(true, Ordering::Release);
            }
        }
    }

    impl Dispatch<XdgActivationTokenV1, Arc<ToplevelRecord>> for WaylandState {
        /// The token the compositor answered with is the window's to hand
        /// back as it settles; the object is done once it came.
        fn event(
            _: &mut Self,
            token: &XdgActivationTokenV1,
            event: xdg_activation_token_v1::Event,
            record: &Arc<ToplevelRecord>,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let xdg_activation_token_v1::Event::Done { token: granted } = event {
                #[cfg(any(test, feature = "testing"))]
                {
                    let mut counted = record.tokens_counted.lock();
                    counted.0 = counted.0.saturating_add(1);
                }
                record.tokens.lock().grant(ActivationToken::new(granted));
                token.destroy();
            }
        }
    }

    impl Dispatch<WpPresentationFeedback, PresentedCommit> for WaylandState {
        /// A commit was shown: the frame it carried, at its time on the
        /// compositor's presentation clock rebased onto the event clock by its
        /// distance from a reading of that clock, and with the output's
        /// refresh where it is a cadence. A discarded commit (another replaced
        /// it before a refresh) reports nothing, and neither does a feedback
        /// the window forgot (its present was never confirmed committed, or
        /// its surface was replaced: [`presented::CommitFeedbacks`]); the
        /// compositor destroys the feedback after either event.
        fn event(
            _: &mut Self,
            _: &WpPresentationFeedback,
            event: wp_presentation_feedback::Event,
            commit: &PresentedCommit,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            let (tv_sec_hi, tv_sec_lo, tv_nsec, refresh) = match event {
                wp_presentation_feedback::Event::Presented {
                    tv_sec_hi,
                    tv_sec_lo,
                    tv_nsec,
                    refresh,
                    ..
                } => (tv_sec_hi, tv_sec_lo, tv_nsec, refresh),
                wp_presentation_feedback::Event::Discarded => {
                    commit.record.feedbacks.lock().discarded(commit.number);
                    return;
                }
                _ => return,
            };
            let frame = commit.record.feedbacks.lock().presented(commit.number);
            let (Some(frame), Some(shown), Some(now)) = (
                frame,
                presented::shown_at(tv_sec_hi, tv_sec_lo, tv_nsec),
                commit.to.time.clock.now(),
            ) else {
                return;
            };
            let mut report = flui_platform::FramePresented::new(frame, rebased(now, shown));
            // A zero refresh, a variable rate, is none, as is one outside a cadence.
            if let Ok(refresh) = flui_platform::RefreshPeriod::try_from(
                std::time::Duration::from_nanos(u64::from(refresh)),
            ) {
                report = report.refresh(refresh);
            }
            commit.to.reports.report(report);
        }
    }

    impl Dispatch<ZxdgToplevelDecorationV1, Arc<ToplevelRecord>> for WaylandState {
        fn event(
            _: &mut Self,
            _: &ZxdgToplevelDecorationV1,
            event: zxdg_toplevel_decoration_v1::Event,
            record: &Arc<ToplevelRecord>,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            // Part of the configure the next `xdg_surface.configure` closes;
            // the snapshot reads it once the window settles.
            if let zxdg_toplevel_decoration_v1::Event::Configure { mode } = event {
                let mode = match mode {
                    WEnum::Value(mode) => u32::from(mode),
                    WEnum::Unknown(mode) => mode,
                };
                let mut told = record.protocol.lock();
                #[cfg(any(test, feature = "testing"))]
                {
                    told.raw_frame_mode = Some(mode);
                }
                if let Some(drawer) = FrameDrawer::from_mode(mode) {
                    told.frame = Some(drawer);
                    told.frame_answered = true;
                    told.frame_asked = false;
                }
            }
        }
    }

    impl Dispatch<WpFractionalScaleV1, Arc<ToplevelRecord>> for WaylandState {
        fn event(
            _: &mut Self,
            _: &WpFractionalScaleV1,
            event: wp_fractional_scale_v1::Event,
            record: &Arc<ToplevelRecord>,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
                // By the hosts' one rule: a scale no screen has is refused and
                // the window keeps the one it held, and a run of them is
                // reported once, with the lock let go.
                let refused = record.protocol.lock().fractional_scale.tell(
                    scale,
                    f64::from(scale) / 120.0,
                    "wp_fractional_scale_v1",
                );
                if let Some(refused) = refused {
                    refused.report();
                }
            }
        }
    }

    impl Dispatch<ZxdgExportedV2, Arc<ToplevelRecord>> for WaylandState {
        /// The handle is kept where something reads it: only tests, until
        /// the contract's window reference carries it.
        fn event(
            _: &mut Self,
            _: &ZxdgExportedV2,
            event: zxdg_exported_v2::Event,
            record: &Arc<ToplevelRecord>,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            #[cfg(any(test, feature = "testing"))]
            if let zxdg_exported_v2::Event::Handle { handle } = event {
                record.protocol.lock().exported = Some(handle);
            }
            #[cfg(not(any(test, feature = "testing")))]
            let _ = (event, record);
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

    /// What the compositor sent a window, as the protocol carried it and
    /// before the window decoded it, for a test that reads it apart from the
    /// window's snapshot.
    #[cfg(any(test, feature = "testing"))]
    #[derive(Debug, Clone)]
    pub(crate) struct CompositorSaid {
        /// The size the last toplevel configure gave, `0` where it left an
        /// axis to the window.
        pub(crate) size: (i32, i32),
        /// The last toplevel configure's states array, undecoded.
        pub(crate) raw_states: Vec<u8>,
        /// How many `xdg_surface.configure` events came.
        pub(crate) configures: u32,
        /// The buffer scale the compositor prefers for the surface.
        pub(crate) preferred_scale: Option<i32>,
        /// The fractional scale the compositor prefers, as a numerator over
        /// 120.
        pub(crate) fractional_scale: Option<u32>,
        /// The last `zxdg_toplevel_decoration_v1.configure` mode.
        pub(crate) frame_mode: Option<u32>,
        /// The globals of the outputs the surface entered and has not left.
        pub(crate) outputs: Vec<OutputName>,
        /// How many activation tokens came for the window.
        pub(crate) activation_tokens: u32,
        /// How many of them the window handed back.
        pub(crate) activations: u32,
        /// Whether the compositor answered a frame callback of the window's:
        /// it showed the window at least once.
        pub(crate) shown: bool,
    }

    /// The frame a window draws itself, as it last sent it, for a test.
    #[cfg(any(test, feature = "testing"))]
    #[derive(Debug, Clone)]
    pub(crate) struct ClientFrameSaid {
        /// The window geometry last sent; `None` before the window drew a
        /// frame.
        pub(crate) geometry: Option<client_frame::Rect>,
        /// Each piece shown, with its rectangle relative to the content.
        pub(crate) pieces: Vec<(client_frame::Piece, client_frame::Rect)>,
        /// The moves, resizes and menus carried out, with their serials.
        pub(crate) carried: Vec<(Action, u32)>,
        /// The most buffers any piece keeps.
        pub(crate) buffers: usize,
        /// The most buffers any piece made.
        pub(crate) made: usize,
    }

    #[cfg(any(test, feature = "testing"))]
    impl WaylandWindow {
        /// What the compositor sent the window so far.
        pub(crate) fn compositor_said(&self) -> CompositorSaid {
            let told = self.record.protocol.lock();
            let (activation_tokens, activations) = *self.record.tokens_counted.lock();
            CompositorSaid {
                size: told.size,
                raw_states: told.raw_states.clone(),
                configures: told.configures,
                preferred_scale: told.preferred_scale.raw(),
                fractional_scale: told.fractional_scale.raw(),
                frame_mode: told.raw_frame_mode,
                outputs: told.outputs.clone(),
                activation_tokens,
                activations,
                shown: self.record.frame_shown.load(Ordering::Acquire),
            }
        }

        /// The frame the window draws itself: the window geometry it sent
        /// last, each piece shown with its rectangle relative to the content,
        /// the moves, resizes and menus it carried out with the serial each
        /// named, and the most buffers a piece keeps.
        pub(crate) fn client_frame_said(&self) -> ClientFrameSaid {
            let client_frame = self.client_frame.borrow();
            ClientFrameSaid {
                geometry: self.applied.borrow().geometry,
                pieces: client_frame.as_ref().map_or_default(ClientFrame::shown),
                carried: self.frame_record.carried_out(),
                buffers: client_frame
                    .as_ref()
                    .map_or_default(ClientFrame::most_buffers),
                made: client_frame.as_ref().map_or_default(ClientFrame::most_made),
            }
        }

        /// Whether the accessibility bridge was last told the window is the
        /// active one.
        pub(crate) fn bridge_focus(&self) -> bool {
            self.bridge_focus.get()
        }

        /// Take the last configure again with `suspended` as given, as though
        /// the compositor had sent it, and have every later configure carry
        /// it too until told otherwise; publish what follows. `false` for a
        /// window not configured yet.
        pub(crate) fn suspend_as_told(&self, suspended: bool) -> bool {
            self.record
                .suspended_by_test
                .store(suspended, Ordering::Release);
            {
                let mut applied = self.applied.borrow_mut();
                if !applied.configured {
                    return false;
                }
                let states = ToplevelStates {
                    suspended,
                    ..applied.sizing.states()
                };
                let constraints = applied.constraints;
                // Zero on both axes: the window keeps its extents.
                applied.sizing.configure(0, 0, states, constraints);
            }
            self.refresh_state();
            true
        }

        /// Ask the compositor to show the window fullscreen on the output
        /// `output`, which moves it there: a client places its window on an
        /// output in no other way. `false` for a closed window.
        pub(crate) fn fullscreen_on(
            &self,
            output: &wayland_client::protocol::wl_output::WlOutput,
        ) -> bool {
            let roles = self.roles.borrow();
            let Some(roles) = roles.as_ref() else {
                return false;
            };
            roles.toplevel.set_fullscreen(Some(output));
            true
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
            client_frame::surfaces::damage_all(surface);
            surface.commit();
            true
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
