/// A two-phase redraw decision around one native event poll.
///
/// Construction only observes pending work. Consumption is available on the
/// post-poll value, so a lifecycle event delivered by `poll` can revoke frame
/// delivery before a redraw request is acknowledged.
///
/// Pending sources force an immediate poll only while execution is resumed.
/// A due deadline is stateless and stays due until its producer advances it;
/// allowing it to force zero-timeout polls while paused would busy-loop even
/// though no frame can be delivered. A redraw is likewise preserved rather
/// than consumed until the post-poll execution snapshot permits delivery.
pub(crate) struct RedrawPoll {
    deadline_due: bool,
    redraw_observed: bool,
}

impl RedrawPoll {
    pub(crate) fn new(deadline_due: bool, redraw_observed: bool) -> Self {
        Self {
            deadline_due,
            redraw_observed,
        }
    }

    pub(crate) fn poll<T>(
        self,
        resumed: bool,
        poll: impl FnOnce(bool) -> T,
    ) -> (T, PostPollRedraw) {
        let force_immediate = resumed && (self.deadline_due || self.redraw_observed);
        let output = poll(force_immediate);
        (
            output,
            PostPollRedraw {
                deadline_due: self.deadline_due,
            },
        )
    }
}

pub(crate) struct PostPollRedraw {
    deadline_due: bool,
}

impl PostPollRedraw {
    pub(crate) fn take_if_deliverable(
        self,
        resumed_after_poll: bool,
        take_redraw: impl FnOnce() -> bool,
    ) -> bool {
        if !resumed_after_poll {
            return false;
        }
        let redraw_requested = take_redraw();
        self.deadline_due || redraw_requested
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::RedrawPoll;

    #[test]
    fn pause_during_poll_preserves_redraw_until_a_resumed_iteration() {
        let resumed = Cell::new(true);
        let redraw_requested = Cell::new(true);

        let ((), after_pause) =
            RedrawPoll::new(false, redraw_requested.get()).poll(resumed.get(), |force_immediate| {
                assert!(force_immediate);
                resumed.set(false);
            });
        assert!(
            !after_pause.take_if_deliverable(resumed.get(), || { redraw_requested.replace(false) })
        );
        assert!(redraw_requested.get(), "Pause must preserve the request");

        let ((), after_resume) =
            RedrawPoll::new(false, redraw_requested.get()).poll(resumed.get(), |force_immediate| {
                assert!(!force_immediate);
                resumed.set(true);
            });
        assert!(
            after_resume.take_if_deliverable(resumed.get(), || { redraw_requested.replace(false) })
        );
        assert!(
            !redraw_requested.get(),
            "delivery consumes the request once"
        );
    }
}
