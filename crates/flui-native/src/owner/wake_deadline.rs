//! The owner's wake deadline as a loop arms and delivers it: the instant the
//! application's handler asks the loop to wake at
//! (`AppHandler::wake_deadline`), and the rule that keeps a deadline no frame
//! could service from spinning the loop. The Win32 and X11 loops wait on it.

/// The last deadline the loop delivered. The application's handler answers
/// with an instant only, so the same instant is either work no frame has had
/// a chance to service, which must not spin, or work a frame left or accepted
/// at the instant it just serviced (a queue of obligations all due at one
/// instant answers the same instant once per item), which must get its frame.
/// The frame count is the delivery generation that tells them apart. A run
/// has one handler, so the instant alone names the answer.
pub(crate) struct DeadlineDelivery {
    pub(crate) arm: DeadlineArm,
    /// The platform's frame count when the delivery was made.
    pub(crate) frames: u64,
}

impl DeadlineDelivery {
    /// Whether `deadline` may be armed after this delivery: a different
    /// instant always, the same one whenever a frame callback ran since. An
    /// answer no frame could have serviced stays delivered, so a delivery that
    /// reaches no frame never spins the loop.
    pub(crate) fn admits(&self, deadline: web_time::Instant, frames: u64) -> bool {
        self.arm.deadline != deadline || frames != self.frames
    }
}

/// The last wake deadline a modal loop delivered, kept across its frame
/// requests ([`ModalFrames`](super::windows::modal_loop::ModalFrames)): the
/// main loop's rule, held apart from the main loop's own record, which is
/// suspended under the modal loop.
#[cfg(windows)]
#[derive(Default)]
pub(crate) struct ModalDelivery(std::cell::RefCell<Option<DeadlineDelivery>>);

#[cfg(windows)]
impl ModalDelivery {
    /// Whether `deadline` is due for delivery after the last one.
    pub(crate) fn admits(&self, deadline: web_time::Instant, frames: u64) -> bool {
        self.0
            .borrow()
            .as_ref()
            .is_none_or(|last| last.admits(deadline, frames))
    }

    /// `deadline` was delivered at the frame count `frames`.
    pub(crate) fn record(&self, deadline: web_time::Instant, frames: u64) {
        *self.0.borrow_mut() = Some(DeadlineDelivery {
            arm: DeadlineArm { deadline },
            frames,
        });
    }

    /// No deadline is asked for: the next one is new whatever it is.
    pub(crate) fn clear(&self) {
        *self.0.borrow_mut() = None;
    }
}

/// How many frames this platform's windows' handlers have produced. The
/// loop compares it across a deadline delivery to tell an answer repeated
/// after a serviced frame from one nothing has serviced yet. Each window's
/// [`WindowDelivery`](crate::owner::window_delivery::WindowDelivery) counts
/// into it when its handler's `frame_requested` produces a frame, so a frame
/// request no handler ran a frame for (a declined one, or a window that let go
/// of its handler) leaves it unchanged.
#[cfg(any(windows, x11_host))]
#[derive(Default)]
pub(crate) struct FrameCount(std::sync::Arc<std::sync::atomic::AtomicU64>);

#[cfg(any(windows, x11_host))]
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
pub(crate) struct DeadlineArm {
    pub(crate) deadline: web_time::Instant,
}

/// The wake deadline of a loop that waits with a timeout of its own: the
/// deadline armed, and the last one delivered, by the rule of
/// [`DeadlineDelivery`].
#[derive(Default)]
pub(crate) struct WakeDeadline {
    armed: Option<DeadlineArm>,
    delivered: Option<DeadlineDelivery>,
}

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

    use super::WakeDeadline;

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
