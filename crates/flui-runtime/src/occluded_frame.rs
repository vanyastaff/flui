//! The frame an occluded window owes its surface (ADR-0357).
//!
//! A window the window system keeps off the screen draws no frame: its
//! presentation's clock is hidden, and what changes in its tree waits. Where
//! the occlusion may be a covering by other windows, the window system shows
//! the window's surface again the moment it is uncovered, before any frame
//! drawn then reaches it, so a window that drew nothing while covered shows
//! the content of before the covering for that moment. Such a window draws one
//! owed frame once it has drawn nothing for [`OCCLUDED_FRAME_QUIET`] with work
//! waiting: at most one frame per quiet period, and the surface is never older
//! than that when the window is uncovered. An occlusion that is never a
//! covering (a locked session, a background app or page, a window the host
//! does not tell a covering for) owes nothing: drawing there is wasted, or
//! not allowed.

use std::time::Duration;

use web_time::Instant;

/// How long a covered window with work waiting draws nothing before it draws
/// the frame it owes its surface. The cost of a covered window that keeps
/// changing (one frame per period) against how old its surface may be when
/// it is uncovered.
pub(crate) const OCCLUDED_FRAME_QUIET: Duration = Duration::from_secs(1);

/// Whether a presentation owes its surface frames while it cannot be seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OccludedFrame {
    /// Seen, or unseen for a reason that is never a covering.
    #[default]
    NotOwed,
    /// Occluded where the occlusion may be a covering, and drawing nothing
    /// since this instant.
    QuietSince(Instant),
}

impl OccludedFrame {
    /// What a window in `state` that stopped being seen at `now` owes.
    pub(crate) fn for_window(state: &flui_platform::WindowState, now: Instant) -> Self {
        if may_be_covered(state) {
            Self::QuietSince(now)
        } else {
            Self::NotOwed
        }
    }

    /// When the owed frame comes due, if one is owed: the end of the quiet
    /// period. An instant past the clock's range reads as never.
    pub(crate) fn due_at(self) -> Option<Instant> {
        match self {
            Self::NotOwed => None,
            Self::QuietSince(since) => since.checked_add(OCCLUDED_FRAME_QUIET),
        }
    }
}

/// Whether the window in `state` is occluded by what may be a covering: shown,
/// not minimized, running, occluded, on a host that tells a covering
/// ([`reports_occlusion`](flui_platform::WindowCapabilities::reports_occlusion)).
/// A host that does not tell one reads occluded only for what is never a
/// covering.
pub(crate) fn may_be_covered(state: &flui_platform::WindowState) -> bool {
    state.occluded
        && state.visible
        && state.mode != flui_platform::WindowMode::Minimized
        && state.execution == flui_platform::WindowExecutionState::Running
        && state.capabilities.reports_occlusion().is_available()
}
