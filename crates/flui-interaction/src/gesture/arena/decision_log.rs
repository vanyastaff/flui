//! The verdicts an arena committed, kept for a runtime that records them.
//!
//! Recording input replays only if the decisions it produced can be compared
//! (ADR-0215 §1): a recording carries each arena verdict as a row, including
//! the ones a deadline made with no input of its own. The log stores plain
//! values and runs no user code; it is off (and allocation-free) until a
//! recording turns it on.

use std::{cell::RefCell, sync::Arc};

use flui_foundation::MonotonicClock;
use flui_platform_api::EventTime;

use super::GestureDisposition;
use crate::ids::PointerId;

/// One verdict an arena delivered to a member: who won or lost which
/// pointer's competition, and when on the event timeline.
///
/// `member` names the member's concrete type. It is a description for
/// comparing two runs of the same application, not an identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ArenaDecision {
    /// The pointer whose competition was decided.
    pub pointer: PointerId,
    /// When the verdict was committed, on the arena's event timeline.
    pub time: EventTime,
    /// Whether the member won or lost.
    pub disposition: GestureDisposition,
    /// The member's concrete type name.
    pub member: &'static str,
}

impl ArenaDecision {
    /// A verdict for `member` on `pointer` at `time`.
    #[must_use]
    pub const fn new(
        pointer: PointerId,
        time: EventTime,
        disposition: GestureDisposition,
        member: &'static str,
    ) -> Self {
        Self {
            pointer,
            time,
            disposition,
            member,
        }
    }
}

/// The verdicts committed while a recording listens.
#[derive(Default)]
pub(super) struct DecisionLog {
    listening: RefCell<Option<Listening>>,
}

struct Listening {
    clock: Arc<dyn MonotonicClock>,
    decisions: Vec<ArenaDecision>,
}

impl DecisionLog {
    /// Start (with `Some(clock)`) or stop keeping verdicts; stopping drops the
    /// ones not yet taken.
    pub(super) fn listen(&self, clock: Option<Arc<dyn MonotonicClock>>) {
        let outgoing = self.listening.replace(clock.map(|clock| Listening {
            clock,
            decisions: Vec::new(),
        }));
        drop(outgoing);
    }

    pub(super) fn is_listening(&self) -> bool {
        self.listening.borrow().is_some()
    }

    /// Note a committed verdict. Free when nothing listens.
    pub(super) fn note(
        &self,
        pointer: PointerId,
        disposition: GestureDisposition,
        member: &'static str,
    ) {
        if let Some(listening) = self.listening.borrow_mut().as_mut() {
            let time = crate::events::interaction_now(listening.clock.as_ref());
            listening
                .decisions
                .push(ArenaDecision::new(pointer, time, disposition, member));
        }
    }

    /// The verdicts noted since the last take, oldest first.
    pub(super) fn take(&self) -> Vec<ArenaDecision> {
        self.listening
            .borrow_mut()
            .as_mut()
            .map(|listening| std::mem::take(&mut listening.decisions))
            .unwrap_or_default()
    }
}
