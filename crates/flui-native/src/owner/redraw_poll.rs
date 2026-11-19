/// A two-phase redraw decision around one native event poll.
///
/// Construction only observes a pending redraw. Consumption is available on
/// the post-poll value, so a lifecycle event delivered by `poll` can revoke
/// frame delivery before a redraw request is acknowledged.
///
/// A pending redraw forces an immediate poll only while the window runs:
/// forcing one while stopped would busy-loop even though no frame can be
/// delivered. The redraw is preserved rather than consumed until the
/// post-poll execution snapshot permits delivery. A due wake deadline reaches
/// here as such a redraw request (`WakeDeadline::turn`).
pub(crate) struct RedrawPoll {
    redraw_observed: bool,
}

impl RedrawPoll {
    pub(crate) fn new(redraw_observed: bool) -> Self {
        Self { redraw_observed }
    }

    pub(crate) fn poll<T>(
        self,
        running: bool,
        poll: impl FnOnce(bool) -> T,
    ) -> (T, PostPollRedraw) {
        let force_immediate = running && self.redraw_observed;
        (poll(force_immediate), PostPollRedraw(()))
    }
}

/// The proof that the poll ran, which the redraw is consumed through: only
/// [`RedrawPoll::poll`] makes one, so no consumption comes before a poll.
pub(crate) struct PostPollRedraw(());

impl PostPollRedraw {
    pub(crate) fn take_if_deliverable(
        self,
        running_after_poll: bool,
        take_redraw: impl FnOnce() -> bool,
    ) -> bool {
        // The proof is spent here.
        let Self(()) = self;
        running_after_poll && take_redraw()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::RedrawPoll;

    #[test]
    fn a_stop_during_poll_preserves_redraw_until_a_running_iteration() {
        let running = Cell::new(true);
        let redraw_requested = Cell::new(true);

        let ((), after_stop) =
            RedrawPoll::new(redraw_requested.get()).poll(running.get(), |force_immediate| {
                assert!(force_immediate);
                running.set(false);
            });
        assert!(
            !after_stop.take_if_deliverable(running.get(), || { redraw_requested.replace(false) })
        );
        assert!(redraw_requested.get(), "a stop must preserve the request");

        let ((), after_start) =
            RedrawPoll::new(redraw_requested.get()).poll(running.get(), |force_immediate| {
                assert!(!force_immediate);
                running.set(true);
            });
        assert!(
            after_start.take_if_deliverable(running.get(), || { redraw_requested.replace(false) })
        );
        assert!(
            !redraw_requested.get(),
            "delivery consumes the request once"
        );
    }
}
