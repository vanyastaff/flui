//! The X11 window: the native X11 host's window, and what the winit host
//! reads of an X11 window that winit does not report.
//!
//! The pure half reads and writes what the X server and the window manager
//! keep of a window, and is checked on every host:
//!
//! - [`mode`]: the window's mode from the two properties a window manager
//!   keeps for it (`WM_STATE`, `_NET_WM_STATE`), and the `_NET_WM_STATE` a
//!   window not mapped yet asks a mode with;
//! - [`support`]: what a window honours, from what the window manager says it
//!   supports (`_NET_SUPPORTING_WM_CHECK`, `_NET_SUPPORTED`): with no window
//!   manager, nothing minimizes, maximizes or enters fullscreen;
//! - [`hints`]: the size limits a window asks in `WM_NORMAL_HINTS`, and the
//!   decorations in `_MOTIF_WM_HINTS`;
//! - [`placement`]: the window's outer rectangle from its content and the
//!   frame the window manager put around it, the display it is on, and
//!   logical numbers as the 16-bit coordinates and extents X11 takes.
//!
//! The live half:
//!
//! - `window` (the native X11 host): [`X11Window`], one window of the
//!   platform's connection, and [`X11Windows`], the platform's windows, which
//!   the loop holds as its `LoopWindows` (`owner::linux::x11`);
//! - `probe` (the winit host): reads the mode properties of a winit window
//!   from the server and watches them change, answering the window model's
//!   `WindowManagerView`, which the winit host connects to the winit window.

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
mod probe;
#[cfg(all(target_os = "linux", feature = "x11"))]
mod window;

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
pub(crate) use probe::connect_window_manager;
#[cfg(all(target_os = "linux", feature = "x11"))]
pub(crate) use window::WindowAtoms;
#[cfg(all(target_os = "linux", feature = "x11"))]
pub use window::{X11Window, X11Windows};

// The winit window on a real X server, over a running winit loop.
#[cfg(all(test, target_os = "linux", feature = "winit-backend"))]
mod tests;

pub(crate) mod mode {
    //! The mode of an X11 window, from the two properties a window manager keeps
    //! for it.
    //!
    //! winit reports a window as maximized or fullscreen but not as minimized, and
    //! its answers for the other two lag the window manager by an event. The
    //! adapter reads the properties directly, this model reads what they say
    //! (`mode_flags`) and `window::state::mode_of` decides the mode.
    //!
    //! - **`WM_STATE`** (ICCCM) is the window manager's statement of whether the
    //!   window is mapped: normal, iconic (minimized) or withdrawn (not mapped).
    //!   It is the authority for minimized.
    //! - **`_NET_WM_STATE`** (EWMH) is a list of atoms. `_NET_WM_STATE_HIDDEN`
    //!   means the window would not be visible on screen, which a minimized window
    //!   is but so can be one on another desktop; it decides minimized only for a
    //!   window manager that sets no `WM_STATE`. The fullscreen atom and the two
    //!   maximized atoms (one per axis) decide those modes.
    //!
    //! Atoms are numbers the server hands out at run time, so the adapter passes
    //! the numbers it interned in [`NetWmStateAtoms`].

    #![forbid(unsafe_code)]

    use crate::window::state::ModeFlags;

