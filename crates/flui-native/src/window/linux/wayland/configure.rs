//! What a configure from the compositor means for a Wayland window: the
//! toplevel's states, the size the window takes, and the capabilities the
//! compositor announces (xdg-shell).

#![forbid(unsafe_code)]

use flui_foundation::geometry::Size;
use flui_platform::{CapabilityAbsence, SizeConstraints, WindowCapabilities, WindowCapability};

use crate::window::state::ModeFlags;

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

    /// The facts the window's mode is decided from
    /// ([`mode_of`](crate::window::state::mode_of)). A Wayland client
    /// never hears that its window was minimized, so it never says so.
    pub(crate) const fn mode_flags(self) -> ModeFlags {
        ModeFlags {
            minimized: false,
            fullscreen: self.fullscreen,
            maximized: self.maximized,
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
/// frame turned on and off (`decoration`, `None` where it can), why it
/// cannot take a title bar of the application's (`app_title_bar`, `None`
/// where it can: its own frame keeps the margins, ADR-0286 §11) and
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
/// to offer every mode, as the window cannot know which it ignores. The
/// window reports a covering where its `xdg_toplevel` is version 6 or newer
/// (`tells_suspension`): the compositor then suspends a toplevel it does not
/// show, covered by other windows among the reasons xdg-shell names, and the
/// window reads occluded; before it, a covering is never told.
pub(crate) fn capabilities(
    announced: Option<WmCapabilities>,
    decoration: Option<CapabilityAbsence>,
    app_title_bar: Option<CapabilityAbsence>,
    states: ToplevelStates,
    activation: Activation,
    tells_suspension: bool,
) -> WindowCapabilities {
    use WindowCapability::{Absent, Emulated, Native};

    let on_this_device = Absent(CapabilityAbsence::NOT_ON_THIS_DEVICE);
    let offered = |mode: fn(WmCapabilities) -> bool| {
        if announced.is_none_or(mode) {
            Native
        } else {
            on_this_device
        }
    };
    // Each capability the window has is named, so one the contract gains
    // is absent here until this host carries it out. What stays as
    // `NONE` has it is the platform's: the position, hiding and showing,
    // a material, the keyboard's geometry (the text-input protocol
    // reports none) and system gesture edges (a desktop compositor keeps
    // none), with nothing observed of the position or of a minimized
    // window, no placement at open and no refresh-rate request. The
    // presentation report is the compositor's to offer
    // (`presentation::reported`).
    let mut capabilities = WindowCapabilities::NONE.into_parts();
    capabilities.sets_title = Native;
    capabilities.sets_constraints = Native;
    capabilities.closes = Native;
    capabilities.sets_size = if states.tiled_by_compositor() {
        on_this_device
    } else {
        Native
    };
    capabilities.sets_resizable = Emulated;
    capabilities.sets_frame = decoration.map_or(Native, Absent);
    capabilities.app_title_bar = app_title_bar.map_or(Native, Absent);
    capabilities.activates = match activation {
        Activation::Offered => Emulated,
        Activation::Missing => on_this_device,
    };
    capabilities.minimizes = offered(|announced| announced.minimize);
    capabilities.maximizes = offered(|announced| announced.maximize);
    capabilities.enters_fullscreen = offered(|announced| announced.fullscreen);
    capabilities.reports_occlusion = if tells_suspension {
        Native
    } else {
        on_this_device
    };
    capabilities.into_capabilities()
}

/// `size` within `constraints`, the maximum before the minimum, and at
/// least one logical pixel each way.
pub(crate) fn clamp(size: Size<f64>, constraints: SizeConstraints) -> Size<f64> {
    let mut width = size.width;
    let mut height = size.height;
    if let Some(max) = constraints.max {
        width = width.min(max.width());
        height = height.min(max.height());
    }
    if let Some(min) = constraints.min {
        width = width.max(min.width());
        height = height.max(min.height());
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

/// The size the compositor recommends a window's content stays within
/// (`xdg_toplevel.configure_bounds`, version 4: the work area of the
/// output, say), each axis in logical pixels; an axis the compositor
/// gives as zero has no bound. The default bounds nothing, as before the
/// compositor sends any.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Bounds {
    width: Option<f64>,
    height: Option<f64>,
}

impl Bounds {
    /// The bounds of a `configure_bounds` of `width` by `height`: an
    /// axis that is zero or less is unknown.
    pub(crate) fn told(width: i32, height: i32) -> Self {
        let axis = |extent: i32| (extent > 0).then(|| f64::from(extent));
        Self {
            width: axis(width),
            height: axis(height),
        }
    }

    /// `size` held within the bounds, each axis on its own.
    fn hold(self, size: Size<f64>) -> Size<f64> {
        let axis = |extent: f64, bound: Option<f64>| bound.map_or(extent, |b| extent.min(b));
        Size::new(axis(size.width, self.width), axis(size.height, self.height))
    }
}

/// What a configure gives the window geometry: `xdg_toplevel.configure`'s
/// size (an axis of zero left to the window) and states, and the
/// `configure_bounds` before it (an axis of zero bounds nothing).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct GeometryConfigure {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) states: ToplevelStates,
    pub(crate) bounds: (i32, i32),
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
    /// Whether the normal size is still the one the window opened at:
    /// no configure in normal mode and no request has set it since, so
    /// the compositor's bounds hold it.
    at_open_size: bool,
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
            at_open_size: true,
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

    /// Take `told`, a configure of the window geometry, whose content is the
    /// geometry less what `content_of` takes off (the title bar of a frame
    /// the window draws itself): off its size and off its bounds alike.
    /// Returns whether the content size changed.
    pub(crate) fn configure_geometry(
        &mut self,
        told: GeometryConfigure,
        constraints: SizeConstraints,
        content_of: impl Fn(i32, i32) -> (i32, i32),
    ) -> bool {
        let (width, height) = content_of(told.width, told.height);
        let (bound_width, bound_height) = content_of(told.bounds.0, told.bounds.1);
        self.configure(
            width,
            height,
            told.states,
            constraints,
            Bounds::told(bound_width, bound_height),
        )
    }

    /// Take a configure of `width` by `height` content with `states`. Each
    /// axis is zero on its own where the compositor leaves it to the client
    /// (xdg-shell), and keeps the window's own extent, held within `bounds`
    /// while that is still the size the window opened at. A window the
    /// compositor sizes takes the size it is given; one in normal mode
    /// takes a size asked for meanwhile, else the compositor's suggestion,
    /// else returns to the size it had in normal mode, each held within
    /// `constraints` (limits changed while the compositor sized the window
    /// hold the size it returns to). Returns whether the content size
    /// changed.
    pub(crate) fn configure(
        &mut self,
        width: i32,
        height: i32,
        states: ToplevelStates,
        constraints: SizeConstraints,
        bounds: Bounds,
    ) -> bool {
        let before = self.content;
        let given = |extent: i32, own: f64| {
            if extent > 0 { f64::from(extent) } else { own }
        };
        // The extents the window falls back on: the normal size once the
        // compositor lets go of it, its current size otherwise. The
        // size the window opened at is a request the compositor's bounds
        // hold, unlike one the window took since.
        let own = if self.states.sized_by_compositor() && !states.sized_by_compositor() {
            self.normal
        } else {
            self.content
        };
        let own = if self.at_open_size && !states.sized_by_compositor() {
            bounds.hold(own)
        } else {
            own
        };
        if states.sized_by_compositor() {
            self.content = Size::new(given(width, own.width), given(height, own.height));
        } else if let Some(size) = self.pending.take() {
            self.content = clamp(size, constraints);
        } else {
            self.content = clamp(
                Size::new(given(width, own.width), given(height, own.height)),
                constraints,
            );
        }
        if !states.sized_by_compositor() {
            self.normal = self.content;
            self.at_open_size = false;
        }
        self.states = states;
        self.content != before
    }

    /// The client asks for `size`. In normal mode it is the size at once;
    /// while the compositor sizes the window it is taken when the
    /// compositor stops, and this returns `true`: the window asks to
    /// leave the mode. Either way the window's size is its own from
    /// here: the compositor's bounds no longer hold it.
    pub(crate) fn request(&mut self, size: Size<f64>, constraints: SizeConstraints) -> bool {
        self.at_open_size = false;
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
    use flui_platform::{WindowCommand, WindowFrame, WindowMode};

    use super::*;
    use crate::window::state::mode_of;

    /// Limits with these bounds.
    fn bounded(min: Option<Size<f64>>, max: Option<Size<f64>>) -> SizeConstraints {
        SizeConstraints::try_new(min, max).expect("finite limits")
    }

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
        assert_eq!(mode_of(states.mode_flags()), WindowMode::Maximized);
        let both = ToplevelStates::decode(&array(&[MAXIMIZED, FULLSCREEN]));
        assert_eq!(mode_of(both.mode_flags()), WindowMode::Fullscreen);
        let tiled = ToplevelStates::decode(&array(&[TILED_BOTTOM, SUSPENDED]));
        assert!(tiled.tiled.any() && tiled.suspended && tiled.sized_by_compositor());
        assert_eq!(mode_of(tiled.mode_flags()), WindowMode::Normal);
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
            !sizing.configure(0, 0, normal, none, Bounds::default()),
            "0x0: the client decides"
        );
        assert_eq!(sizing.content(), Size::new(800.0, 600.0));

        assert!(sizing.configure(1920, 1080, maximized, none, Bounds::default()));
        assert_eq!(sizing.content(), Size::new(1920.0, 1080.0));

        assert!(
            sizing.request(Size::new(640.0, 480.0), none),
            "leave the mode first"
        );
        assert_eq!(sizing.content(), Size::new(1920.0, 1080.0), "not yet");
        assert!(sizing.configure(1900, 1000, normal, none, Bounds::default()));
        assert_eq!(
            sizing.content(),
            Size::new(640.0, 480.0),
            "the size asked for wins over the compositor's suggestion"
        );

        assert!(sizing.configure(1920, 1080, maximized, none, Bounds::default()));
        assert!(sizing.configure(0, 0, normal, none, Bounds::default()));
        assert_eq!(
            sizing.content(),
            Size::new(640.0, 480.0),
            "back to the normal size"
        );

        let limits = bounded(
            Some(Size::new(200.0, 150.0)),
            Some(Size::new(1000.0, 700.0)),
        );
        assert!(sizing.configure(1200, 100, normal, limits, Bounds::default()));
        assert_eq!(
            sizing.content(),
            Size::new(1000.0, 150.0),
            "within the limits"
        );
        assert!(!sizing.request(Size::new(50.0, 50.0), limits));
        assert_eq!(sizing.content(), Size::new(200.0, 150.0));
        // Negative sizes are no size.
        assert!(!sizing.configure(-5, 40, normal, limits, Bounds::default()));
        assert_eq!(sizing.states(), normal);

        // New limits hold a window in normal mode, never one the
        // compositor sizes.
        sizing.constrain(bounded(Some(Size::new(300.0, 300.0)), None));
        assert_eq!(sizing.content(), Size::new(300.0, 300.0));
        assert!(sizing.configure(1920, 1080, maximized, none, Bounds::default()));
        sizing.constrain(bounded(None, Some(Size::new(100.0, 100.0))));
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
        assert!(sizing.configure(800, 0, normal, none, Bounds::default()));
        assert_eq!(sizing.content(), Size::new(800.0, 480.0));
        assert!(sizing.configure(0, 300, normal, none, Bounds::default()));
        assert_eq!(sizing.content(), Size::new(800.0, 300.0));
        assert!(sizing.configure(1920, 0, maximized, none, Bounds::default()));
        assert_eq!(sizing.content(), Size::new(1920.0, 300.0));
        assert!(sizing.configure(0, 0, normal, none, Bounds::default()));
        assert_eq!(
            sizing.content(),
            Size::new(800.0, 300.0),
            "the normal size back"
        );
    }

    /// A window asked for larger than the compositor's bounds (the work
    /// area of a 1366 by 768 laptop) opens within them when the first
    /// configure leaves the size to it, each axis on its own and within
    /// its limits. The bounds are a hint: a size asked for later, the
    /// size the window has since taken, and a size the compositor gives
    /// are not held to them.
    #[test]
    fn a_window_opens_within_the_compositors_bounds() {
        let none = SizeConstraints::NONE;
        let normal = ToplevelStates::default();
        let maximized = ToplevelStates {
            maximized: true,
            ..ToplevelStates::default()
        };
        let laptop = Bounds::told(1366, 740);
        let opened = || Sizing::new(Size::new(1600.0, 1000.0));

        let mut sizing = opened();
        assert!(sizing.configure(0, 0, normal, none, laptop));
        assert_eq!(sizing.content(), Size::new(1366.0, 740.0));
        assert!(!sizing.request(Size::new(1600.0, 1000.0), none));
        assert!(!sizing.configure(0, 0, normal, none, laptop));
        assert_eq!(
            sizing.content(),
            Size::new(1600.0, 1000.0),
            "a size asked for after the open is not held"
        );

        let mut sizing = opened();
        assert!(!sizing.configure(0, 0, normal, none, Bounds::told(0, 0)));
        assert_eq!(sizing.content(), Size::new(1600.0, 1000.0), "no bounds");
        assert!(!sizing.configure(0, 0, normal, none, laptop));
        assert_eq!(
            sizing.content(),
            Size::new(1600.0, 1000.0),
            "bounds told after the window took its size"
        );

        let mut sizing = opened();
        assert!(sizing.configure(0, 0, normal, none, Bounds::told(2560, 900)));
        assert_eq!(sizing.content(), Size::new(1600.0, 900.0), "one axis");

        let mut sizing = opened();
        assert!(sizing.configure(1200, 0, normal, none, laptop));
        assert_eq!(
            sizing.content(),
            Size::new(1200.0, 740.0),
            "the axis left to the window"
        );

        let mut sizing = opened();
        let limits =
            SizeConstraints::try_new(Some(Size::new(800.0, 800.0)), None).expect("finite limits");
        assert!(sizing.configure(0, 0, normal, limits, laptop));
        assert_eq!(
            sizing.content(),
            Size::new(1366.0, 800.0),
            "the window's limits over the bounds"
        );

        let mut sizing = opened();
        assert!(sizing.configure(1920, 1080, maximized, none, laptop));
        assert_eq!(
            sizing.content(),
            Size::new(1920.0, 1080.0),
            "the compositor's size"
        );
        assert!(sizing.configure(0, 0, normal, none, laptop));
        assert_eq!(
            sizing.content(),
            Size::new(1366.0, 740.0),
            "the size it opened at, held, once it is the window's again"
        );

        // The bounds and the size are the window geometry's: a title bar the
        // window draws itself comes off both.
        let title_bar = |width: i32, height: i32| {
            if height > 0 {
                (width, (height - 37).max(1))
            } else {
                (width, height)
            }
        };
        let mut sizing = opened();
        let first = GeometryConfigure {
            bounds: (1366, 740),
            ..GeometryConfigure::default()
        };
        assert!(sizing.configure_geometry(first, none, title_bar));
        assert_eq!(
            sizing.content(),
            Size::new(1366.0, 703.0),
            "the bounds less the title bar"
        );
        let maximized_geometry = GeometryConfigure {
            width: 1920,
            height: 1080,
            states: maximized,
            bounds: (1366, 740),
        };
        assert!(sizing.configure_geometry(maximized_geometry, none, title_bar));
        assert_eq!(sizing.content(), Size::new(1920.0, 1043.0));
    }

    /// Every capability of a Wayland window whose compositor offers
    /// everything, named: a capability the contract gains does not
    /// compile here until this host says whether its window has it, and
    /// none is had for not being taken away. The presentation report is
    /// the compositor's (`presented::reported`).
    #[test]
    fn a_wayland_window_has_the_capabilities_it_names() {
        use WindowCapability::{Emulated, Native};
        use flui_platform::{Availability, CapabilityParts};

        let every_mode = WmCapabilities {
            window_menu: true,
            maximize: true,
            fullscreen: true,
            minimize: true,
        };
        let honoured = capabilities(
            Some(every_mode),
            None,
            None,
            ToplevelStates::default(),
            Activation::Offered,
            true,
        );
        assert_eq!(
            honoured.into_parts(),
            CapabilityParts {
                sets_title: Native,
                sets_position: WindowCapability::NOT_ON_THIS_PLATFORM,
                sets_size: Native,
                sets_constraints: Native,
                sets_resizable: Emulated,
                sets_frame: Native,
                app_title_bar: Native,
                minimizes: Native,
                maximizes: Native,
                enters_fullscreen: Native,
                hides: WindowCapability::NOT_ON_THIS_PLATFORM,
                shows: WindowCapability::NOT_ON_THIS_PLATFORM,
                activates: Emulated,
                closes: Native,
                material: WindowCapability::NOT_ON_THIS_PLATFORM,
                reports_presentation: WindowCapability::NOT_ON_THIS_PLATFORM,
                reports_keyboard_inset: WindowCapability::NOT_ON_THIS_PLATFORM,
                reports_system_gestures: WindowCapability::NOT_ON_THIS_PLATFORM,
                reports_occlusion: Native,
                observes_position: Availability::Unavailable,
                observes_minimized: Availability::Unavailable,
                places_at_open: Availability::Unavailable,
                frame_rate_hint: Availability::Unavailable,
            }
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
            Some(CapabilityAbsence::NOT_ON_THIS_DEVICE),
            Some(CapabilityAbsence::NOT_ON_THIS_DEVICE),
            free,
            Activation::Missing,
            false,
        );
        assert!(unannounced.maximizes().is_available());
        assert!(unannounced.minimizes().is_available());
        assert_eq!(
            unannounced.reports_occlusion(),
            WindowCapability::Absent(CapabilityAbsence::NOT_ON_THIS_DEVICE),
            "a toplevel before version 6 is never told it is suspended, so never that it is covered"
        );
        assert!(unannounced.sets_size().is_available());
        assert_eq!(unannounced.sets_resizable(), WindowCapability::Emulated);
        assert_eq!(
            capabilities(None, None, None, free, Activation::Missing, false).sets_frame(),
            WindowCapability::Native,
            "a compositor that draws frames takes the command"
        );
        assert_eq!(
            capabilities(None, None, None, free, Activation::Offered, false).activates(),
            WindowCapability::Emulated,
            "the compositor decides what an activation request does"
        );
        assert!(
            capabilities(None, None, None, free, Activation::Missing, false)
                .app_title_bar()
                .is_available(),
            "a window that draws its own margins takes the application's title bar"
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
                WindowCommand::SetFrame(WindowFrame::None),
                flui_platform::UnsupportedReason::NotOnThisDevice,
            ),
            (
                WindowCommand::Activate,
                flui_platform::UnsupportedReason::NotOnThisDevice,
            ),
            (
                WindowCommand::SetFrame(WindowFrame::AppTitleBar(
                    flui_platform::AppTitleBar::default(),
                )),
                flui_platform::UnsupportedReason::NotOnThisDevice,
            ),
        ] {
            let refused = command
                .clone()
                .admit(
                    &unannounced
                        .publish()
                        .expect("the process has publications left"),
                )
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
            None,
            free,
            Activation::Offered,
            false,
        );
        assert!(!announced.maximizes().is_available());
        assert!(!announced.minimizes().is_available());
        assert!(announced.enters_fullscreen().is_available());
        assert_eq!(
            WindowCommand::SetMode(WindowMode::Maximized)
                .admit(
                    &announced
                        .publish()
                        .expect("the process has publications left")
                )
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
            .admit(
                &capabilities(None, None, None, tiled, Activation::Missing, false)
                    .publish()
                    .expect("the process has publications left"),
            )
            .expect_err("the tiling holds the size");
        assert_eq!(
            refused.unsupported().map(|unsupported| unsupported.reason),
            Some(flui_platform::UnsupportedReason::NotOnThisDevice)
        );
        let maximized = ToplevelStates::decode(&array(&[TILED_LEFT, MAXIMIZED]));
        assert!(
            capabilities(None, None, None, maximized, Activation::Missing, false)
                .sets_size()
                .is_available()
        );
        let floating = ToplevelStates::decode(&array(&[ACTIVATED]));
        assert!(
            capabilities(None, None, None, floating, Activation::Missing, false)
                .sets_size()
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

    #[cfg(not(target_arch = "wasm32"))]
    proptest::proptest! {
        /// A configure's states array and a capabilities announcement read
        /// as the whole values they hold, whatever the compositor wrote: a
        /// trailing partial value is ignored and an unknown one skipped,
        /// without a panic.
        #[test]
        fn any_array_reads_as_its_whole_values(
            raw in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..128),
        ) {
            let whole = &raw[..raw.len() / 4 * 4];
            proptest::prop_assert_eq!(ToplevelStates::decode(&raw), ToplevelStates::decode(whole));
            proptest::prop_assert_eq!(WmCapabilities::decode(&raw), WmCapabilities::decode(whole));
        }
    }
}
