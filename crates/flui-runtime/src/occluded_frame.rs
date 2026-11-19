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
//! The owed frame is the window's own: the presentation asks its window for a
//! frame at the instant ([`OwedFrameRequest`],
//! [`PlatformWindow::request_frame_at`]), which the host serves for that window
//! alone, and no application deadline carries it (ADR-0389). A window is asked
//! once per instant, whatever the frames of other windows do; the request is
//! withdrawn once nothing is owed, and replaced when the instant moves.
//!
//! [`OccludedSurface::Kept`]: flui_platform::OccludedSurface::Kept
//! [`PlatformWindow::request_frame_at`]: flui_platform::PlatformWindow::request_frame_at

use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_platform::{EventTime, PlatformWindow};
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

/// What a presentation asks of its window for the frame its surface is owed:
/// a frame at an instant on the event timeline, or the withdrawal of the
/// instant asked before (ADR-0389). The UI runtime makes one only where that
/// differs from what the window was last asked
/// ([`UiRuntime::owed_frame_requests`](crate::ui_runtime::UiRuntime::owed_frame_requests)),
/// so the window hears each instant once.
#[must_use = "a request reaches its window only when it is delivered"]
pub struct OwedFrameRequest {
    window: Weak<dyn PlatformWindow>,
    at: Option<EventTime>,
    /// The instant the window was last asked for, shared with its
    /// presentation: `None` while it was asked for none, or that was withdrawn.
    asked: Rc<Cell<Option<EventTime>>>,
}

impl std::fmt::Debug for OwedFrameRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwedFrameRequest")
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

impl OwedFrameRequest {
    /// The request a presentation whose window was last asked for `asked`
    /// makes of `window` when it owes a frame at `at` (`None`: nothing), or
    /// `None` when that is what the window was last asked: a window asked for
    /// nothing has nothing to withdraw.
    pub(crate) fn of(
        window: &Weak<dyn PlatformWindow>,
        at: Option<EventTime>,
        asked: &Rc<Cell<Option<EventTime>>>,
    ) -> Option<Self> {
        (asked.get() != at).then(|| Self {
            window: Weak::clone(window),
            at,
            asked: Rc::clone(asked),
        })
    }

    /// Ask the window: its host's
    /// [`request_frame_at`](PlatformWindow::request_frame_at) runs here, so a
    /// host delivers it with no borrow of a UI runtime held. The presentation
    /// records it as asked once that returns: a host that panics is asked
    /// again for the same instant by the next request made. A window already
    /// gone has nothing left to ask.
    pub fn deliver(self) {
        if let Some(window) = self.window.upgrade() {
            window.request_frame_at(self.at);
        }
        self.asked.set(self.at);
    }
}

/// `at` on the event timeline counted from `epoch`, the clock's
/// ([`MonotonicClock::epoch`](flui_foundation::MonotonicClock::epoch)). An
/// instant before the epoch is the epoch, and one past the timeline's end is
/// its end.
pub(crate) fn event_time(epoch: Instant, at: Instant) -> EventTime {
    EventTime::from_nanos(
        u64::try_from(at.saturating_duration_since(epoch).as_nanos()).unwrap_or(u64::MAX),
    )
}
