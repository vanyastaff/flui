//! The present mode that follows from who paces a surface (ADR-0356).
//!
//! A surface the display paces presents in `Fifo`: its present blocks at the
//! display's cadence, and that block is the pacer (ADR-0353). A Wayland
//! surface is paced by its compositor instead: the host arms a frame callback
//! with every present (`SurfaceTarget::pre_present_notify`) and asks for the
//! next frame only once the compositor answers it, or once its grace runs out.
//! A compositor answers no callback for a surface it does not draw
//! (minimized, on a hidden workspace, covered), and a `Fifo` swapchain on
//! Wayland waits for exactly that answer: Mesa's, below `wp_fifo_v1`, waits in
//! `vkQueuePresentKHR` until the previous present's callback comes, with no
//! bound, so the present after the grace would block the owner thread until
//! the window is shown again. A compositor-paced surface therefore presents
//! without that wait, in `Mailbox`, else `Immediate`; the host's callback
//! still holds it to one frame per compositor refresh, so nothing spins.
//!
//! Who paces a surface is its target's word
//! ([`SurfaceTarget::frame_pacing`](flui_platform::SurfaceTarget::frame_pacing)),
//! read once when the surface is made and kept by its lease for every
//! configuration after (`SurfaceLease::pacing`).

use flui_platform::FramePacing;

/// The present mode a surface paced by `pacing` and offering `offered` is
/// configured with: `Fifo` for one the display paces; for one its compositor
/// paces, a mode whose present never waits on the compositor, `Mailbox`
/// before `Immediate` (which can tear where the compositor does not take over
/// the buffer), and `Fifo` where the surface offers neither. wgpu offers
/// `Fifo` on every surface. A pacing this engine does not know yet presents
/// as the display's.
pub(crate) fn present_mode(
    pacing: FramePacing,
    offered: &[wgpu::PresentMode],
) -> wgpu::PresentMode {
    match pacing {
        FramePacing::Compositor => [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
            .into_iter()
            .find(|mode| offered.contains(mode))
            .unwrap_or(wgpu::PresentMode::Fifo),
        _ => wgpu::PresentMode::Fifo,
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::FramePacing::{Compositor, Display};
    use wgpu::PresentMode::{AutoNoVsync, AutoVsync, Fifo, FifoRelaxed, Immediate, Mailbox};

    use super::present_mode;

    /// A surface the display paces presents in `Fifo` whatever it offers; one
    /// its compositor paces presents in a mode that never waits on the
    /// compositor where it offers one, `Mailbox` first, and in `Fifo` where it
    /// offers neither.
    #[test]
    fn the_pacer_chooses_the_present_mode() {
        for (case, pacing, offered, expected) in [
            (
                "display, all offered",
                Display,
                &[Fifo, Mailbox, Immediate][..],
                Fifo,
            ),
            ("display, Fifo alone", Display, &[Fifo][..], Fifo),
            (
                "compositor, Mesa",
                Compositor,
                &[Fifo, Mailbox, Immediate][..],
                Mailbox,
            ),
            (
                "compositor, no Mailbox",
                Compositor,
                &[Fifo, Immediate][..],
                Immediate,
            ),
            (
                "compositor, GL",
                Compositor,
                &[Fifo, AutoVsync, AutoNoVsync][..],
                Fifo,
            ),
            (
                "compositor, relaxed",
                Compositor,
                &[FifoRelaxed, Fifo][..],
                Fifo,
            ),
        ] {
            assert_eq!(present_mode(pacing, offered), expected, "{case}");
        }
    }
}
