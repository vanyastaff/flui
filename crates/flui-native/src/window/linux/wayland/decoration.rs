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

    /// Whether the window draws the margins of its own frame around a
    /// title bar of the application's (`app_title_bar`, which asks the
    /// compositor for no frame): where it can draw them and the
    /// compositor draws none.
    #[cfg_attr(
        not(wayland_host),
        allow(dead_code, reason = "read by the Wayland host's window alone")
    )]
    pub(crate) const fn app_margins(self, app_title_bar: bool) -> bool {
        app_title_bar && self.draws_own && matches!(self.drawn(), Drawn::Nobody)
    }

    /// Why the window cannot take a title bar of the application's, or
    /// `None` where it can: it draws the margins of its own frame around
    /// it, with the subsurfaces and shared memory a frame is drawn with,
    /// where the compositor leaves the frame to it.
    #[cfg_attr(
        not(wayland_host),
        allow(dead_code, reason = "read by the Wayland host's window alone")
    )]
    pub(crate) const fn app_title_bar_absence(self) -> Option<CapabilityAbsence> {
        if self.draws_own {
            self.absence()
        } else {
            Some(CapabilityAbsence::NotOnThisDevice)
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
