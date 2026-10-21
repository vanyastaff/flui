//! How a macOS application answers `applicationShouldTerminate:` while the
//! user's session is ending.
//!
//! Logging out, restarting and shutting down first post
//! `NSWorkspaceWillPowerOffNotification` to every application and then ask each
//! one to terminate. The notification is where the application is asked whether
//! it needs the user to wait; the terminate request is where the answer takes
//! effect: `NSTerminateLater` holds the session end until
//! `replyToApplicationShouldTerminate:`. A terminate request with no such
//! notification before it (Cmd-Q, the Dock menu) is an ordinary quit.
//!
//! The session end can be called off by the user or by another application, and
//! AppKit says nothing then. An announcement therefore lapses: after
//! [`ANNOUNCEMENT_LIFETIME`] with no terminate request the session end is taken
//! to be cancelled, and a Cmd-Q after that is an ordinary quit again.
//!
//! The gate is the state that connects the two, with no AppKit and no clock in
//! it. Every announcement and deferral has an epoch, so a timer armed for an
//! earlier one never settles a later one.

use std::time::Duration;

/// How long an announcement stays in force without a terminate request
/// following it. The request follows within moments; one that comes much later
/// belongs to a quit the user started after the session end was called off.
pub(crate) const ANNOUNCEMENT_LIFETIME: Duration = Duration::from_secs(10);

/// How long a deferred answer waits for the application to release its blocks.
/// The operating system gives a terminating application a few seconds more
/// before it asks the user.
pub(crate) const DEFERRAL_LIMIT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No session end is known.
    Idle,
    /// The session end was announced and the application answered.
    Announced { blocked: bool, epoch: u64 },
    /// `NSTerminateLater` was answered; the reply is owed.
    Deferred { epoch: u64 },
}

/// What `applicationShouldTerminate:` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Termination {
    /// An ordinary quit: cancel this request and quit through the run loop.
    Quit,
    /// The session is ending and the application needs nothing more: answer
    /// `NSTerminateNow`.
    Now,
    /// The session is ending and the application asked to wait: answer
    /// `NSTerminateLater` and settle `epoch` after [`DEFERRAL_LIMIT`].
    Later { epoch: u64 },
}

/// The state between a power-off notification and the terminate request.
#[derive(Debug)]
pub(crate) struct TerminateGate {
    state: State,
    last_epoch: u64,
}

impl TerminateGate {
    pub(crate) const fn new() -> Self {
        Self {
            state: State::Idle,
            last_epoch: 0,
        }
    }

    fn next_epoch(&mut self) -> u64 {
        self.last_epoch = self
            .last_epoch
            .checked_add(1)
            .expect("BUG: more announcements than a u64 counts");
        self.last_epoch
    }

    /// The system announced a session end and the application answered
    /// `blocked`. Returns the epoch the announcement lapses under after
    /// [`ANNOUNCEMENT_LIFETIME`]. `None` while an answer is already deferred:
    /// that deferral still owes its reply and is left alone.
    pub(crate) fn announced(&mut self, blocked: bool) -> Option<u64> {
        if matches!(self.state, State::Deferred { .. }) {
            return None;
        }
        let epoch = self.next_epoch();
        self.state = State::Announced { blocked, epoch };
        Some(epoch)
    }

    /// The announcement `epoch` ran out with no terminate request. `true` when
    /// it was still in force: the session end is taken to be cancelled.
    pub(crate) fn announcement_lapsed(&mut self, epoch: u64) -> bool {
        if matches!(self.state, State::Announced { epoch: current, .. } if current == epoch) {
            self.state = State::Idle;
            true
        } else {
            false
        }
    }

    /// The system asked the application to terminate.
    pub(crate) fn terminate_requested(&mut self) -> Termination {
        match self.state {
            State::Announced { blocked: true, .. } => {
                let epoch = self.next_epoch();
                self.state = State::Deferred { epoch };
                Termination::Later { epoch }
            }
            State::Announced { blocked: false, .. } => {
                self.state = State::Idle;
                Termination::Now
            }
            // A second request while the reply is owed: the answer is already
            // `Later`, and the epoch stays.
            State::Deferred { epoch } => Termination::Later { epoch },
            State::Idle => Termination::Quit,
        }
    }

    /// The application released its last block. `true` when the reply is owed
    /// now. A release before the terminate request only clears the block, so
    /// the request is answered `Now`.
    pub(crate) fn released(&mut self) -> bool {
        match self.state {
            State::Deferred { .. } => {
                self.state = State::Idle;
                true
            }
            State::Announced { epoch, .. } => {
                self.state = State::Announced {
                    blocked: false,
                    epoch,
                };
                false
            }
            State::Idle => false,
        }
    }

