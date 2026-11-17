//! When a Wayland window delivers the frame it was asked for: once the
//! compositor has shown the last one (its frame callback's `done`), or
//! once that callback is given up.
//!
//! The raster thread arms the callback just before it presents, and the
//! present's commit carries it to the compositor. A present that is
//! skipped (an outdated surface, a zero size) commits nothing, so its
//! callback never comes: after [`CALLBACK_GRACE`] the window stops waiting
//! for it. A window the compositor does not draw sends no `done` either;
//! it is then asked for a frame at most once per grace.
//!
//! A window the compositor says it does not show (`suspended`: covered by
//! other windows, on a hidden workspace, its outputs off) is occluded: it
//! is given no frame, and no grace runs out for it. The frame asked for
//! is kept, and delivered once the compositor shows the window again.

#![forbid(unsafe_code)]

use std::time::Duration;

use web_time::Instant;

/// How long a window waits for a frame callback before it gives it up.
pub(crate) const CALLBACK_GRACE: Duration = Duration::from_secs(1);

/// Whether a callback armed at `armed` still holds the next frame back.
pub(crate) fn waiting_on_callback(armed: Option<Instant>, now: Instant) -> bool {
    armed.is_some_and(|armed| now < armed + CALLBACK_GRACE)
}

/// What decides whether a window delivers a frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pacing {
    /// A frame was asked for.
    pub(crate) asked: bool,
    /// The compositor configured the window.
    pub(crate) configured: bool,
    /// The compositor does not show the window.
    pub(crate) suspended: bool,
    /// When the frame callback that is out was armed.
    pub(crate) armed: Option<Instant>,
}

impl Pacing {
    /// Whether the window delivers its frame now.
    pub(crate) fn due(self, now: Instant) -> bool {
        self.asked && self.configured && !self.suspended && !waiting_on_callback(self.armed, now)
    }

    /// When the window gives up the callback that holds its frame back;
    /// `None` with nothing armed, nothing asked for, or while the window
    /// is not shown, whose frame waits for the compositor instead.
    pub(crate) fn gives_up_at(self) -> Option<Instant> {
        self.armed
            .filter(|_| self.asked && !self.suspended)
            .map(|armed| armed + CALLBACK_GRACE)
    }
}

/// The first output in `entered` (the order a surface entered them) the
/// compositor still announces: one removed while the surface was on it
/// sends the surface no `leave`.
pub(crate) fn first_live<T: Copy>(entered: &[T], live: impl Fn(T) -> bool) -> Option<T> {
    entered.iter().copied().find(|output| live(*output))
}

/// What the compositor said about whether it draws a window.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sight {
    /// The window acknowledged a configure.
    pub(crate) configured: bool,
    /// The compositor answered a frame callback of the window's: it
    /// mapped the window at least once.
    pub(crate) shown: bool,
    /// The surface is on an output now.
    pub(crate) on_output: bool,
    /// The compositor suspended the window.
    pub(crate) suspended: bool,
}

impl Sight {
    /// Whether the window is occluded: not configured yet, suspended, or
    /// shown once and on no output now. A configured window the
    /// compositor has not shown yet is on no output either, since a
    /// surface enters one only once it is mapped with its first frame; it
    /// is not occluded, or it would never be given that frame.
    pub(crate) fn occluded(self) -> bool {
        !self.configured || self.suspended || (self.shown && !self.on_output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A configured window is occluded once the compositor suspends it,
    /// or once it showed the window and the window is on no output; a
    /// window it has not shown yet waits for its first frame unoccluded,
    /// on no output.
    #[test]
    fn a_window_is_occluded_once_shown_and_off_every_output() {
        for (case, configured, shown, on_output, suspended, expected) in [
            ("not configured", false, false, false, false, true),
            (
                "configured, before its first frame",
                true,
                false,
                false,
                false,
                false,
            ),
            ("shown, on an output", true, true, true, false, false),
            ("shown, on no output", true, true, false, false, true),
            (
                "suspended before its first frame",
                true,
                false,
                false,
                true,
                true,
            ),
            ("suspended on an output", true, true, true, true, true),
        ] {
            let sight = Sight {
                configured,
                shown,
                on_output,
                suspended,
            };
            assert_eq!(sight.occluded(), expected, "{case}");
        }
    }

    /// A frame waits on the callback its present armed until the
    /// compositor answers or the grace runs out; a present that committed
    /// nothing never holds the window for longer.
    #[test]
    fn a_lost_frame_callback_holds_the_window_for_its_grace_only() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        assert!(!waiting_on_callback(None, now), "nothing armed");
        assert!(waiting_on_callback(Some(now), now), "just armed");
        assert!(waiting_on_callback(now.checked_sub(ms(900)), now));
        assert!(
            !waiting_on_callback(now.checked_sub(CALLBACK_GRACE), now),
            "given up at the grace"
        );
        let pacing = |asked, armed| Pacing {
            asked,
            configured: true,
            suspended: false,
            armed,
        };
        assert_eq!(
            pacing(true, Some(now)).gives_up_at(),
            Some(now + CALLBACK_GRACE)
        );
        assert_eq!(
            pacing(false, Some(now)).gives_up_at(),
            None,
            "no frame asked for"
        );
        assert_eq!(pacing(true, None).gives_up_at(), None);
        assert!(pacing(true, None).due(now));
        assert!(!pacing(true, Some(now)).due(now), "the callback holds it");
        assert!(pacing(true, now.checked_sub(CALLBACK_GRACE)).due(now));
        assert!(!pacing(false, None).due(now), "nothing asked for");
        let unconfigured = Pacing {
            configured: false,
            ..pacing(true, None)
        };
        assert!(!unconfigured.due(now));
    }

    /// An occluded window (the compositor suspended it) is given no frame
    /// and gives up no callback, however long it waits; the frame asked
    /// for is delivered once the compositor shows the window again.
    #[test]
    fn an_occluded_window_holds_its_frame_until_it_is_shown() {
        let now = Instant::now();
        let long_ago = now.checked_sub(CALLBACK_GRACE * 5);
        let occluded = Pacing {
            asked: true,
            configured: true,
            suspended: true,
            armed: long_ago,
        };
        assert!(!occluded.due(now), "past the grace, still occluded");
        assert_eq!(occluded.gives_up_at(), None, "no wake for a grace");
        let unarmed = Pacing {
            armed: None,
            ..occluded
        };
        assert!(!unarmed.due(now), "nothing armed, still occluded");
        let shown = Pacing {
            suspended: false,
            ..occluded
        };
        assert!(shown.due(now), "shown again: the kept frame goes out");
    }

    /// A surface is on the first output it entered that still exists.
    #[test]
    fn a_removed_output_names_no_display() {
        let live = |output: u32| output != 7;
        assert_eq!(first_live(&[7, 9], live), Some(9));
        assert_eq!(first_live(&[7], live), None);
        assert_eq!(first_live::<u32>(&[], live), None);
    }
}
