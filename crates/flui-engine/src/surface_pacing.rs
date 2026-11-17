//! Who decides when a surface's frames are shown, and the present mode that
//! follows from it (ADR-0356).
//!
//! A surface the display paces presents in `Fifo`: its present blocks at the
//! display's cadence, and that block is the pacer (ADR-0353). A Wayland
//! surface is paced by its compositor instead: the host arms a frame callback
//! with every present (`SurfaceTarget::pre_present_notify`) and asks for the
//! next frame only once the compositor answers it, or once its grace runs out
//! (`flui-native`'s `window::linux::wayland::frame`). A compositor answers no
//! callback for a surface it does not draw (minimized, on a hidden workspace,
//! covered), and a `Fifo` swapchain on Wayland waits for exactly that answer:
//! Mesa's, below `wp_fifo_v1`, waits in `vkQueuePresentKHR` until the
//! previous present's callback comes, with no bound. The present after the
//! grace would block the owner thread until the window is shown again. So a
//! compositor-paced surface presents without that wait, in `Mailbox`, else
//! `Immediate`; the host's callback still holds it to one frame per
//! compositor refresh, so nothing spins.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Who decides when a surface's frames are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfacePacing {
    /// The display: the swapchain's `Fifo` present blocks at its cadence.
    Display,
    /// The compositor, through the frame callback the host arms with each
    /// present: a Wayland surface.
    Compositor,
}

impl SurfacePacing {
    /// The pacing of the surface `target` hands out: a Wayland surface is its
    /// compositor's, any other the display's, and so is a target whose window
    /// is gone (it makes no surface to configure).
    pub(crate) fn of(target: &(impl HasWindowHandle + ?Sized)) -> Self {
        match target.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Wayland(_)) => Self::Compositor,
            _ => Self::Display,
        }
    }

    /// The present mode a surface offering `offered` is configured with:
    /// `Fifo` for one the display paces; for one its compositor paces, a mode
    /// whose present never waits on the compositor, `Mailbox` before
    /// `Immediate` (which can tear where the compositor does not take over
    /// the buffer), and `Fifo` where the surface offers neither. wgpu offers
    /// `Fifo` on every surface.
    pub(crate) fn present_mode(self, offered: &[wgpu::PresentMode]) -> wgpu::PresentMode {
        match self {
            Self::Display => wgpu::PresentMode::Fifo,
            Self::Compositor => [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
                .into_iter()
                .find(|mode| offered.contains(mode))
                .unwrap_or(wgpu::PresentMode::Fifo),
        }
    }
}

#[cfg(test)]
mod tests {
    use wgpu::PresentMode::{AutoNoVsync, AutoVsync, Fifo, FifoRelaxed, Immediate, Mailbox};

    use super::SurfacePacing::{Compositor, Display};

    /// A surface the display paces presents in `Fifo` whatever it offers; one
    /// its compositor paces presents in a mode that never waits on the
    /// compositor where it offers one, `Mailbox` first, and in `Fifo` where it
    /// offers neither.
    #[test]
    fn the_pacer_chooses_the_present_mode() {
        for (case, pacing, offered, expected) in [
            ("display, all offered", Display, &[Fifo, Mailbox, Immediate][..], Fifo),
            ("display, Fifo alone", Display, &[Fifo][..], Fifo),
            ("compositor, Mesa", Compositor, &[Fifo, Mailbox, Immediate][..], Mailbox),
            ("compositor, no Mailbox", Compositor, &[Fifo, Immediate][..], Immediate),
            ("compositor, GL", Compositor, &[Fifo, AutoVsync, AutoNoVsync][..], Fifo),
            ("compositor, relaxed", Compositor, &[FifoRelaxed, Fifo][..], Fifo),
        ] {
            assert_eq!(pacing.present_mode(offered), expected, "{case}");
        }
    }
}