    /// The deferral `epoch` ran out. `true` when its reply is owed now; a
    /// timer of an earlier deferral, or one already settled, owes nothing.
    pub(crate) fn deferral_ran_out(&mut self, epoch: u64) -> bool {
        if self.state == (State::Deferred { epoch }) {
            self.state = State::Idle;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deferred(gate: &mut TerminateGate) -> u64 {
        match gate.terminate_requested() {
            Termination::Later { epoch } => epoch,
            other => panic!("a blocked session end defers the answer, got {other:?}"),
        }
    }

    #[test]
    fn a_quit_with_no_session_end_is_an_ordinary_quit() {
        let mut gate = TerminateGate::new();
        assert_eq!(gate.terminate_requested(), Termination::Quit);
        assert!(!gate.released(), "nothing is owed");
    }

    #[test]
    fn a_session_end_the_application_lets_pass_terminates_at_once() {
        let mut gate = TerminateGate::new();
        gate.announced(false);
        assert_eq!(gate.terminate_requested(), Termination::Now);
        assert_eq!(
            gate.terminate_requested(),
            Termination::Quit,
            "the announcement is used up by the first request"
        );
    }

    #[test]
    fn a_blocked_session_end_waits_for_the_release() {
        let mut gate = TerminateGate::new();
        gate.announced(true);
        let epoch = deferred(&mut gate);
        assert!(gate.released(), "the release owes the reply");
        assert!(
            !gate.deferral_ran_out(epoch),
            "and settles the deferral for good"
        );
        assert!(!gate.released(), "a second release owes nothing");
    }

    #[test]
    fn a_deferral_ends_at_its_limit() {
        let mut gate = TerminateGate::new();
        gate.announced(true);
        let epoch = deferred(&mut gate);
        assert!(gate.deferral_ran_out(epoch));
        assert!(!gate.released(), "a release after the limit owes nothing");
    }

    #[test]
    fn a_timer_of_an_earlier_deferral_does_not_settle_a_later_one() {
        let mut gate = TerminateGate::new();
        gate.announced(true);
        let first = deferred(&mut gate);
        assert!(gate.released());

        gate.announced(true);
        let second = deferred(&mut gate);
        assert_ne!(first, second);
        assert!(!gate.deferral_ran_out(first), "the older timer fires late");
        assert!(gate.deferral_ran_out(second));
    }

    #[test]
    fn a_block_released_before_the_request_does_not_defer_it() {
        let mut gate = TerminateGate::new();
        gate.announced(true);
        assert!(!gate.released());
        assert_eq!(gate.terminate_requested(), Termination::Now);
    }

    #[test]
    fn a_session_end_called_off_becomes_an_ordinary_quit_again() {
        let mut gate = TerminateGate::new();
        let epoch = gate.announced(false).expect("nothing is deferred");
        // The user called the logout off; AppKit said nothing, and the
        // announcement ran out.
        assert!(gate.announcement_lapsed(epoch));
        assert_eq!(gate.terminate_requested(), Termination::Quit);
    }

    #[test]
    fn the_lapse_of_an_earlier_announcement_leaves_a_later_one_standing() {
        let mut gate = TerminateGate::new();
        let first = gate.announced(false).expect("nothing is deferred");
        let second = gate.announced(true).expect("nothing is deferred");
        assert_ne!(first, second);
        assert!(!gate.announcement_lapsed(first), "the older timer fires");
        assert!(matches!(
            gate.terminate_requested(),
            Termination::Later { .. }
        ));
        assert!(
            !gate.announcement_lapsed(second),
            "an announcement that was answered has nothing left to lapse"
        );
    }

    #[test]
    fn an_announcement_during_a_deferral_leaves_the_deferral_alone() {
        let mut gate = TerminateGate::new();
        gate.announced(true);
        let epoch = deferred(&mut gate);
        assert_eq!(gate.announced(false), None);
        assert_eq!(
            gate.terminate_requested(),
            Termination::Later { epoch },
            "the deferral and its epoch stand"
        );
        assert!(gate.deferral_ran_out(epoch));
    }

    #[test]
    fn the_limits_are_as_long_as_the_operating_system_waits() {
        // The reply must come well within the few seconds AppKit gives a
        // terminating application, and a lapse must outlast the terminate
        // request that follows an announcement.
        assert!(DEFERRAL_LIMIT <= Duration::from_secs(5));
        assert!(ANNOUNCEMENT_LIFETIME > DEFERRAL_LIMIT);
    }
}