    /// The `_NET_WM_STATE` atoms this model reads, as the server numbered them.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct NetWmStateAtoms {
        pub hidden: u32,
        pub maximized_horz: u32,
        pub maximized_vert: u32,
        pub fullscreen: u32,
    }

    /// The `_NET_WM_STATE` entries this model reads, set or not.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct NetWmState {
        pub hidden: bool,
        pub maximized_horz: bool,
        pub maximized_vert: bool,
        pub fullscreen: bool,
    }

    impl NetWmState {
        /// Read the property's atoms; atoms this model does not know are ignored.
        pub(crate) fn from_atoms(atoms: &[u32], known: &NetWmStateAtoms) -> Self {
            Self {
                hidden: atoms.contains(&known.hidden),
                maximized_horz: atoms.contains(&known.maximized_horz),
                maximized_vert: atoms.contains(&known.maximized_vert),
                fullscreen: atoms.contains(&known.fullscreen),
            }
        }
    }

    /// The window manager's statement of whether a window is mapped.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum WmState {
        /// Not mapped.
        Withdrawn,
        /// Mapped.
        Normal,
        /// Mapped as an icon: minimized.
        Iconic,
    }

    impl WmState {
        /// Read the `WM_STATE` property (`state`, `icon window`). Anything but the
        /// three states ICCCM defines (the obsolete zoom state included) is
        /// unknown, not a guess.
        pub(crate) fn from_property(words: &[u32]) -> Option<Self> {
            match words.first()? {
                0 => Some(Self::Withdrawn),
                1 => Some(Self::Normal),
                3 => Some(Self::Iconic),
                _ => None,
            }
        }
    }

    /// What the two properties say of the window, which `window::state::mode_of`
    /// decides the mode from. `wm_state` is `None` when the window manager sets
    /// none.
    ///
    /// `WM_STATE` decides minimized, and `_NET_WM_STATE_HIDDEN` only when it is
    /// missing. A window maximized on one axis only is tiled or snapped, not
    /// maximized.
    pub(crate) fn mode_flags(wm_state: Option<WmState>, net: NetWmState) -> ModeFlags {
        ModeFlags {
            minimized: match wm_state {
                Some(state) => state == WmState::Iconic,
                None => net.hidden,
            },
            fullscreen: net.fullscreen,
            maximized: net.maximized_horz && net.maximized_vert,
        }
    }

    /// The `_NET_WM_STATE` a window not mapped yet holds to be mapped in
    /// `mode`, from the atoms it holds now (`current`): the maximized and
    /// fullscreen atoms replaced, every other kept, as EWMH has a client do for
    /// a window the window manager does not manage yet. [`WindowMode::Normal`]
    /// clears the mode, and so does [`WindowMode::Minimized`], which is no mode
    /// to be mapped in (a window is minimized after it is mapped). `None` when
    /// the property already says so, or for a mode this model does not know:
    /// a write of what is there would still tell every watcher of the window
    /// that something changed.
    pub(crate) fn with_mode(
        current: &[u32],
        mode: flui_platform::WindowMode,
        known: &NetWmStateAtoms,
    ) -> Option<Vec<u32>> {
        use flui_platform::WindowMode;
        let wanted: &[u32] = match mode {
            WindowMode::Normal | WindowMode::Minimized => &[],
            WindowMode::Maximized => &[known.maximized_horz, known.maximized_vert],
            WindowMode::Fullscreen => &[known.fullscreen],
            _ => return None,
        };
        let mode_atoms = [known.maximized_horz, known.maximized_vert, known.fullscreen];
        let mut atoms = current.to_vec();
        atoms.retain(|atom| !mode_atoms.contains(atom));
        atoms.extend_from_slice(wanted);
        (atoms != current).then_some(atoms)
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::WindowMode;

        use super::*;
        use crate::window::state::mode_of;

        const KNOWN: NetWmStateAtoms = NetWmStateAtoms {
            hidden: 301,
            maximized_horz: 302,
            maximized_vert: 303,
            fullscreen: 304,
        };

        /// An atom of some other state (`_NET_WM_STATE_FOCUSED`, `…_ABOVE`, …).
        const OTHER: u32 = 999;

        #[test]
        fn x11_window_state_parse() {
            use WindowMode::{Fullscreen, Maximized, Minimized, Normal};
            use WmState::{Iconic, Normal as Mapped, Withdrawn};

            // (case, `WM_STATE`, `_NET_WM_STATE` atoms, mode)
            let table: &[(&str, Option<WmState>, &[u32], WindowMode)] = &[
                ("no properties at all", None, &[], Normal),
                ("mapped, no state atoms", Some(Mapped), &[], Normal),
                (
                    "only atoms this model ignores",
                    Some(Mapped),
                    &[OTHER],
                    Normal,
                ),
                (
                    "maximized on both axes",
                    Some(Mapped),
                    &[302, 303],
                    Maximized,
                ),
                (
                    "maximized, atoms in the other order",
                    Some(Mapped),
                    &[303, OTHER, 302],
                    Maximized,
                ),
                ("maximized, no WM_STATE", None, &[302, 303], Maximized),
                ("horizontal only is tiled", Some(Mapped), &[302], Normal),
                ("vertical only is tiled", Some(Mapped), &[303], Normal),
                ("fullscreen", Some(Mapped), &[304], Fullscreen),
                (
                    "fullscreen beats maximized",
                    Some(Mapped),
                    &[302, 303, 304],
                    Fullscreen,
                ),
                ("iconic is minimized", Some(Iconic), &[301], Minimized),
                (
                    "iconic without the hidden atom",
                    Some(Iconic),
                    &[],
                    Minimized,
                ),
                (
                    "minimized beats fullscreen",
                    Some(Iconic),
                    &[304],
                    Minimized,
                ),
                (
                    "minimized beats maximized",
                    Some(Iconic),
                    &[302, 303],
                    Minimized,
                ),
                (
                    "hidden without WM_STATE is minimized",
                    None,
                    &[301],
                    Minimized,
                ),
                (
                    "hidden on another desktop stays mapped",
                    Some(Mapped),
                    &[301],
                    Normal,
                ),
                (
                    "hidden and maximized, mapped",
                    Some(Mapped),
                    &[301, 302, 303],
                    Maximized,
                ),
                (
                    "withdrawn is not minimized",
                    Some(Withdrawn),
                    &[301],
                    Normal,
                ),
                (
                    "withdrawn keeps its maximized state",
                    Some(Withdrawn),
                    &[302, 303],
                    Maximized,
                ),
            ];
            for (case, wm_state, atoms, expected) in table {
                let net = NetWmState::from_atoms(atoms, &KNOWN);
                assert_eq!(mode_of(mode_flags(*wm_state, net)), *expected, "{case}");
            }
        }

        #[test]
        fn net_wm_state_reads_only_known_atoms() {
            assert_eq!(NetWmState::from_atoms(&[], &KNOWN), NetWmState::default());
            assert_eq!(
                NetWmState::from_atoms(&[OTHER, 0, u32::MAX], &KNOWN),
                NetWmState::default()
            );
            assert_eq!(
                NetWmState::from_atoms(&[304, 301, 301], &KNOWN),
                NetWmState {
                    hidden: true,
                    fullscreen: true,
                    ..NetWmState::default()
                }
            );
        }

        #[test]
        fn wm_state_property_words() {
            // (case, property words, state)
            let table: &[(&str, &[u32], Option<WmState>)] = &[
                ("withdrawn", &[0, 0], Some(WmState::Withdrawn)),
                ("normal", &[1, 0], Some(WmState::Normal)),
                ("iconic", &[3, 0x0040_0001], Some(WmState::Iconic)),
                ("state word alone", &[3], Some(WmState::Iconic)),
                ("obsolete zoom state", &[2, 0], None),
                ("unknown state", &[7, 0], None),
                ("huge state", &[u32::MAX, 0], None),
                ("empty property", &[], None),
            ];
            for (case, words, expected) in table {
                assert_eq!(WmState::from_property(words), *expected, "{case}");
            }
        }

        /// A window not mapped yet asks for its mode in `_NET_WM_STATE`: the
        /// mode atoms are replaced, the others kept, and a property that
        /// already says so is not written again.
        #[test]
        fn an_unmapped_window_asks_its_mode_in_its_state_property() {
            use WindowMode::{Fullscreen, Maximized, Minimized, Normal};

            /// A case, the atoms held, the mode, the atoms written.
            type Row = (
                &'static str,
                &'static [u32],
                WindowMode,
                Option<&'static [u32]>,
            );

            let table: &[Row] = &[
                ("maximized from nothing", &[], Maximized, Some(&[302, 303])),
                ("fullscreen from nothing", &[], Fullscreen, Some(&[304])),
                ("normal from nothing", &[], Normal, None),
                ("minimized is no mode to map in", &[], Minimized, None),
                ("maximized again", &[302, 303], Maximized, None),
                (
                    "maximized, atoms in the other order",
                    &[303, 302],
                    Maximized,
                    Some(&[302, 303]),
                ),
                (
                    "fullscreen replaces maximized",
                    &[302, 303],
                    Fullscreen,
                    Some(&[304]),
                ),
                ("normal clears fullscreen", &[304], Normal, Some(&[])),
                (
                    "minimized clears maximized",
                    &[302, 303],
                    Minimized,
                    Some(&[]),
                ),
                (
                    "another state is kept",
                    &[OTHER, 304],
                    Maximized,
                    Some(&[OTHER, 302, 303]),
                ),
                ("only another state, normal", &[OTHER], Normal, None),
            ];
            for (case, current, mode, expected) in table {
                assert_eq!(
                    with_mode(current, *mode, &KNOWN).as_deref(),
                    *expected,
                    "{case}"
                );
            }
        }
    }
}

