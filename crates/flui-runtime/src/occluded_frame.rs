//! The frame a covered window owes its surface (ADR-0357).
//!
//! A window the window system keeps off the screen draws no frame: its
//! presentation's clock is hidden, and what changes in its tree waits. Where
//! the window system keeps a covered window's last frame and shows it again the
//! moment the window is uncovered ([`OccludedSurface::Kept`], AppKit), a window
//! that drew nothing while covered shows the content of before the covering
//! for that moment. Such a window owes its surface a frame: once it has drawn
//! nothing for [`OCCLUDED_FRAME_QUIET`] with work waiting, it draws one, so the
//! surface is never older than that when it is uncovered. The frame stays due
//! until it runs, however late the host serves its wake or however long a hold
//! (the dropped-frame backoff, a full sink, text preparation) keeps it; only a
//! frame that ran starts the next quiet period.
//!
//! Nothing else owes: a window system that keeps no image of an obscured window
//! (X11 without a compositing manager) clips the frame away, one that holds a
//! hidden window's frames (a suspended Wayland toplevel) never runs it, and an
//! occlusion that is no covering (a lock, a background app or page, a sleeping
//! machine) shows nothing the frame could reach. The frame the surface refuses
//! or drops ends the obligation until the window is seen again.
//!
//! [`OccludedSurface::Kept`]: flui_platform::OccludedSurface::Kept

use std::time::Duration;

use web_time::Instant;

/// How long a covered window with work waiting draws nothing before it draws
/// the frame it owes its surface: the cost of a covered window that keeps
/// changing (one frame per period) against how old its surface may be when
/// it is uncovered.
pub(crate) const OCCLUDED_FRAME_QUIET: Duration = Duration::from_secs(1);

/// Whether a presentation owes its surface a frame while it cannot be seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OccludedFrame {
    /// Seen, or unseen where no frame drawn now would be shown.
    #[default]
    NotOwed,
    /// Covered where the surface is kept, and drawing nothing since this
    /// instant: since it stopped owing nothing, or since its last owed frame
    /// ran.
    QuietSince(Instant),
    /// The surface refused or dropped the frame owed: nothing more is owed
    /// until the window is seen again.
    Refused,
}

impl OccludedFrame {
    /// The window, unseen, is now in `sight`: what it owes from `now` on. A
    /// running quiet period is kept, whatever the snapshot that brought the
    /// new sight, and a refusal stands; a window whose frame would no longer be
    /// shown owes nothing, and one that owed nothing starts a quiet period.
    pub(crate) fn follow(self, sight: Sight, now: Instant) -> Self {
        match self {
            Self::Refused => Self::Refused,
            _ if !sight.owes() => Self::NotOwed,
            Self::QuietSince(_) => self,
            Self::NotOwed => Self::QuietSince(now),
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
}

/// Whether the user's session shows windows, from the host's notices: a lock
/// and the machine's sleep are separate facts, since the lock can outlast a
/// sleep (locked, asleep, awake with the lock still up).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Session {
    locked: bool,
    asleep: bool,
}

impl Session {
    /// The session after `notice`; `None` for a notice that changes neither.
    pub(crate) fn after(self, notice: &flui_platform::HostNotice) -> Option<Self> {
        use flui_platform::HostNotice;
        match notice {
            HostNotice::SessionLocked => Some(Self {
                locked: true,
                ..self
            }),
            HostNotice::SessionUnlocked => Some(Self {
                locked: false,
                ..self
            }),
            HostNotice::Sleep => Some(Self {
                asleep: true,
                ..self
            }),
            HostNotice::Wake => Some(Self {
                asleep: false,
                ..self
            }),
            _ => None,
        }
    }

    /// Whether the session shows windows: unlocked and awake.
    pub(crate) fn shows_windows(self) -> bool {
        !self.locked && !self.asleep
    }
}

/// What an unseen window's frame would reach, from its state, its surface and
/// its session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sight {
    covered: bool,
    surface: flui_platform::OccludedSurface,
    session: Session,
}

impl Sight {
    /// The sight of a window in `state` whose host names its occluded surface
    /// `surface`, in `session`.
    pub(crate) fn of(
        state: &flui_platform::WindowState,
        surface: flui_platform::OccludedSurface,
        session: Session,
    ) -> Self {
        Self {
            covered: may_be_covered(state),
            surface,
            session,
        }
    }

    /// Whether a frame drawn now would be shown when the window is uncovered:
    /// it may be covered, its surface is kept, and its session shows windows.
    pub(crate) fn owes(self) -> bool {
        self.covered
            && self.surface == flui_platform::OccludedSurface::Kept
            && self.session.shows_windows()
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
