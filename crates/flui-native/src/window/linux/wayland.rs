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
//! and the size a configure gives loses the title bar to the content.
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
    const TILED_RIGHT: u32 = 6;
    const TILED_TOP: u32 = 7;
    const TILED_BOTTOM: u32 = 8;
    const SUSPENDED: u32 = 9;

    /// `xdg_toplevel.wm_capabilities` values.
    const CAN_WINDOW_MENU: u32 = 1;
    const CAN_MAXIMIZE: u32 = 2;
    const CAN_FULLSCREEN: u32 = 3;
    const CAN_MINIMIZE: u32 = 4;

    /// The values of a protocol array of `u32`s, which the compositor writes
    /// in the host's byte order; a trailing partial value is ignored.
    fn values(raw: &[u8]) -> impl Iterator<Item = u32> + '_ {
        raw.as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(u32::from_ne_bytes)
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
        /// The edges of its output, or of other windows, the window is tiled
        /// against.
        pub(crate) tiled: TiledEdges,
        /// The compositor does not show the window (version 6).
        pub(crate) suspended: bool,
    }

    /// The edges a toplevel is tiled against (`tiled_left` and the others,
    /// version 2): the compositor keeps the window's frame from them.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct TiledEdges {
        pub(crate) left: bool,
        pub(crate) right: bool,
        pub(crate) top: bool,
        pub(crate) bottom: bool,
    }

    impl TiledEdges {
        /// No edge is tiled.
        pub(crate) const NONE: Self = Self {
            left: false,
            right: false,
            top: false,
            bottom: false,
        };

        /// Whether any edge is tiled.
        pub(crate) const fn any(self) -> bool {
            self.left || self.right || self.top || self.bottom
        }
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
                    TILED_LEFT => states.tiled.left = true,
                    TILED_RIGHT => states.tiled.right = true,
                    TILED_TOP => states.tiled.top = true,
                    TILED_BOTTOM => states.tiled.bottom = true,
                    SUSPENDED => states.suspended = true,
                    _ => {}
                }
            }
            states
        }

        /// Whether the compositor sizes the window itself: a maximized,
        /// fullscreen or tiled window takes the size it is given.
        pub(crate) const fn sized_by_compositor(self) -> bool {
            self.maximized || self.fullscreen || self.tiled.any()
        }

        /// Whether the window is tiled against an edge and in no mode it
        /// could ask to leave: its size is the tiling's, and a size the
        /// client asks for is not taken.
        pub(crate) const fn tiled_by_compositor(self) -> bool {
            self.tiled.any() && !self.maximized && !self.fullscreen
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
        /// The compositor shows a window menu on request.
        pub(crate) window_menu: bool,
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
                    CAN_WINDOW_MENU => capabilities.window_menu = true,
                    CAN_MAXIMIZE => capabilities.maximize = true,
                    CAN_FULLSCREEN => capabilities.fullscreen = true,
                    CAN_MINIMIZE => capabilities.minimize = true,
                    _ => {}
                }
            }
            capabilities
        }
    }

    /// Whether the compositor takes activation requests (`xdg_activation_v1`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Activation {
        /// It announces `xdg_activation_v1`.
        Offered,
        /// It announces none (Weston 13).
        Missing,
    }

    /// What a Wayland window honours, given what its compositor announced
    /// (`None` before version 5, or before it said), why it cannot have its
    /// frame turned on and off (`decoration`, `None` where it can) and
    /// whether the compositor takes activation requests (`activation`):
    /// the title, the size and its
    /// limits, the modes the compositor offers, and closing. Resizability is
    /// emulated with equal limits; placement, hiding, showing and position
    /// are not the client's on Wayland. Activation is emulated: the window
    /// asks with a token, and the compositor decides from it whether the
    /// window takes the focus or only asks for the user's attention
    /// (`activation`). A window the compositor tiles and neither maximized
    /// nor made fullscreen (`states`) takes the size the compositor gives
    /// it: the client's size is no request it honours. A compositor that
    /// announces no `wm_capabilities` (before version 5; sway 1.9) is taken
    /// to offer every mode, as the window cannot know which it ignores.
    pub(crate) fn capabilities(
        announced: Option<WmCapabilities>,
        decoration: Option<CapabilityAbsence>,
        states: ToplevelStates,
        activation: Activation,
    ) -> WindowCapabilities {
        let mut capabilities = WindowCapabilities::ALL
            // The text-input protocol reports no keyboard geometry, and a
            // desktop compositor keeps no edges for system gestures.
            .without_keyboard_inset(CapabilityAbsence::NotOnThisPlatform)
            .without_system_gestures(CapabilityAbsence::NotOnThisPlatform)
            .unavailable(
                &WindowCommand::SetPosition(flui_foundation::geometry::Point::new(0.0, 0.0)),
                CapabilityAbsence::NotOnThisPlatform,
            )
            .unavailable(&WindowCommand::Hide, CapabilityAbsence::NotOnThisPlatform)
            .unavailable(&WindowCommand::Show, CapabilityAbsence::NotOnThisPlatform)
            .unavailable(
                &WindowCommand::SetMaterial(flui_platform::WindowMaterial::System),
                CapabilityAbsence::NotOnThisPlatform,
            );
        match activation {
            Activation::Offered => capabilities.activates = Availability::Emulated,
            Activation::Missing => {
                capabilities = capabilities
                    .unavailable(&WindowCommand::Activate, CapabilityAbsence::NotOnThisDevice);
            }
        }
        if let Some(absence) = decoration {
            capabilities = capabilities.unavailable(&WindowCommand::SetDecorated(true), absence);
        }
        if states.tiled_by_compositor() {
            capabilities = capabilities.unavailable(
                &WindowCommand::SetSize(Size::new(1.0, 1.0)),
                CapabilityAbsence::NotOnThisDevice,
            );
        }
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
                    tiled: TiledEdges::NONE,
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

        /// Take a configure of `width` by `height` with `states`. Each axis
        /// is zero on its own where the compositor leaves it to the client
        /// (xdg-shell), and keeps the window's own extent. A window the
        /// compositor sizes takes the size it is given; one in normal mode
        /// takes a size asked for meanwhile, else the compositor's suggestion
        /// within `constraints`, else returns to the size it had in normal
        /// mode. Returns whether the content size changed.
        pub(crate) fn configure(
            &mut self,
            width: i32,
            height: i32,
            states: ToplevelStates,
            constraints: SizeConstraints,
        ) -> bool {
            let before = self.content;
            let given = |extent: i32, own: f64| {
                if extent > 0 { f64::from(extent) } else { own }
            };
            let suggested = width > 0 || height > 0;
            // The extents the window falls back on: the normal size once the
            // compositor lets go of it, its current size otherwise.
            let own = if self.states.sized_by_compositor() && !states.sized_by_compositor() {
                self.normal
            } else {
                self.content
            };
            if states.sized_by_compositor() {
                self.content = Size::new(given(width, own.width), given(height, own.height));
            } else if let Some(size) = self.pending.take() {
                self.content = clamp(size, constraints);
            } else if suggested {
                self.content = clamp(
                    Size::new(given(width, own.width), given(height, own.height)),
                    constraints,
                );
            } else {
                self.content = own;
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
            values
                .iter()
                .flat_map(|value| value.to_ne_bytes())
                .collect()
        }

        /// A configure's states read as the protocol numbers them, a value of
        /// a newer protocol is ignored, and the mode follows: fullscreen over
        /// maximized, never minimized.
        #[test]
        fn a_configure_names_the_toplevel_states() {
            let states = ToplevelStates::decode(&array(&[ACTIVATED, MAXIMIZED, 77]));
            assert!(states.activated && states.maximized);
            assert!(!states.fullscreen && !states.tiled.any() && !states.suspended);
            assert_eq!(states.mode(), WindowMode::Maximized);
            let both = ToplevelStates::decode(&array(&[MAXIMIZED, FULLSCREEN]));
            assert_eq!(both.mode(), WindowMode::Fullscreen);
            let tiled = ToplevelStates::decode(&array(&[TILED_BOTTOM, SUSPENDED]));
            assert!(tiled.tiled.any() && tiled.suspended && tiled.sized_by_compositor());
            assert_eq!(tiled.mode(), WindowMode::Normal);
            // Each edge is its own: the frame leaves the tiled ones alone.
            let edges = ToplevelStates::decode(&array(&[TILED_LEFT, TILED_RIGHT, TILED_TOP]));
            assert_eq!(
                edges.tiled,
                TiledEdges {
                    left: true,
                    right: true,
                    top: true,
                    bottom: false,
                }
            );
            assert!(WmCapabilities::decode(&array(&[CAN_WINDOW_MENU])).window_menu);
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

            assert!(
                !sizing.configure(0, 0, normal, none),
                "0x0: the client decides"
            );
            assert_eq!(sizing.content(), Size::new(800.0, 600.0));

            assert!(sizing.configure(1920, 1080, maximized, none));
            assert_eq!(sizing.content(), Size::new(1920.0, 1080.0));

            assert!(
                sizing.request(Size::new(640.0, 480.0), none),
                "leave the mode first"
            );
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

            let limits = SizeConstraints::new(
                Some(Size::new(200.0, 150.0)),
                Some(Size::new(1000.0, 700.0)),
            );
            assert!(sizing.configure(1200, 100, normal, limits));
            assert_eq!(
                sizing.content(),
                Size::new(1000.0, 150.0),
                "within the limits"
            );
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

        /// Each axis of a configure is zero on its own: the window keeps its
        /// own extent on that axis and takes the other.
        #[test]
        fn a_configure_axis_left_to_the_window_keeps_its_extent() {
            let none = SizeConstraints::NONE;
            let normal = ToplevelStates::default();
            let maximized = ToplevelStates {
                maximized: true,
                ..ToplevelStates::default()
            };
            let mut sizing = Sizing::new(Size::new(640.0, 480.0));
            assert!(sizing.configure(800, 0, normal, none));
            assert_eq!(sizing.content(), Size::new(800.0, 480.0));
            assert!(sizing.configure(0, 300, normal, none));
            assert_eq!(sizing.content(), Size::new(800.0, 300.0));
            assert!(sizing.configure(1920, 0, maximized, none));
            assert_eq!(sizing.content(), Size::new(1920.0, 300.0));
            assert!(sizing.configure(0, 0, normal, none));
            assert_eq!(
                sizing.content(),
                Size::new(800.0, 300.0),
                "the normal size back"
            );
        }

        /// The capabilities follow what the compositor announces, and a
        /// command the client can never carry out on Wayland, or that this
        /// host does not carry out yet, is refused with its own reason.
        #[test]
        fn the_capabilities_follow_the_compositor() {
            let free = ToplevelStates::default();
            let unannounced = capabilities(
                None,
                Some(CapabilityAbsence::NotOnThisDevice),
                free,
                Activation::Missing,
            );
            assert!(unannounced.maximizes.is_available());
            assert!(unannounced.minimizes.is_available());
            assert!(unannounced.sets_size.is_available());
            assert_eq!(unannounced.sets_resizable, Availability::Emulated);
            assert_eq!(
                capabilities(None, None, free, Activation::Missing).sets_decorated,
                Availability::Native,
                "a compositor that draws frames takes the command"
            );
            assert_eq!(
                capabilities(None, None, free, Activation::Offered).activates,
                Availability::Emulated,
                "the compositor decides what an activation request does"
            );
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
                    flui_platform::UnsupportedReason::NotOnThisDevice,
                ),
                (
                    WindowCommand::Activate,
                    flui_platform::UnsupportedReason::NotOnThisDevice,
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
            let announced = capabilities(
                Some(WmCapabilities::decode(&array(&[CAN_FULLSCREEN]))),
                None,
                free,
                Activation::Offered,
            );
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

        /// A window the compositor tiles takes the size the tiling gives it,
        /// so it takes no size command; one tiled and maximized or fullscreen
        /// takes it, by leaving the mode first.
        #[test]
        fn a_tiled_window_takes_no_size() {
            let tiled = ToplevelStates::decode(&array(&[TILED_LEFT, ACTIVATED]));
            let refused = WindowCommand::SetSize(Size::new(640.0, 480.0))
                .admit(&capabilities(None, None, tiled, Activation::Missing))
                .expect_err("the tiling holds the size");
            assert_eq!(
                refused.unsupported().map(|unsupported| unsupported.reason),
                Some(flui_platform::UnsupportedReason::NotOnThisDevice)
            );
            let maximized = ToplevelStates::decode(&array(&[TILED_LEFT, MAXIMIZED]));
            assert!(
                capabilities(None, None, maximized, Activation::Missing)
                    .sets_size
                    .is_available()
            );
            let floating = ToplevelStates::decode(&array(&[ACTIVATED]));
            assert!(
                capabilities(None, None, floating, Activation::Missing)
                    .sets_size
                    .is_available()
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

pub(crate) mod frame {
    //! When a Wayland window delivers the frame it was asked for: once the
    //! compositor has shown the last one (its frame callback's `done`), or
    //! once that callback is given up.
    //!
    //! The raster thread arms the callback just before it presents, and the
    //! present's commit carries it to the compositor. A present that is
    //! skipped (an outdated surface, a zero size) commits nothing, so its
    //! callback never comes: after [`CALLBACK_GRACE`] the window stops waiting
    //! for it. A window the compositor does not draw sends no `done` either;
    //! it is then asked for a frame at most once per grace.
    //!
    //! A window the compositor says it does not show (`suspended`: covered by
    //! other windows, on a hidden workspace, its outputs off) is occluded: it
    //! is given no frame, and no grace runs out for it. The frame asked for
    //! is kept, and delivered once the compositor shows the window again.

    #![forbid(unsafe_code)]

    use std::time::Duration;

    use web_time::Instant;

    /// How long a window waits for a frame callback before it gives it up.
    pub(crate) const CALLBACK_GRACE: Duration = Duration::from_secs(1);

    /// Whether a callback armed at `armed` still holds the next frame back.
    pub(crate) fn waiting_on_callback(armed: Option<Instant>, now: Instant) -> bool {
        armed.is_some_and(|armed| now < armed + CALLBACK_GRACE)
    }

    /// What decides whether a window delivers a frame.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Pacing {
        /// A frame was asked for.
        pub(crate) asked: bool,
        /// The compositor configured the window.
        pub(crate) configured: bool,
        /// The compositor does not show the window.
        pub(crate) suspended: bool,
        /// When the frame callback that is out was armed.
        pub(crate) armed: Option<Instant>,
    }

    impl Pacing {
        /// Whether the window delivers its frame now.
        pub(crate) fn due(self, now: Instant) -> bool {
            self.asked
                && self.configured
                && !self.suspended
                && !waiting_on_callback(self.armed, now)
        }

        /// When the window gives up the callback that holds its frame back;
        /// `None` with nothing armed, nothing asked for, or while the window
        /// is not shown, whose frame waits for the compositor instead.
        pub(crate) fn gives_up_at(self) -> Option<Instant> {
            self.armed
                .filter(|_| self.asked && !self.suspended)
                .map(|armed| armed + CALLBACK_GRACE)
        }
    }

    /// The first output in `entered` (the order a surface entered them) the
    /// compositor still announces: one removed while the surface was on it
    /// sends the surface no `leave`.
    pub(crate) fn first_live<T: Copy>(entered: &[T], live: impl Fn(T) -> bool) -> Option<T> {
        entered.iter().copied().find(|output| live(*output))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A frame waits on the callback its present armed until the
        /// compositor answers or the grace runs out; a present that committed
        /// nothing never holds the window for longer.
        #[test]
        fn a_lost_frame_callback_holds_the_window_for_its_grace_only() {
            let now = Instant::now();
            let ms = Duration::from_millis;
            assert!(!waiting_on_callback(None, now), "nothing armed");
            assert!(waiting_on_callback(Some(now), now), "just armed");
            assert!(waiting_on_callback(now.checked_sub(ms(900)), now));
            assert!(
                !waiting_on_callback(now.checked_sub(CALLBACK_GRACE), now),
                "given up at the grace"
            );
            let pacing = |asked, armed| Pacing {
                asked,
                configured: true,
                suspended: false,
                armed,
            };
            assert_eq!(
                pacing(true, Some(now)).gives_up_at(),
                Some(now + CALLBACK_GRACE)
            );
            assert_eq!(
                pacing(false, Some(now)).gives_up_at(),
                None,
                "no frame asked for"
            );
            assert_eq!(pacing(true, None).gives_up_at(), None);
            assert!(pacing(true, None).due(now));
            assert!(!pacing(true, Some(now)).due(now), "the callback holds it");
            assert!(pacing(true, now.checked_sub(CALLBACK_GRACE)).due(now));
            assert!(!pacing(false, None).due(now), "nothing asked for");
            let unconfigured = Pacing {
                configured: false,
                ..pacing(true, None)
            };
            assert!(!unconfigured.due(now));
        }

        /// An occluded window (the compositor suspended it) is given no frame
        /// and gives up no callback, however long it waits; the frame asked
        /// for is delivered once the compositor shows the window again.
        #[test]
        fn an_occluded_window_holds_its_frame_until_it_is_shown() {
            let now = Instant::now();
            let long_ago = now.checked_sub(CALLBACK_GRACE * 5);
            let occluded = Pacing {
                asked: true,
                configured: true,
                suspended: true,
                armed: long_ago,
            };
            assert!(!occluded.due(now), "past the grace, still occluded");
            assert_eq!(occluded.gives_up_at(), None, "no wake for a grace");
            let unarmed = Pacing {
                armed: None,
                ..occluded
            };
            assert!(!unarmed.due(now), "nothing armed, still occluded");
            let shown = Pacing {
                suspended: false,
                ..occluded
            };
            assert!(shown.due(now), "shown again: the kept frame goes out");
        }

        /// A surface is on the first output it entered that still exists.
        #[test]
        fn a_removed_output_names_no_display() {
            let live = |output: u32| output != 7;
            assert_eq!(first_live(&[7, 9], live), Some(9));
            assert_eq!(first_live(&[7], live), None);
            assert_eq!(first_live::<u32>(&[], live), None);
        }
    }
}

pub(crate) mod decoration {
    //! Who draws a Wayland window's frame: the compositor (`xdg-decoration`),
    //! the window itself ([`client_frame`](super::client_frame)), or nobody.
    //!
    //! A compositor that offers `zxdg_decoration_manager_v1` draws a frame for
    //! a window that asks, outside its surface; it may also decline and leave
    //! the frame to the client. A compositor that offers none (Weston, GNOME)
    //! draws none, and leaves the frame to the client. Where the frame is the
    //! client's and the window wants one, the window draws its own; where it
    //! wants none, it has none.

    #![forbid(unsafe_code)]

    use flui_platform::CapabilityAbsence;

    /// `zxdg_toplevel_decoration_v1.mode` values.
    const CLIENT_SIDE: u32 = 1;
    const SERVER_SIDE: u32 = 2;

    /// Who draws the frame, as the compositor last said.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum FrameDrawer {
        /// The compositor draws it.
        Compositor,
        /// The client draws it, or none.
        Client,
    }

    impl FrameDrawer {
        /// The drawer a configure's `mode` names; `None` for a mode of a
        /// newer protocol.
        pub(crate) const fn from_mode(mode: u32) -> Option<Self> {
            match mode {
                SERVER_SIDE => Some(Self::Compositor),
                CLIENT_SIDE => Some(Self::Client),
                _ => None,
            }
        }
    }

    /// Who draws the window's frame now.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Drawn {
        /// The compositor, outside the window's surface.
        Compositor,
        /// The window, on surfaces of its own around its content.
        Client,
        /// Nobody: the window wants none, or the compositor has not said yet.
        Nobody,
    }

    /// A window's frame as the client asked for it and the compositor
    /// answered.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Decoration {
        /// The compositor offers `xdg-decoration`.
        pub(crate) offered: bool,
        /// The window can draw a frame of its own: the compositor offers the
        /// subsurfaces and the shared memory it is drawn with.
        pub(crate) draws_own: bool,
        /// The client last asked for a frame.
        pub(crate) requested: bool,
        /// What the compositor last said; `None` before it said.
        pub(crate) told: Option<FrameDrawer>,
        /// The compositor answered the last request.
        pub(crate) answered: bool,
    }

    impl Decoration {
        /// Who draws the frame: the compositor once it said it does; the
        /// window where the frame is left to it and it wants one, which a
        /// compositor without `xdg-decoration` does from the start; nobody
        /// while a compositor that offers frames has not answered yet.
        pub(crate) const fn drawn(self) -> Drawn {
            match self.told {
                Some(FrameDrawer::Compositor) => Drawn::Compositor,
                Some(FrameDrawer::Client) if self.requested && self.draws_own => Drawn::Client,
                None if !self.offered && self.requested && self.draws_own => Drawn::Client,
                _ => Drawn::Nobody,
            }
        }

        /// Whether the window has a frame, whoever draws it.
        pub(crate) const fn decorated(self) -> bool {
            !matches!(self.drawn(), Drawn::Nobody)
        }

        /// Why the window's frame cannot be turned on and off, or `None`
        /// where it can. A compositor that answered a request for no frame
        /// with one of its own decides the frame itself (sway draws one
        /// around every tiled window). A window that cannot draw its own
        /// frame has none where the compositor draws none, and one the
        /// compositor answered with the other mode is overruled either way.
        /// Until the compositor answers, the request stands.
        pub(crate) const fn absence(self) -> Option<CapabilityAbsence> {
            let (answered_with, decided) = match (self.answered, self.told) {
                (true, Some(drawer)) => (Some(drawer), true),
                _ => (None, false),
            };
            let overruled = if self.draws_own {
                decided && matches!(answered_with, Some(FrameDrawer::Compositor)) && !self.requested
            } else {
                decided && matches!(answered_with, Some(FrameDrawer::Compositor)) != self.requested
            };
            if overruled || (!self.offered && !self.draws_own) {
                Some(CapabilityAbsence::NotOnThisDevice)
            } else {
                None
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The frame is the compositor's answer where it draws one and the
        /// window's own where the frame is left to it, and only a compositor
        /// that insists on drawing it takes the command away.
        #[test]
        fn a_window_has_the_frame_its_compositor_leaves_it() {
            use Drawn::{Client as Own, Compositor as Theirs, Nobody};
            use FrameDrawer::{Client, Compositor};
            let overruled = Some(CapabilityAbsence::NotOnThisDevice);
            let decoration = |offered, requested, told, answered| Decoration {
                offered,
                draws_own: true,
                requested,
                told,
                answered,
            };
            for (case, decoration, drawn, absence) in [
                (
                    "no xdg-decoration: the window's own",
                    decoration(false, true, None, false),
                    Own,
                    None,
                ),
                (
                    "no xdg-decoration, none wanted",
                    decoration(false, false, None, false),
                    Nobody,
                    None,
                ),
                (
                    "asked, not answered yet",
                    decoration(true, true, None, false),
                    Nobody,
                    None,
                ),
                (
                    "asked and drawn",
                    decoration(true, true, Some(Compositor), true),
                    Theirs,
                    None,
                ),
                (
                    "asked, left to the client: the window's own",
                    decoration(true, true, Some(Client), true),
                    Own,
                    None,
                ),
                (
                    "turned off",
                    decoration(true, false, Some(Client), true),
                    Nobody,
                    None,
                ),
                (
                    "turned off, drawn regardless",
                    decoration(true, false, Some(Compositor), true),
                    Theirs,
                    overruled,
                ),
                (
                    "turned off, the old frame until the answer",
                    decoration(true, false, Some(Compositor), false),
                    Theirs,
                    None,
                ),
                (
                    "turned on again, the window's own until the answer",
                    decoration(true, true, Some(Client), false),
                    Own,
                    None,
                ),
            ] {
                assert_eq!(decoration.drawn(), drawn, "{case}");
                assert_eq!(decoration.decorated(), drawn != Nobody, "{case}");
                assert_eq!(decoration.absence(), absence, "{case}");
            }
            assert_eq!(
                FrameDrawer::from_mode(SERVER_SIDE),
                Some(FrameDrawer::Compositor)
            );
            assert_eq!(
                FrameDrawer::from_mode(CLIENT_SIDE),
                Some(FrameDrawer::Client)
            );
            assert_eq!(FrameDrawer::from_mode(9), None);
        }

        /// A window that cannot draw a frame of its own (no subsurfaces or
        /// shared memory) has only the compositor's, and says so.
        #[test]
        fn a_window_that_draws_no_frame_has_the_compositors() {
            let none = |offered, requested, told, answered| Decoration {
                offered,
                draws_own: false,
                requested,
                told,
                answered,
            };
            let overruled = Some(CapabilityAbsence::NotOnThisDevice);
            let bare = none(false, true, None, false);
            assert_eq!(bare.drawn(), Drawn::Nobody);
            assert_eq!(bare.absence(), overruled);
            let left = none(true, true, Some(FrameDrawer::Client), true);
            assert_eq!(left.drawn(), Drawn::Nobody);
            assert_eq!(left.absence(), overruled);
            let drawn = none(true, true, Some(FrameDrawer::Compositor), true);
            assert_eq!(drawn.drawn(), Drawn::Compositor);
            assert_eq!(drawn.absence(), None);
        }
    }
}

pub(crate) mod scale {
    //! The scale a Wayland window's buffer is drawn at.
    //!
    //! With `wp_viewporter` the surface's destination is its logical size and
    //! the buffer any size, so the buffer is drawn at the scale the compositor
    //! prefers: a fractional one where it offers `wp_fractional_scale_v1`
    //! (`preferred_scale`, as a numerator over 120), else the whole one `wl_surface`
    //! version 6 prefers, else that of the output the surface entered.
    //! Without the viewporter the buffer is the surface's size: scale 1, and
    //! the compositor scales it.

    #![forbid(unsafe_code)]

    /// What the compositor said of the scale a surface is drawn at.
    #[derive(Debug, Clone, Copy, Default, PartialEq)]
    pub(crate) struct ScaleTold {
        /// `wp_fractional_scale_v1.preferred_scale`, as a numerator over 120.
        pub(crate) fractional: Option<u32>,
        /// `wl_surface.preferred_buffer_scale`.
        pub(crate) preferred: Option<i32>,
        /// The scale of the output the surface is on.
        pub(crate) output: Option<f64>,
    }

    /// A fractional scale as the protocol carries it, as a numerator over 120; `None` for
    /// zero, which is no scale.
    fn over_120(numerator: u32) -> Option<f64> {
        (numerator > 0).then(|| f64::from(numerator) / 120.0)
    }

    /// The scale the window's buffer is drawn at, finite and positive.
    pub(crate) fn buffer_scale(viewported: bool, told: ScaleTold) -> f64 {
        if !viewported {
            return 1.0;
        }
        told.fractional
            .and_then(over_120)
            .or_else(|| told.preferred.filter(|scale| *scale > 0).map(f64::from))
            .or(told.output)
            .filter(|scale| scale.is_finite() && *scale > 0.0)
            .unwrap_or(1.0)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The fractional scale wins over the whole ones, and each falls back
        /// to the next where the compositor said nothing usable.
        #[test]
        fn a_buffer_is_drawn_at_the_scale_the_compositor_prefers() {
            let told = |fractional, preferred, output| ScaleTold {
                fractional,
                preferred,
                output,
            };
            for (case, viewported, told, expected) in [
                ("nothing said", true, told(None, None, None), 1.0),
                (
                    "a fractional scale",
                    true,
                    told(Some(180), Some(2), Some(2.0)),
                    1.5,
                ),
                (
                    "a whole preferred scale",
                    true,
                    told(None, Some(2), Some(1.0)),
                    2.0,
                ),
                ("the output's", true, told(None, None, Some(3.0)), 3.0),
                ("a zero fraction", true, told(Some(0), Some(2), None), 2.0),
                (
                    "a zero preferred scale",
                    true,
                    told(None, Some(0), Some(1.25)),
                    1.25,
                ),
                (
                    "a broken output scale",
                    true,
                    told(None, None, Some(f64::NAN)),
                    1.0,
                ),
                (
                    "no viewporter",
                    false,
                    told(Some(180), Some(2), Some(2.0)),
                    1.0,
                ),
                (
                    "a numerator over 120",
                    true,
                    told(Some(150), None, None),
                    1.25,
                ),
            ] {
                assert_eq!(buffer_scale(viewported, told), expected, "{case}");
            }
        }
    }
}

pub(crate) mod presented {
    //! When the compositor showed a window's commit, as
    //! `wp_presentation_feedback.presented` carries it: a time on the clock
    //! `wp_presentation.clock_id` named, and the output's refresh interval.
    //! The window asks for one feedback per commit it presents (its surface
    //! target's `pre_present_notify`) and reports each shown commit on the
    //! event timeline; a discarded commit reports nothing.

    #![forbid(unsafe_code)]

    use std::time::Duration;

    use flui_platform::{CapabilityAbsence, WindowCapabilities};

    /// The time a `presented` event carries, from its three words: the
    /// seconds split into high and low halves, and the nanoseconds.
    /// Nanoseconds of a second or more are not a time and give `None`.
    pub(crate) fn shown_at(tv_sec_hi: u32, tv_sec_lo: u32, tv_nsec: u32) -> Option<Duration> {
        if tv_nsec >= 1_000_000_000 {
            return None;
        }
        let seconds = (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo);
        Some(Duration::new(seconds, tv_nsec))
    }

    /// `capabilities` with the presentation report the compositor makes:
    /// where it offers no `wp_presentation`, a window learns nothing of when
    /// its frames are shown.
    pub(crate) const fn reported(
        capabilities: WindowCapabilities,
        offered: bool,
    ) -> WindowCapabilities {
        if offered {
            capabilities
        } else {
            capabilities.without_presentation_feedback(CapabilityAbsence::NotOnThisDevice)
        }
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::{Availability, UnsupportedReason};

        use super::*;

        /// A `presented` event's words make the time it names; nanoseconds
        /// past a second do not.
        #[test]
        fn a_presented_event_names_its_time() {
            assert_eq!(
                shown_at(0, 12, 500_000_000),
                Some(Duration::from_millis(12_500))
            );
            assert_eq!(
                shown_at(1, 0, 0),
                Some(Duration::from_secs(1 << 32)),
                "the high word holds the upper seconds"
            );
            assert_eq!(shown_at(0, 1, 1_000_000_000), None);
        }

        /// A compositor without `wp_presentation` leaves a window without the
        /// report, for want of the device's compositor, not the platform.
        #[test]
        fn the_report_follows_the_compositor() {
            let offered = reported(WindowCapabilities::ALL, true);
            assert_eq!(offered.presentation_feedback(), Ok(Availability::Native));
            let missing = reported(WindowCapabilities::ALL, false);
            assert_eq!(
                missing
                    .presentation_feedback()
                    .map_err(|absent| absent.reason),
                Err(UnsupportedReason::NotOnThisDevice)
            );
        }
    }
}

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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Weak};

    use cursor_icon::CursorIcon;
    use flui_foundation::geometry::{Size, WindowMetrics};
    use flui_platform::input_capabilities::InputCapabilities;
    use flui_platform::{
        AdmittedCommand, ApplyError, CloseDecision, CursorError, PlatformTextInput, PlatformWindow,
        SizeConstraints, Unsupported, UnsupportedReason, WindowCapabilities, WindowCommand,
        WindowExecutionState, WindowGeometry, WindowHandler, WindowId, WindowMode, WindowOptions,
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
        Activation, Sizing, ToplevelStates, WmCapabilities, capabilities, clamp, protocol_extent,
    };
    use super::decoration::{Decoration, Drawn, FrameDrawer};
    #[cfg(any(test, feature = "testing"))]
    use super::foreign::PortalParent;
    use super::frame::{Pacing, first_live, waiting_on_callback};
    use super::presented;
    use super::scale::{ScaleTold, buffer_scale};
    use crate::executor::BackgroundExecutor;
    use crate::host::{self, WindowFacets, WindowPositions};
    use crate::host_window::{CoreHostWindow, HostWindow, OwnerThreadToken};
    use crate::owner::OpenWindowError;
    use crate::owner::handlers::{PresentationReports, rebased};
    use crate::owner::linux::PortalSlot;
    use crate::owner::linux::wayland::activation::ActivationToken;
    use crate::owner::linux::wayland::{
        LastInput, LiveOutputs, LoopWaker, LoopWindow, OutputFacts, OutputName, PresentationTime,
        SeatWindow, WaylandDisplays, WaylandInput, WaylandLoop, WaylandState, WindowSeed,
        WindowTable,
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
        /// When the frame callback that is out was armed: the compositor has
        /// not shown the last frame yet, and the next waits for it, for the
        /// grace at most (`frame::CALLBACK_GRACE`).
        callback_armed: Mutex<Option<Instant>>,
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
        /// The buffer scale the compositor prefers for the surface.
        preferred_scale: Option<i32>,
        /// The fractional scale the compositor prefers, as a numerator over 120.
        fractional_scale: Option<u32>,
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
        /// now; `None` where it draws none.
        fn own_frame(&self, decoration: Decoration, states: ToplevelStates) -> Option<Frame> {
            (decoration.drawn() == Drawn::Client).then_some(Frame {
                content: self.sizing.content(),
                states,
                resizable: self.resizable,
                offers: Offers::from(self.told.wm),
            })
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
        table: Weak<WindowTable>,
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
        frame_record: Arc<FrameRecord>,
        /// The fonts its title is drawn with, shared by the host's windows.
        title_fonts: TitleFonts,
        /// The loop's background work, which reads the host's fonts.
        executor: Arc<BackgroundExecutor>,
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
        applied: RefCell<Applied>,
        closed: Cell<bool>,
        /// Whether the accessibility bridge was last told the window is the
        /// active one.
        bridge_focus: Cell<bool>,
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
        closed: bool,
    }

    impl Sample<'_> {
        /// The output the window is on, and the scale its buffer is drawn at.
        fn output_and_scale(&self) -> (Option<OutputFacts>, f64) {
            let applied = self.applied;
            let entered = first_live(&applied.told.outputs, |output| self.live.contains(output));
            let output = entered.and_then(|output| self.displays.output(output));
            let scale = buffer_scale(
                self.viewported,
                ScaleTold {
                    fractional: applied.told.fractional_scale,
                    preferred: applied.told.preferred_scale,
                    output: output.map(|output| output.scale),
                },
            );
            (output, scale)
        }

        fn state(&self) -> WindowState {
            let applied = self.applied;
            let entered = first_live(&applied.told.outputs, |output| self.live.contains(output));
            let (output, scale) = self.output_and_scale();
            let decoration = applied.decoration(self.offers_decorations, self.draws_own);
            let content = applied.sizing.content();
            let states = applied.sizing.states();
            // The outer size is the window's geometry: the content, and the
            // title bar of a frame it draws itself.
            let header = applied
                .own_frame(decoration, states)
                .map_or(0.0, |frame| frame.header());
            let outer = Size::new(content.width, content.height + header);
            let state = WindowState::new(WindowGeometry::new(None, outer), content)
                .with_mode(states.mode())
                // A toplevel has no hidden state of its own: it is shown
                // until it closes.
                .with_visible(!self.closed)
                .with_focused(states.activated)
                .with_title(Some(applied.title.clone()))
                .with_constraints(applied.constraints)
                .with_resizable(applied.resizable)
                .with_decorated(decoration.decorated())
                .with_display(output.map(|output| output.display));
            ReportedFacts {
                metrics: WindowMetrics::from_logical(content, scale).ok(),
                appearance: self.portal.appearance(),
                insets: flui_platform::WindowInsets::ZERO,
                execution: WindowExecutionState::Running,
                // What the compositor says it draws: a surface it maps (a
                // configured window with a frame) enters an output, and one
                // it does not draw (covered, on a hidden workspace, its
                // outputs off, minimized) is suspended.
                occluded: !applied.configured || entered.is_none() || states.suspended,
                hovered: self.hovered,
                active: states.activated,
                capabilities: presented::reported(
                    capabilities(
                        applied.told.wm,
                        decoration.absence(),
                        states,
                        self.activation,
                    ),
                    self.reports_presentation,
                ),
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
                frames,
                input,
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
            // Asked before the first commit, so the first configure says who
            // draws the frame.
            let decoration = protocol.decoration.as_ref().map(|manager| {
                let decoration =
                    manager.get_toplevel_decoration(&toplevel, &queue, Arc::clone(&record));
                decoration.set_mode(frame_mode(options.decorated));
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
                decoration_requested: options.decorated,
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
                offers_decorations: protocol.decoration.is_some(),
                frame_globals,
                client_frame: RefCell::new(None),
                frame_record: Arc::new(FrameRecord::default()),
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
                applied: RefCell::new(applied),
                closed: Cell::new(false),
                bridge_focus: Cell::new(false),
                #[cfg(any(test, feature = "testing"))]
                shm: protocol.shm,
            });
            // The input gives the window its capabilities before the handler
            // hears its snapshot, and its input from the compositor's first
            // event.
            let state = Arc::downgrade(&window.state);
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
                frame: Arc::clone(&window.frame_record) as _,
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
        fn sample(&self) -> WindowState {
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
                closed: self.closed.get(),
            }
            .state()
        }

        /// The frame as asked for and answered now.
        fn decoration(&self, applied: &Applied) -> Decoration {
            applied.decoration(self.offers_decorations, self.frame_globals.is_some())
        }

        /// The scale the window's buffers are drawn at now.
        fn buffer_scale(&self, applied: &Applied) -> f64 {
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
                closed: false,
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
                let frame = applied.own_frame(self.decoration(&applied), applied.sizing.states());
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
                        Arc::clone(&self.frame_record),
                        &fonts,
                    ));
                }
                if let Some(client_frame) = client_frame.as_mut() {
                    let look = Look {
                        dark: matches!(
                            self.portal.appearance(),
                            flui_platform::WindowAppearance::Dark
                                | flui_platform::WindowAppearance::VibrantDark
                        ),
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
                        .own_frame(decoration, configure.states)
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
            self.input.detach(self.id);
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
            size.map_or((0, 0), |size| {
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
            (extent(constraints.min), extent(constraints.max))
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
    /// carries. The user data of each `wp_presentation_feedback`.
    #[derive(Clone)]
    pub(crate) struct PresentedTo {
        time: PresentationTime,
        reports: PresentationReports,
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
        /// compositor has shown this one, or the grace after which a present
        /// that never committed is given up (`frame::CALLBACK_GRACE`). Returns
        /// at once; one callback is out at a time. Where the compositor offers
        /// `wp_presentation`, every present also asks when its commit is
        /// shown ([`presented`]).
        fn pre_present_notify(&self, _: flui_platform::SwapChainStatistics) {
            if self.retirement.is_retired() {
                return;
            }
            if let Some(presented) = &self.presentation {
                let _feedback = presented.time.manager.feedback(
                    &self.surface.0,
                    &self.queue,
                    presented.clone(),
                );
            }
            let now = Instant::now();
            {
                let mut armed = self.record.callback_armed.lock();
                if waiting_on_callback(*armed, now) {
                    return;
                }
                *armed = Some(now);
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
                presentation: self.presentation.clone().map(|time| PresentedTo {
                    time,
                    reports: self.delivery.presentation_reports(),
                }),
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
                WindowCommand::SetDecorated(decorated) => {
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
                        return Err(not_carried_out(&WindowCommand::SetDecorated(decorated)));
                    } else {
                        drop(roles);
                    }
                    self.applied.borrow_mut().decoration_requested = decorated;
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
                // `admit` refuses what a Wayland window cannot do.
                other => return Err(not_carried_out(&other)),
            }
            // After the command's requests: what the window decides itself
            // shows now, what the compositor decides with its configure.
            self.refresh_state();
            Ok(())
        }

        /// The cursor is the seats' input's, which shows it through
        /// `cursor-shape` while the pointer is over the window.
        fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
            self.input.set_cursor(self.id, cursor)
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
    }

    /// What the Wayland host's windows share and open with: what every
    /// window hands out of the window-level positions. Public where it is
    /// defined, as the host's table names it, and nameable only inside the
    /// crate.
    pub struct WaylandWindows {
        positions: WindowPositions<SurfaceRef>,
        /// The fonts the windows draw their own titles with.
        title_fonts: TitleFonts,
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
            Self {
                positions,
                title_fonts: TitleFonts::default(),
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

        fn active(owner: &WaylandLoop, _: &WaylandWindows) -> Option<WindowId> {
            owner.active_window()
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
            presented::reported(
                capabilities(
                    None,
                    decoration.absence(),
                    ToplevelStates::default(),
                    if owner.offers_activation() {
                        Activation::Offered
                    } else {
                        Activation::Missing
                    },
                ),
                owner.offers_presentation(),
            )
        }

        /// A frame the compositor draws lies outside the window's geometry;
        /// where it offers none, a window draws its own title bar above its
        /// content. Where it offers frames, its answer comes with the
        /// window, so the inset is the frame it is asked for: none.
        fn frame_inset(owner: &WaylandLoop, _: &WaylandWindows, _: f64) -> Size<f64> {
            if !owner.offers_decorations() && owner.offers_frame_surfaces() {
                Size::new(0.0, client_frame::HEADER)
            } else {
                Size::new(0.0, 0.0)
            }
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
                *record.callback_armed.lock() = None;
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

    impl Dispatch<WpPresentationFeedback, PresentedTo> for WaylandState {
        /// A commit was shown: its time, on the compositor's presentation
        /// clock, is rebased onto the event clock by its distance from a
        /// reading of that clock, and reported with the output's refresh. A
        /// discarded commit (another replaced it before a refresh) reports
        /// nothing; the compositor destroys the feedback after either.
        fn event(
            _: &mut Self,
            _: &WpPresentationFeedback,
            event: wp_presentation_feedback::Event,
            presented_to: &PresentedTo,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            let wp_presentation_feedback::Event::Presented {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
                refresh,
                ..
            } = event
            else {
                return;
            };
            let (Some(shown), Some(now)) = (
                presented::shown_at(tv_sec_hi, tv_sec_lo, tv_nsec),
                presented_to.time.clock.now(),
            ) else {
                return;
            };
            // A zero refresh, a variable rate, is none (`FramePresented::with_refresh`).
            let report = flui_platform::FramePresented::new(rebased(now, shown))
                .with_refresh(std::time::Duration::from_nanos(u64::from(refresh)));
            presented_to.reports.report(report);
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
                record.protocol.lock().fractional_scale = Some(scale);
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
                preferred_scale: told.preferred_scale,
                fractional_scale: told.fractional_scale,
                frame_mode: told.raw_frame_mode,
                outputs: told.outputs.clone(),
                activation_tokens,
                activations,
            }
        }

        /// The frame the window draws itself: the window geometry it sent
        /// last, each piece shown with its rectangle relative to the content,
        /// and the moves, resizes and menus it carried out with the serial
        /// each named.
        pub(crate) fn client_frame_said(&self) -> ClientFrameSaid {
            ClientFrameSaid {
                geometry: self.applied.borrow().geometry,
                pieces: self
                    .client_frame
                    .borrow()
                    .as_ref()
                    .map(ClientFrame::shown)
                    .unwrap_or_default(),
                carried: self.frame_record.carried_out(),
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
            surface.damage_buffer(0, 0, width, height);
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
