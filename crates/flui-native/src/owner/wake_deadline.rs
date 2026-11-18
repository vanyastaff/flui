//! The owner's two deadlines as a loop arms and delivers them.
//!
//! - The turn deadline (`AppHandler::turn_deadline`, ADR-0343 §1) is
//!   delivered as one owner turn with no frame, through [`TurnDeadline`],
//!   which every host uses, headless included. Its one rule: a turn delivered
//!   for the deadline serves every deadline not later than that turn's own
//!   time, and an answer not later than the last time served is not armed
//!   again. A handler that keeps answering an instant already past gets one
//!   turn for it, never a loop that spins: work that is ready now is no
//!   deadline, and the application does it in the turn it is in or asks for
//!   an owner turn through the signal.
//! - The wake deadline (`AppHandler::wake_deadline`) is delivered as frame
//!   requests, through [`WakeDeadline`], on every native host: the Win32,
//!   X11, Wayland and Android loops wait with a timeout of their own, UIKit
//!   with a main-queue timer and AppKit with its wake pump, and each asks its
//!   windows for frames at it.

/// The turn deadline of one loop: the instant armed, and the time of the
/// last turn delivered for it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TurnDeadline {
    armed: Option<web_time::Instant>,
    /// The time of the last turn delivered for the deadline: that turn served
    /// every answer not later than it.
    served: Option<web_time::Instant>,
}

impl TurnDeadline {
    /// The application's handler answered `answer`: arm it unless a turn
    /// delivered for the deadline already served it, that is unless it is not
    /// later than that turn's time. The instant to wait until, if any.
    pub(crate) fn arm(&mut self, answer: Option<web_time::Instant>) -> Option<web_time::Instant> {
        self.armed = answer.filter(|at| self.served.is_none_or(|served| *at > served));
        self.armed
    }

    /// The instant armed, if any. A host that schedules a timer for it reads
    /// it before [`arm`](Self::arm) and keeps the timer it has when the
    /// instant did not change: AppKit and the web, which time the deadline
    /// with a timer per answer.
    #[cfg(any(target_os = "macos", target_arch = "wasm32", test))]
    pub(crate) fn armed(&self) -> Option<web_time::Instant> {
        self.armed
    }

    /// Whether the armed instant is due at `now`. When it is, the caller
    /// gives the owner one turn, which serves every deadline not later than
    /// `now`.
    pub(crate) fn take_due(&mut self, now: web_time::Instant) -> bool {
        if self.armed.take_if(|at| *at <= now).is_none() {
            return false;
        }
        self.served = Some(self.served.map_or(now, |served| served.max(now)));
        true
    }
}

/// The last wake deadline the loop delivered. The application's handler
/// answers with an instant only, so the same instant is either work no frame
/// has had a chance to service, which must not spin, or work a frame left due
/// at the instant just serviced, which must be delivered again. The count of
/// frames that ran is the delivery generation that tells them apart. A run
/// has one handler, so the instant alone names the answer.
#[cfg(any(
    windows,
    x11_host,
    wayland_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
struct DeadlineDelivery {
    arm: DeadlineArm,
    /// The count of frames that ran when the delivery was made.
    frames: u64,
}

#[cfg(any(
    windows,
    x11_host,
    wayland_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
impl DeadlineDelivery {
    /// Whether `deadline` may be armed after this delivery: a different
    /// instant always, the same one whenever a frame callback ran since. An
    /// answer no frame could have serviced stays delivered, so a delivery that
    /// reaches no frame never spins the loop.
    fn admits(&self, deadline: web_time::Instant, frames: u64) -> bool {
        self.arm.deadline != deadline || frames != self.frames
    }
}

/// How many frames this platform's windows' handlers have produced. The
/// loop compares it across a deadline delivery to tell an answer repeated
/// after a serviced frame from one nothing has serviced yet. Each window's
/// [`WindowDelivery`](crate::owner::window_delivery::WindowDelivery) counts
/// into it when its handler's `frame_requested` produces a frame, so a frame
/// request no handler ran a frame for (a declined one, or a window that let go
/// of its handler) leaves it unchanged.
#[cfg(any(
    windows,
    x11_host,
    wayland_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android"
))]
#[derive(Default)]
pub(crate) struct FrameCount(std::sync::Arc<std::sync::atomic::AtomicU64>);