pub(crate) mod support {
    //! What an X11 window honours, from what the window manager says it
    //! supports.
    //!
    //! A window manager announces itself with `_NET_SUPPORTING_WM_CHECK` on the
    //! root window and lists the EWMH hints it understands in
    //! `_NET_SUPPORTED`. With none, a request to minimize, maximize or enter
    //! fullscreen reaches nobody, and nothing says a window is minimized: X11
    //! has the request, this session lacks what answers it, so those
    //! capabilities are unavailable on this device. A window manager that does
    //! not list a hint leaves that hint's command unavailable the same way.
    //! Placing, sizing, showing, hiding and focusing a window the server does
    //! itself. X11 takes no frame-rate request and draws no material behind a
    //! window.

    #![forbid(unsafe_code)]

    use flui_platform::{
        Availability, CapabilityAbsence, WindowCapabilities, WindowCommand, WindowMaterial,
        WindowMode,
    };

    /// The `_NET_SUPPORTED` atoms this model reads, as the server numbered
    /// them.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct SupportAtoms {
        pub maximized_horz: u32,
        pub maximized_vert: u32,
        pub fullscreen: u32,
    }

    /// What the window manager supports, as the root window says.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct WindowManagerSupport {
        /// A window manager runs: it answers `_NET_SUPPORTING_WM_CHECK`.
        pub managed: bool,
        /// It maximizes a window on both axes.
        pub maximizes: bool,
        /// It puts a window in fullscreen.
        pub fullscreen: bool,
    }

    impl WindowManagerSupport {
        /// The support of a window manager that runs or not (`managed`) and
        /// lists `supported` in `_NET_SUPPORTED`; a list without a window
        /// manager to stand behind it (one left by a window manager that quit)
        /// says nothing.
        pub(crate) fn read(managed: bool, supported: &[u32], atoms: &SupportAtoms) -> Self {
            let lists = |atom: u32| managed && supported.contains(&atom);
            Self {
                managed,
                maximizes: lists(atoms.maximized_horz) && lists(atoms.maximized_vert),
                fullscreen: lists(atoms.fullscreen),
            }
        }
    }

    /// What an X11 window honours under `support`.
    pub(crate) fn capabilities(support: WindowManagerSupport) -> WindowCapabilities {
        let device = CapabilityAbsence::NotOnThisDevice;
        let mut capabilities = WindowCapabilities::ALL.unavailable(
            &WindowCommand::SetMaterial(WindowMaterial::System),
            CapabilityAbsence::NotOnThisPlatform,
        );
        capabilities.frame_rate_hint = Availability::Unavailable;
        if !support.managed {
            capabilities =
                capabilities.unavailable(&WindowCommand::SetMode(WindowMode::Minimized), device);
            capabilities.observes_minimized = Availability::Unavailable;
        }
        if !support.maximizes {
            capabilities =
                capabilities.unavailable(&WindowCommand::SetMode(WindowMode::Maximized), device);
        }
        if !support.fullscreen {
            capabilities =
                capabilities.unavailable(&WindowCommand::SetMode(WindowMode::Fullscreen), device);
        }
        capabilities
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::{AdmissionError, UnsupportedReason};

        use super::*;

        const ATOMS: SupportAtoms = SupportAtoms {
            maximized_horz: 302,
            maximized_vert: 303,
            fullscreen: 304,
        };

        /// What a window manager lists decides what a window honours: with no
        /// window manager, or one that does not list a hint, the command is
        /// refused as missing on this device; what the server does itself is
        /// always honoured; the material and the frame-rate hint never are.
        #[test]
        fn what_the_window_manager_supports_is_what_a_window_honours() {
            use Availability::{Native, Unavailable};

            // (case, managed, `_NET_SUPPORTED`, [minimizes, maximizes,
            //  enters fullscreen, observes minimized])
            let table: &[(&str, bool, &[u32], [Availability; 4])] = &[
                (
                    "no window manager",
                    false,
                    &[],
                    [Unavailable, Unavailable, Unavailable, Unavailable],
                ),
                (
                    "a list left by a window manager that quit",
                    false,
                    &[302, 303, 304],
                    [Unavailable, Unavailable, Unavailable, Unavailable],
                ),
                (
                    "a window manager that lists nothing",
                    true,
                    &[],
                    [Native, Unavailable, Unavailable, Native],
                ),
                (
                    "maximized on one axis only",
                    true,
                    &[302],
                    [Native, Unavailable, Unavailable, Native],
                ),
                (
                    "both axes",
                    true,
                    &[303, 302],
                    [Native, Native, Unavailable, Native],
                ),
                (
                    "everything",
                    true,
                    &[302, 303, 304, 999],
                    [Native, Native, Native, Native],
                ),
            ];
            for (case, managed, supported, [minimizes, maximizes, fullscreen, observes]) in table {
                let honoured =
                    capabilities(WindowManagerSupport::read(*managed, supported, &ATOMS));
                assert_eq!(honoured.minimizes, *minimizes, "{case}: minimizes");
                assert_eq!(honoured.maximizes, *maximizes, "{case}: maximizes");
                assert_eq!(
                    honoured.enters_fullscreen, *fullscreen,
                    "{case}: fullscreen"
                );
                assert_eq!(honoured.observes_minimized, *observes, "{case}: observes");
                for always in [
                    honoured.sets_title,
                    honoured.sets_position,
                    honoured.sets_size,
                    honoured.sets_constraints,
                    honoured.sets_resizable,
                    honoured.sets_decorated,
                    honoured.hides,
                    honoured.shows,
                    honoured.activates,
                    honoured.closes,
                    honoured.observes_position,
                    honoured.places_at_open,
                ] {
                    assert_eq!(always, Native, "{case}: what the server does itself");
                }
                assert_eq!(honoured.frame_rate_hint, Unavailable, "{case}");
                assert_eq!(honoured.material, Unavailable, "{case}");
            }
        }

        /// A refused command says where it is missing: a mode on this session,
        /// the material on X11 itself.
        #[test]
        fn a_refused_command_says_where_it_is_missing() {
            let honoured = capabilities(WindowManagerSupport::default());
            let reason = |command: WindowCommand| match command.admit(&honoured) {
                Err(AdmissionError::Unsupported(refused)) => Some(refused.reason),
                _ => None,
            };
            assert_eq!(
                reason(WindowCommand::SetMode(WindowMode::Maximized)),
                Some(UnsupportedReason::NotOnThisDevice)
            );
            assert_eq!(
                reason(WindowCommand::SetMode(WindowMode::Minimized)),
                Some(UnsupportedReason::NotOnThisDevice)
            );
            assert_eq!(
                reason(WindowCommand::SetMaterial(WindowMaterial::System)),
                Some(UnsupportedReason::NotOnThisPlatform)
            );
            assert_eq!(reason(WindowCommand::SetMode(WindowMode::Normal)), None);
        }
    }
}

