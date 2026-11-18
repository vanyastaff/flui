//! An X11 window waiting for its window manager to carry out a mode it asked
//! for. The window manager answers each `_NET_WM_STATE` request with a
//! property change of its own and may pass through other modes on the way, so
//! the window's state cell keeps the mode the window left until the server's
//! properties say the mode asked (`WindowStateCell::expect_mode`); the loop
//! wakes when a wait ends and the window reads itself again.

use std::time::Instant;

use flui_platform::WindowMode;

use super::{X11Window, X11Windows};
use crate::window::state::MODE_ANSWER_LIMIT;

impl X11Window {
    /// The window asked its window manager for `mode`: the snapshot keeps
    /// the mode the window left until the server's properties say `mode`, or
    /// for at most [`MODE_ANSWER_LIMIT`]. Every mode request calls it, one
    /// for the mode the window is in too, so a newer request replaces the
    /// wait of an older one whose answers may still be coming.
    pub(super) fn expect_mode(&self, mode: WindowMode) {
        self.cell
            .expect_mode(mode, Instant::now() + MODE_ANSWER_LIMIT);
    }

    /// Whether a read at `now` of a window in `mode` finds it where it
    /// settles: in no mode change, or in the mode its change asked for. A
    /// step on the way to another mode is no place the window returns to.
    pub(super) fn settles_in(&self, mode: WindowMode, now: Instant) -> bool {
        self.cell
            .mode_in_flight(now)
            .is_none_or(|asked| asked == mode)
    }
}

impl X11Windows {
    /// Each window whose wait for a mode change ended by `now` reads itself
    /// again at the turn's read, though no event marked it.
    pub(super) fn mark_waits_over(&self, now: Instant) {
        for window in self.table.borrow().windows.values() {
            if window
                .cell
                .mode_deadline()
                .is_some_and(|until| until <= now)
            {
                window.mark_stale();
            }
        }
    }

    /// The earliest instant a window stops waiting for a mode change.
    pub(super) fn earliest_mode_deadline(&self) -> Option<Instant> {
        let table = self.table.borrow();
        let waits = table
            .windows
            .values()
            .filter_map(|window| window.cell.mode_deadline());
        waits.min()
    }
}