#[cfg(any(
    windows,
    x11_host,
    wayland_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android"
))]
impl FrameCount {
    pub(crate) fn get(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The counter a window's callbacks increment.
    pub(crate) fn counter(&self) -> std::sync::Arc<std::sync::atomic::AtomicU64> {
        std::sync::Arc::clone(&self.0)
    }
}

/// A deadline the loop waits for.
#[cfg(any(
    windows,
    x11_host,
    wayland_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
struct DeadlineArm {
    deadline: web_time::Instant,
}

/// The wake deadline of a loop that waits with a timeout of its own: the
/// deadline armed, and the last one delivered, by the rule of
/// [`DeadlineDelivery`].
#[cfg(any(
    windows,
    x11_host,
    wayland_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
#[derive(Default)]
pub(crate) struct WakeDeadline {
    armed: Option<DeadlineArm>,
    delivered: Option<DeadlineDelivery>,
}

#[cfg(any(
    windows,
    x11_host,
    wayland_host,
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    test
))]
impl WakeDeadline {
    /// Whether the armed deadline is due at `now`. A due deadline is delivery
    /// debt: the caller asks every window for a frame, and the delivery is
    /// recorded at `frames`, the count before those frames.
    pub(crate) fn take_due(&mut self, now: web_time::Instant, frames: u64) -> bool {
        let Some(arm) = self.armed.take_if(|arm| arm.deadline <= now) else {
            return false;
        };
        self.delivered = Some(DeadlineDelivery { arm, frames });
        true
    }

    /// The application's handler answered `answer` at the frame count
    /// `frames`: arm it unless it is the deadline last delivered and no frame
    /// ran since. The instant to wait until, if any.
    pub(crate) fn arm(
        &mut self,
        answer: Option<web_time::Instant>,
        frames: u64,
    ) -> Option<web_time::Instant> {
        self.armed = match answer {
            Some(deadline)
                if self
                    .delivered
                    .as_ref()
                    .is_none_or(|last| last.admits(deadline, frames)) =>
            {
                Some(DeadlineArm { deadline })
            }
            Some(_) => None,
            None => {
                self.delivered = None;
                None
            }
        };
        self.armed.as_ref().map(|arm| arm.deadline)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use web_time::Instant;

    use super::{TurnDeadline, WakeDeadline};

    /// A turn delivered for the deadline serves every answer not later than
    /// its time: a past instant answered again is not armed again, a later
    /// one is.
    #[test]
    fn a_served_turn_deadline_is_not_armed_again() {
        let start = Instant::now();
        let at = start + Duration::from_millis(5);
        let served_at = start + Duration::from_millis(7);
        let later = start + Duration::from_millis(9);
        let mut deadline = TurnDeadline::default();

        assert_eq!(deadline.arm(Some(at)), Some(at), "a new deadline is armed");
        assert!(!deadline.take_due(start), "not due before its instant");
        assert!(deadline.take_due(served_at), "due at its instant");
        assert!(!deadline.take_due(served_at), "delivered once");

        // (case, answer, instant armed)
        let table: &[(&str, Option<Instant>, Option<Instant>)] = &[
            ("the instant served", Some(at), None),
            ("an instant before the turn's time", Some(start), None),
            ("the turn's own time", Some(served_at), None),
            ("no deadline", None, None),
            ("still served after a quiet answer", Some(at), None),
            ("an instant after the turn's time", Some(later), Some(later)),
        ];
        for (case, answer, armed) in table {
            assert_eq!(deadline.arm(*answer), *armed, "{case}");
            assert_eq!(deadline.armed(), *armed, "{case}: what a timer reads");
        }
    }

    /// A deadline is armed when asked for, delivered once when due, and armed
    /// again for the same instant only after a frame ran: a deadline no frame
    /// could service does not spin the loop, and one a frame left due again
    /// gets its frame.
    #[test]
    fn a_delivered_deadline_is_armed_again_only_after_a_frame() {
        let start = Instant::now();
        let at = start + Duration::from_millis(5);
        let later = start + Duration::from_millis(9);
        let mut deadline = WakeDeadline::default();

        assert_eq!(
            deadline.arm(Some(at), 0),
            Some(at),
            "a new deadline is armed"
        );
        assert!(!deadline.take_due(start, 0), "not due before its instant");
        assert!(deadline.take_due(at, 0), "due at its instant");
        assert!(!deadline.take_due(at, 0), "delivered once");

        // (case, answer, frame count, instant armed)
        let table: &[(&str, Option<Instant>, u64, Option<Instant>)] = &[
            ("the same instant, no frame since", Some(at), 0, None),
            ("still no frame", Some(at), 0, None),
            ("a frame ran since", Some(at), 1, Some(at)),
        ];
        for (case, answer, frames, armed) in table {
            assert_eq!(deadline.arm(*answer, *frames), *armed, "{case}");
        }
        assert!(deadline.take_due(at, 1), "the re-armed deadline is due");

        assert_eq!(
            deadline.arm(Some(later), 1),
            Some(later),
            "another instant is armed whatever the frames"
        );
        assert_eq!(deadline.arm(None, 1), None, "no deadline asked for");
        assert!(!deadline.take_due(later, 1), "nothing armed");
        assert_eq!(
            deadline.arm(Some(at), 1),
            Some(at),
            "after a quiet answer, the old instant is new again"
        );
    }
}