pub(crate) mod placement {
    //! Where an X11 window is, in the numbers X11 takes and the window state
    //! publishes.
    //!
    //! X11 coordinates are signed 16-bit numbers and extents unsigned ones,
    //! which servers further hold to the signed range; a logical number is
    //! turned into one only when it is finite and fits, never by a saturating
    //! cast. The outer rectangle of a window is its content's, measured from
    //! the root window, grown by the frame the window manager reports
    //! (`_NET_FRAME_EXTENTS`), in 64-bit arithmetic so no frame overflows it.

    #![forbid(unsafe_code)]

    use flui_foundation::geometry::{Bounds, Point, Size};
    use flui_platform::{DisplayInfo, DisplaySnapshot, WindowGeometry};

    /// The largest extent a window is given, in device pixels.
    pub(crate) const MAX_EXTENT: u16 = 32_767;

    /// Whether `scale` is a scale: finite and positive.
    fn is_scale(scale: f64) -> bool {
        scale.is_finite() && scale > 0.0
    }

    /// `pixels`, already rounded, as the whole number it is, when it is one
    /// between `min` and `max`.
    fn whole(pixels: f64, min: i32, max: i32) -> Option<i32> {
        if !(pixels.is_finite() && pixels >= f64::from(min) && pixels <= f64::from(max)) {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a whole number between two i32 bounds, checked just above"
        )]
        let whole = pixels as i32;
        Some(whole)
    }

    /// A logical length at `scale` as a window extent in device pixels:
    /// rounded, at least one pixel and at most [`MAX_EXTENT`]. `None` for a
    /// length or scale that is not finite and positive.
    pub(crate) fn device_extent(logical: f64, scale: f64) -> Option<u16> {
        if !(is_scale(scale) && logical.is_finite() && logical > 0.0) {
            return None;
        }
        let pixels = (logical * scale).round().clamp(1.0, f64::from(MAX_EXTENT));
        u16::try_from(whole(pixels, 1, i32::from(MAX_EXTENT))?).ok()
    }

    /// A logical coordinate at `scale` as an X11 coordinate in device pixels,
    /// rounded. `None` when it is not finite or does not fit 16 bits.
    pub(crate) fn device_coordinate(logical: f64, scale: f64) -> Option<i16> {
        if !is_scale(scale) {
            return None;
        }
        let pixels = (logical * scale).round();
        i16::try_from(whole(pixels, i32::from(i16::MIN), i32::from(i16::MAX))?).ok()
    }

    /// The frame a window manager put around a window, in device pixels, as
    /// `_NET_FRAME_EXTENTS` says it: left, right, top, bottom.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct FrameExtents {
        pub left: u32,
        pub right: u32,
        pub top: u32,
        pub bottom: u32,
    }

    impl FrameExtents {
        /// The frame `_NET_FRAME_EXTENTS` holds; `None` for a property that
        /// is not four numbers.
        pub(crate) fn from_property(words: &[u32]) -> Option<Self> {
            match *words {
                [left, right, top, bottom] => Some(Self {
                    left,
                    right,
                    top,
                    bottom,
                }),
                _ => None,
            }
        }

        /// What the frame adds to the content's width and height.
        pub(crate) fn added(self) -> Size<i32> {
            let sum =
                |a: u32, b: u32| i32::try_from(u64::from(a) + u64::from(b)).unwrap_or(i32::MAX);
            Size::new(sum(self.left, self.right), sum(self.top, self.bottom))
        }
    }

    /// `value` held to what an `i32` holds.
    fn saturate(value: i64) -> i32 {
        i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
    }

    /// The outer rectangle of a window whose content's top-left corner is at
    /// `origin` on the root window and `content` large, inside `frame`, in
    /// device pixels; a side past what an `i32` holds is held at its edge.
    pub(crate) fn outer_bounds(
        origin: Point<i32>,
        content: Size<i32>,
        frame: FrameExtents,
    ) -> Bounds<i32> {
        Bounds::new(
            Point::new(
                saturate(i64::from(origin.x) - i64::from(frame.left)),
                saturate(i64::from(origin.y) - i64::from(frame.top)),
            ),
            Size::new(
                saturate(i64::from(content.width) + i64::from(frame.left) + i64::from(frame.right)),
                saturate(
                    i64::from(content.height) + i64::from(frame.top) + i64::from(frame.bottom),
                ),
            ),
        )
    }

    /// `bounds`, in device pixels, at `scale` as the logical geometry the
    /// window state publishes; a scale that is not one is taken as 1.
    pub(crate) fn logical_geometry(bounds: Bounds<i32>, scale: f64) -> WindowGeometry {
        let scale = if is_scale(scale) { scale } else { 1.0 };
        WindowGeometry::new(
            Some(Point::new(
                f64::from(bounds.origin.x) / scale,
                f64::from(bounds.origin.y) / scale,
            )),
            Size::new(
                f64::from(bounds.size.width) / scale,
                f64::from(bounds.size.height) / scale,
            ),
        )
    }

    /// The area two rectangles share, in square device pixels.
    fn overlap(a: Bounds<i32>, b: Bounds<i32>) -> i64 {
        let span = |start_a: i32, len_a: i32, start_b: i32, len_b: i32| {
            let start = i64::from(start_a).max(i64::from(start_b));
            let end = (i64::from(start_a) + i64::from(len_a.max(0)))
                .min(i64::from(start_b) + i64::from(len_b.max(0)));
            (end - start).max(0)
        };
        span(a.origin.x, a.size.width, b.origin.x, b.size.width)
            * span(a.origin.y, a.size.height, b.origin.y, b.size.height)
    }

    /// The display of `snapshot` that `bounds` overlaps most, the first of
    /// equals; `None` when it overlaps none.
    pub(crate) fn display_of(
        snapshot: &DisplaySnapshot,
        bounds: Bounds<i32>,
    ) -> Option<&DisplayInfo> {
        let mut best: Option<(i64, &DisplayInfo)> = None;
        for display in snapshot.displays() {
            let area = overlap(display.bounds, bounds);
            if area > 0 && best.is_none_or(|(most, _)| area > most) {
                best = Some((area, display));
            }
        }
        best.map(|(_, display)| display)
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::{DisplayKey, DisplayRegistry};

        use super::*;

        #[test]
        fn logical_numbers_become_x11_numbers_only_when_they_fit() {
            // (case, logical, scale, extent, coordinate)
            let table = [
                ("whole", 100.0, 1.0, Some(100), Some(100)),
                ("scaled and rounded", 10.3, 1.5, Some(15), Some(15)),
                ("a fraction of a pixel is one", 0.2, 1.0, Some(1), Some(0)),
                ("negative", -40.0, 2.0, None, Some(-80)),
                ("past the extent", 40_000.0, 1.0, Some(MAX_EXTENT), None),
                (
                    "past 16 bits by scale",
                    20_000.0,
                    2.0,
                    Some(MAX_EXTENT),
                    None,
                ),
                (
                    "the last coordinate",
                    32_767.0,
                    1.0,
                    Some(MAX_EXTENT),
                    Some(32_767),
                ),
                ("the first coordinate", -32_768.0, 1.0, None, Some(-32_768)),
                ("not a number", f64::NAN, 1.0, None, None),
                ("infinite", f64::INFINITY, 1.0, None, None),
                ("a zero scale", 10.0, 0.0, None, None),
                ("a scale that is not one", 10.0, f64::NAN, None, None),
            ];
            for (case, logical, scale, extent, coordinate) in table {
                assert_eq!(device_extent(logical, scale), extent, "{case}: extent");
                assert_eq!(
                    device_coordinate(logical, scale),
                    coordinate,
                    "{case}: coordinate"
                );
            }
        }

        #[test]
        fn the_outer_rectangle_is_the_content_grown_by_the_frame() {
            let frame = FrameExtents::from_property(&[2, 3, 30, 4]).expect("four numbers");
            assert_eq!(FrameExtents::from_property(&[1, 2, 3]), None);
            assert_eq!(frame.added(), Size::new(5, 34));
            assert_eq!(
                outer_bounds(Point::new(100, 80), Size::new(640, 480), frame),
                Bounds::new(Point::new(98, 50), Size::new(645, 514))
            );
            // A frame no real window manager reports does not overflow.
            let huge = FrameExtents {
                left: u32::MAX,
                right: u32::MAX,
                top: u32::MAX,
                bottom: u32::MAX,
            };
            assert_eq!(huge.added(), Size::new(i32::MAX, i32::MAX));
            let outer = outer_bounds(Point::new(i32::MIN, 0), Size::new(i32::MAX, 1), huge);
            assert_eq!(outer.origin.x, i32::MIN);
            assert_eq!(outer.size.width, i32::MAX);
            assert_eq!(
                logical_geometry(Bounds::new(Point::new(98, 50), Size::new(646, 514)), 2.0),
                WindowGeometry::new(Some(Point::new(49.0, 25.0)), Size::new(323.0, 257.0))
            );
            assert_eq!(
                logical_geometry(Bounds::new(Point::new(4, 2), Size::new(8, 6)), f64::NAN),
                WindowGeometry::new(Some(Point::new(4.0, 2.0)), Size::new(8.0, 6.0)),
                "a scale that is not one is 1"
            );
        }

        #[test]
        fn a_window_is_on_the_display_it_overlaps_most() {
            let registry = DisplayRegistry::new();
            let display = |name: &str, x: i32| {
                let id = registry.mint(&DisplayKey::Unkeyed).expect("an id");
                DisplayInfo::new(
                    id,
                    name,
                    Bounds::new(Point::new(x, 0), Size::new(1920, 1080)),
                    1.0,
                )
            };
            let snapshot = DisplaySnapshot::empty()
                .successor(vec![display("left", 0), display("right", 1920)])
                .expect("a snapshot");
            let on = |x: i32, width: i32| {
                display_of(
                    &snapshot,
                    Bounds::new(Point::new(x, 100), Size::new(width, 300)),
                )
                .map(|display| display.name.as_str())
            };
            // (case, window x, window width, display)
            let table = [
                ("inside the left", 100, 800, Some("left")),
                ("mostly right", 1800, 800, Some("right")),
                ("mostly left", 1500, 800, Some("left")),
                ("evenly across: the first", 1520, 800, Some("left")),
                ("off every display", 5000, 800, None),
                ("no width", 100, 0, None),
                ("at the edge of the range", i32::MAX - 10, i32::MAX, None),
            ];
            for (case, x, width, expected) in table {
                assert_eq!(on(x, width), expected, "{case}");
            }
        }
    }
}

