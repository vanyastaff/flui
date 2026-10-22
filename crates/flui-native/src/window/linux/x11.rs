//! The X11 window as the live layer reads it: what the X server and the window
//! manager say about a window, which winit does not report — whether it is
//! minimized, and when that or its mode changes.
//!
//! Two halves: [`mode`], the pure one, decides a window's mode from the two
//! properties a window manager keeps for it and is checked on every host;
//! `probe` reads those properties from the server and watches them change,
//! answering the window model's `WindowManagerView`, which the winit host
//! connects to the winit window.

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
mod probe;

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
pub(crate) use probe::connect_window_manager;

pub(crate) mod mode {
    //! The mode of an X11 window, from the two properties a window manager keeps
    //! for it.
    //!
    //! winit reports a window as maximized or fullscreen but not as minimized, and
    //! its answers for the other two lag the window manager by an event. The
    //! adapter reads the properties directly and this model decides what they
    //! say.
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

    use flui_platform::WindowMode;

    use crate::window::state::mode_of;

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

    /// The window's mode. `wm_state` is `None` when the window manager sets none.
    ///
    /// Minimized wins over the rest, as it hides the window whatever else is set;
    /// fullscreen wins over maximized, which a fullscreen window often keeps. A
    /// window maximized on one axis only is tiled or snapped, not maximized.
    pub(crate) fn window_mode(wm_state: Option<WmState>, net: NetWmState) -> WindowMode {
        let minimized = match wm_state {
            Some(state) => state == WmState::Iconic,
            None => net.hidden,
        };
        mode_of(
            minimized,
            net.fullscreen,
            net.maximized_horz && net.maximized_vert,
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

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
                assert_eq!(window_mode(*wm_state, net), *expected, "{case}");
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
    }
}
