//! Where a wheel or trackpad sequence ends without a further packet
//! (ADR-0209 §5): at the binding's latch idle deadline, on the arena's
//! event timeline.
//!
//! The binding latches a source's packets into one sequence and reports,
//! with each one, when that latch lapses
//! ([`GestureArena::dispatched_latch_end`]). The leaf that leads the
//! sequence arms one deadline of the arena's owner clock for that event
//! time, so the host wakes for it like a recognizer's, and the poll ends
//! the sequence there: exactly when the binding's latch lapses, so the
//! next packet's new latch is a new sequence for the channel too. A packet
//! before the deadline moves it on; a packet the binding marks as its
//! latch's last ends the sequence once it is dispatched.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use flui_foundation::panic::PanicRecovery;
use flui_interaction::{
    GestureArena, GestureArenaMember, GestureContest, GestureDeadlineRegistration,
    GestureDeadlineUpdate, ScrollLatchEnd,
};
use flui_platform_api::EventTime;

use super::ScrollSequence;

/// The armed idle end of one sequence. Dropping it withdraws the deadline.
pub(crate) struct IdleEnd {
    lapse: Rc<Lapse>,
    registration: GestureDeadlineRegistration,
}

impl IdleEnd {
    /// Arm the idle end of `sequence` on `arena`'s owner clock; `None` when
    /// the arena admits no deadline (closed, or identities exhausted), and
    /// the sequence then ends with the next one, as without a deadline.
    pub(crate) fn arm(arena: &GestureArena, sequence: &Rc<ScrollSequence>) -> Option<Self> {
        let lapse = Rc::new(Lapse {
            due: Cell::new(None),
            sequence: Rc::downgrade(sequence),
        });
        let member: Rc<dyn GestureArenaMember> = Rc::clone(&lapse) as Rc<dyn GestureArenaMember>;
        match arena.register_deadline(&member) {
            Ok(registration) => Some(Self {
                lapse,
                registration,
            }),
            Err(error) => {
                tracing::debug!(
                    %error,
                    "a wheel sequence has no idle deadline; it ends with the next sequence"
                );
                None
            }
        }
    }

    /// The sequence now ends at `due`, unless moved on again. The returned
    /// handle publishes the time; [`publish`](Self::publish) it once no
    /// borrow is held, since publishing wakes the host.
    pub(crate) fn move_to(&self, due: EventTime) -> GestureDeadlineUpdate {
        self.lapse.due.set(Some(due));
        self.registration.update_handle()
    }

    /// Publish `due` through `update`, from [`move_to`](Self::move_to).
    pub(crate) fn publish(update: &GestureDeadlineUpdate, due: EventTime) {
        if let Err(error) = update.publish(Some(due)) {
            tracing::debug!(%error, "a wheel sequence's idle deadline could not be published");
        }
    }
}

/// The deadline source: polled once its time is due.
struct Lapse {
    due: Cell<Option<EventTime>>,
    sequence: Weak<ScrollSequence>,
}

impl GestureArenaMember for Lapse {
    // It joins no contest.
    fn accept_gesture(&self, _contest: GestureContest) {}

    fn reject_gesture(&self, _contest: GestureContest) {}

    fn deadline(&self) -> Option<EventTime> {
        self.due.get()
    }

    fn poll_deadline(&self, now: EventTime) {
        if self.due.get().is_none_or(|due| due > now) {
            return;
        }
        // Disarmed before anyone is told: a repeated poll ends nothing twice.
        self.due.set(None);
        let Some(sequence) = self.sequence.upgrade() else {
            return;
        };
        let mut recovery = PanicRecovery::new();
        sequence.lapse(&mut recovery);
        recovery.finish();
    }
}

/// Follow the binding's report of where the latch of the packet just
/// dispatched ends, for `sequence`, the one its leaf leads for it.
pub(super) fn follow_latch(
    sequence: &Rc<ScrollSequence>,
    arena: &GestureArena,
    end: ScrollLatchEnd,
    recovery: &mut PanicRecovery,
) {
    match end {
        ScrollLatchEnd::IdleAt(due) => sequence.lapse_at(arena, due),
        ScrollLatchEnd::WithThisPacket => sequence.lapse(recovery),
        // A future kind of end the channel does not know waits for the
        // next sequence.
        _ => {}
    }
}