pub(crate) mod hints {
    //! What an X11 window asks of the window manager in its properties: the
    //! content sizes it is held between, in `WM_NORMAL_HINTS` (ICCCM), and
    //! whether it is decorated, in `_MOTIF_WM_HINTS`, the property window
    //! managers read for it.

    #![forbid(unsafe_code)]

    use flui_foundation::geometry::Size;
    use flui_platform::SizeConstraints;

    use super::placement::device_extent;

    /// The content sizes a window is held between, in device pixels.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct SizeLimits {
        pub min: Option<(i32, i32)>,
        pub max: Option<(i32, i32)>,
    }

    /// The limits `constraints`, in logical pixels, set at `scale`. A window
    /// the user cannot resize is held at `content`, its size now.
    pub(crate) fn size_limits(
        constraints: SizeConstraints,
        resizable: bool,
        content: Size<i32>,
        scale: f64,
    ) -> SizeLimits {
        if !resizable {
            let held = Some((content.width, content.height));
            return SizeLimits {
                min: held,
                max: held,
            };
        }
        let device = |size: Size<f64>| {
            Some((
                i32::from(device_extent(size.width, scale)?),
                i32::from(device_extent(size.height, scale)?),
            ))
        };
        SizeLimits {
            min: constraints.min.and_then(device),
            max: constraints.max.and_then(device),
        }
    }

    /// `_MOTIF_WM_HINTS` saying the window is `decorated` or not: the
    /// decorations field alone is set (`MWM_HINTS_DECORATIONS`), all of them
    /// or none.
    pub(crate) fn motif_hints(decorated: bool) -> [u32; 5] {
        const DECORATIONS: u32 = 1 << 1;
        [DECORATIONS, 0, u32::from(decorated), 0, 0]
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_window_asks_the_limits_it_was_given() {
            let constraints = SizeConstraints::new(
                Some(Size::new(200.0, 150.0)),
                Some(Size::new(2000.0, 1500.0)),
            );
            let content = Size::new(640, 480);
            // (case, constraints, resizable, scale, limits)
            let table = [
                (
                    "both limits",
                    constraints,
                    true,
                    1.0,
                    SizeLimits {
                        min: Some((200, 150)),
                        max: Some((2000, 1500)),
                    },
                ),
                (
                    "scaled",
                    constraints,
                    true,
                    2.0,
                    SizeLimits {
                        min: Some((400, 300)),
                        max: Some((4000, 3000)),
                    },
                ),
                (
                    "none",
                    SizeConstraints::NONE,
                    true,
                    1.0,
                    SizeLimits::default(),
                ),
                (
                    "not resizable is held at its size",
                    constraints,
                    false,
                    1.0,
                    SizeLimits {
                        min: Some((640, 480)),
                        max: Some((640, 480)),
                    },
                ),
                (
                    "a limit past the extent is held at it",
                    SizeConstraints::new(None, Some(Size::new(1.0e9, 10.0))),
                    true,
                    1.0,
                    SizeLimits {
                        min: None,
                        max: Some((32_767, 10)),
                    },
                ),
            ];
            for (case, constraints, resizable, scale, expected) in table {
                assert_eq!(
                    size_limits(constraints, resizable, content, scale),
                    expected,
                    "{case}"
                );
            }
        }

        #[test]
        fn decorations_are_all_or_none() {
            assert_eq!(motif_hints(true), [2, 0, 1, 0, 0]);
            assert_eq!(motif_hints(false), [2, 0, 0, 0, 0]);
        }
    }
}
