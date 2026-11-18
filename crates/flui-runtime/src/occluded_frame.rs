//! The frame a covered window owes its surface (ADR-0357).
//!
//! A window the window system keeps off the screen draws no frame: its
//! presentation's clock is hidden, and what changes in its tree waits. Where
//! the window system keeps a covered window's last frame and shows it again the
//! moment the window is uncovered ([`OccludedSurface::Kept`], AppKit), a window
//! that drew nothing while covered shows the content of before the covering
//! for that moment. Such a window owes its surface a frame: once it has drawn
//! nothing for [`OCCLUDED_FRAME_QUIET`] with work waiting, it draws one, so the
//! surface is never older than that when it is uncovered.
//!
//! Nothing else owes: a window system that keeps no image of an obscured window
//! (X11 without a compositing manager) clips the frame away, one that holds a
//! hidden window's frames (a suspended Wayland toplevel) never runs it, and an
//! occlusion that is no covering (a lock, a background app or page, a sleeping
//! machine) shows nothing the frame could reach. A frame owed is never asked
//! for again past its time: if the host does not run it within
//! [`OCCLUDED_FRAME_GRACE`], the host holds the window's frames, and the window
//! owes nothing more until its sight changes. A frame the surface refuses ends
//! the obligation until the window is seen again.
//!
//! [`OccludedSurface::Kept`]: flui_platform::OccludedSurface::Kept

use std::time::Duration;

use web_time::Instant;

/// How long a covered window with work waiting draws nothing before it draws
/// the frame it owes its surface: the cost of a covered window that keeps
/// changing (one frame per period) against how old its surface may be when
/// it is uncovered.
pub(crate) const OCCLUDED_FRAME_QUIET: Duration = Duration::from_secs(1);

/// How long past its time an owed frame stays owed. A host that runs it does
/// so on the wake its deadline brings; one that has not run it by then holds
/// the window's frames, and asking again would only spin the loop.
pub(crate) const OCCLUDED_FRAME_GRACE: Duration = Duration::from_millis(100);

/// Whether a presentation owes its surface a frame while it cannot be seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OccludedFrame {
    /// Seen, or unseen where no frame drawn now would be shown.
    #[default]
    NotOwed,
    /// Covered where the surface is kept, and drawing nothing since this
    /// instant.
    QuietSince(Instant),
    /// The surface refused the frame owed: nothing more is owed until the
    /// window is seen again.
    Refused,
}

impl OccludedFrame {
    /// The window, unseen, is now in `sight`: what it owes from `now` on. A
    /// quiet period that still runs is kept, and a refusal stands; a window
    /// that may no longer be covered owes nothing, and one covered again after
    /// its owed frame lapsed starts a new quiet period.
    pub(crate) fn follow(self, sight: Sight, retry_at: Option<Instant>, now: Instant) -> Self {
        match self {
            Self::Refused => Self::Refused,
            _ if !sight.owes() => Self::NotOwed,
            Self::QuietSince(_) if !self.lapsed(retry_at, now) => self,
            Self::NotOwed | Self::QuietSince(_) => Self::QuietSince(now),
        }
    }

    /// When the owed frame comes due, if one is owed: the end of the quiet
    /// period, and never before the dropped-frame retry `retry_at`, which no
    /// frame runs before. An instant past the clock's range reads as never.
    pub(crate) fn due_at(self, retry_at: Option<Instant>) -> Option<Instant> {
        let Self::QuietSince(since) = self else {
            return None;
        };
        let quiet = since.checked_add(OCCLUDED_FRAME_QUIET)?;
        Some(retry_at.map_or(quiet, |retry| quiet.max(retry)))
    }

    /// The owed frame at `now`: its due time while it is owed and not lapsed,
    /// else `None`.
    pub(crate) fn owed_at(self, retry_at: Option<Instant>, now: Instant) -> Option<Instant> {
        self.due_at(retry_at).filter(|due| {
            due.checked_add(OCCLUDED_FRAME_GRACE)
                .is_none_or(|end| now < end)
        })
    }

    /// Whether the owed frame's time and its grace have passed at `now`
    /// without it running.
    fn lapsed(self, retry_at: Option<Instant>, now: Instant) -> bool {
        self.due_at(retry_at).is_some() && self.owed_at(retry_at, now).is_none()
    }
}

/// What an unseen window's frame would reach, from its state, its surface and
/// its session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sight {
    covered: bool,
    surface: flui_platform::OccludedSurface,
    session_shown: bool,
}

impl Sight {
    /// The sight of a window in `state` whose host names its occluded surface
    /// `surface`, in a session that shows windows (`session_shown`: unlocked
    /// and awake).
    pub(crate) fn of(
        state: &flui_platform::WindowState,
        surface: flui_platform::OccludedSurface,
        session_shown: bool,
    ) -> Self {
        Self {
            covered: may_be_covered(state),
            surface,
            session_shown,
        }
    }

    /// Whether a frame drawn now would be shown when the window is uncovered:
    /// it may be covered, its surface is kept, and its session shows windows.
    pub(crate) fn owes(self) -> bool {
        self.covered && self.surface == flui_platform::OccludedSurface::Kept && self.session_shown
    }
}

/// Whether the window in `state` is occluded by what may be a covering: shown,
/// not minimized, running, occluded, on a host that tells a covering
/// ([`reports_occlusion`](flui_platform::WindowCapabilities::reports_occlusion)).
/// A host that does not tell one reads occluded only for what is never a
/// covering.
fn may_be_covered(state: &flui_platform::WindowState) -> bool {
    state.occluded
        && state.visible
        && state.mode != flui_platform::WindowMode::Minimized
        && state.execution == flui_platform::WindowExecutionState::Running
        && state.capabilities.reports_occlusion().is_available()
}
