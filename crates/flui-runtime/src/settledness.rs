//! What keeps an application from being idle, gathered at one instant (ADR-0400 §3).
//!
//! The host that runs a UI runtime's turns owns the list. After each turn, once its own steps
//! after the pump are done, it gathers the runtime's half
//! ([`UiRuntime::unsettled`](crate::ui_runtime::UiRuntime::unsettled)) and its own into one
//! [`Unsettled`]: its loop wakes at [`Unsettled::wake`], a settle ends when the value lists no
//! cause, and the agent's settled waits are answered from the same value
//! ([`UiRuntime::answer_settled`](crate::ui_runtime::UiRuntime::answer_settled)).
//!
//! A cause is added through the method of what discharges it, which writes the cause's
//! `cleared_by` and, for a deadline, its `due_in_ms`: no caller states the wake rule, and a
//! deadline's exact instant is kept beside its rounded copy. A deadline that is no cause (a parked
//! wait's) is a wake alone.

use flui_protocol::tree::WindowId;
use flui_protocol::wait::{ClearedBy, UnsettledCause, UnsettledReason};
use web_time::Instant;

/// How many answers one parked wait takes at a single instant before it ends
/// [`Unsatisfied`](flui_protocol::wait::WaitEnd::Unsatisfied), naming the causes due at that
/// instant: a cause that re-arms at the instant it is served keeps a virtual clock from moving
/// (egui's `max_steps`). A host's loop that runs turns for a wait stops after as many.
pub const TURNS_AT_ONE_INSTANT: u32 = 10_000;

/// The causes that keep an application from being idle at one instant, each with what discharges
/// it, and the earliest instant its host's loop wakes for. See the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsettled {
    /// The instant the list was gathered at: a cause the next turn serves wakes the loop here.
    at: Instant,
    causes: Vec<Listed>,
    /// Whether a cause the next turn serves is listed.
    next_frame: bool,
    /// Whether a frame that keeps nothing from being idle is asked for at once.
    wake_at_once: bool,
    /// The earliest deadline, of a cause or a wake alone.
    next_deadline: Option<Instant>,
}

/// A cause, with the exact instant of the deadline that serves it where one does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    cause: UnsettledCause,
    due: Option<Instant>,
}

impl Unsettled {
    /// An empty list, gathered at `at`.
    #[must_use]
    pub fn new(at: Instant) -> Self {
        Self {
            at,
            causes: Vec::new(),
            next_frame: false,
            wake_at_once: false,
            next_deadline: None,
        }
    }

    /// The instant the list was gathered at.
    #[must_use]
    pub fn at(&self) -> Instant {
        self.at
    }

    /// Lists `reason`, of `window` or of the whole application, as served by the next turn.
    pub fn next_frame(&mut self, reason: UnsettledReason, window: Option<WindowId>) {
        self.next_frame = true;
        self.push(cause(reason, window, ClearedBy::NextFrame), None);
    }

    /// Lists `reason` as served by a turn at `due`: its report carries the whole milliseconds
    /// from the gather instant to `due`, rounded down, and the loop wakes at `due` exactly.
    pub fn deadline(&mut self, reason: UnsettledReason, window: Option<WindowId>, due: Instant) {
        let remaining = due.saturating_duration_since(self.at);
        let due_in_ms = u64::try_from(remaining.as_millis()).unwrap_or(u64::MAX);
        self.wake_at(due);
        self.push(
            cause(reason, window, ClearedBy::Deadline).due_in_ms(due_in_ms),
            Some(due),
        );
    }

    /// Lists `reason` as discharged only by the test: no turn serves it, and the loop never wakes
    /// for it.
    pub fn test(&mut self, reason: UnsettledReason, window: Option<WindowId>) {
        self.push(cause(reason, window, ClearedBy::Test), None);
    }

    /// Lists `reason` as answered by something outside the application: no turn serves it, and
    /// the loop never wakes for it.
    pub fn outside(&mut self, reason: UnsettledReason, window: Option<WindowId>) {
        self.push(cause(reason, window, ClearedBy::Outside), None);
    }

    /// A deadline the loop wakes for that keeps nothing from being idle: a parked wait's.
    pub fn wake_at(&mut self, due: Instant) {
        self.next_deadline = Some(self.next_deadline.map_or(due, |known| known.min(due)));
    }

    /// A frame the loop runs at once that keeps nothing from being idle: one the development
    /// agent asks for itself (a held act's next boundary, a wait that counts frames). It wakes
    /// the loop as a cause the next turn serves does, and is no cause.
    pub fn wake_at_once(&mut self) {
        self.wake_at_once = true;
    }

    /// When the host's loop wakes: the gather instant while a cause the next turn serves is
    /// listed or a frame is asked for at once, otherwise the earliest deadline, of a cause or a
    /// wake alone. Never for a cause only the test or something outside discharges; `None` when
    /// nothing but those, or nothing at all, is listed.
    #[must_use]
    pub fn wake(&self) -> Option<Instant> {
        if self.next_frame || self.wake_at_once {
            Some(self.at)
        } else {
            self.next_deadline
        }
    }

    /// The earliest deadline, of a cause or a wake alone, whatever else is listed.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.next_deadline
    }

    /// Whether no cause is listed: the application is idle. A wake alone is no cause.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.causes.is_empty()
    }

    /// The causes, in the order they were listed.
    pub fn causes(&self) -> impl ExactSizeIterator<Item = &UnsettledCause> + '_ {
        self.causes.iter().map(|listed| &listed.cause)
    }

    /// The causes a turn at `instant` serves: the next turn's, and each deadline at or before it.
    pub(crate) fn due_by(&self, instant: Instant) -> impl Iterator<Item = &UnsettledCause> + '_ {
        self.causes
            .iter()
            .filter(move |listed| match listed.due {
                Some(due) => due <= instant,
                None => listed.cause.cleared_by == ClearedBy::NextFrame,
            })
            .map(|listed| &listed.cause)
    }

    fn push(&mut self, cause: UnsettledCause, due: Option<Instant>) {
        self.causes.push(Listed { cause, due });
    }
}

/// `reason`, of `window` when it names one.
fn cause(
    reason: UnsettledReason,
    window: Option<WindowId>,
    cleared_by: ClearedBy,
) -> UnsettledCause {
    let cause = UnsettledCause::new(reason, cleared_by);
    match window {
        Some(window) => cause.window(window),
        None => cause,
    }
}

/// What a host hands [`UiRuntime::answer_settled`](crate::ui_runtime::UiRuntime::answer_settled)
/// after its turn.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a host composes its half of the list or it does not; ADR-0400 §3 names no third"
)]
pub enum HostHalf<'a> {
    /// The host's whole list: the runtime's half and its own, gathered into one value after
    /// its turn. A wait may end idle on it.
    Complete(&'a Unsettled),
    /// The host composes no half of its own: the runtime answers from its half alone, never
    /// idle, and its answers say so (`SettleScope::RuntimeOnly`).
    Absent,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_protocol::wait::{ClearedBy, UnsettledReason};
    use web_time::Instant;

    use super::Unsettled;

    /// The wake is the gather instant while a next-turn cause is listed, the earliest deadline
    /// otherwise, and none for a cause only the test or the outside discharges.
    #[test]
    fn the_wake_follows_what_discharges_each_cause() {
        let at = Instant::now();
        let later = at + Duration::from_millis(40);
        let sooner = at + Duration::from_micros(300);

        let mut held = Unsettled::new(at);
        held.test(UnsettledReason::Background, None);
        held.outside(UnsettledReason::PlatformReply, None);
        assert_eq!(held.wake(), None, "nothing a turn serves");
        assert!(!held.is_empty());

        held.deadline(UnsettledReason::Timer, None, later);
        held.wake_at(sooner);
        assert_eq!(
            held.wake(),
            Some(sooner),
            "the earliest deadline, a wake alone included"
        );

        let mut asked = held.clone();
        asked.wake_at_once();
        assert_eq!(
            asked.wake(),
            Some(at),
            "a frame asked for at once wakes at once"
        );
        assert_eq!(
            asked.causes().count(),
            held.causes().count(),
            "and is no cause"
        );

        held.next_frame(UnsettledReason::NeedsRedraw, None);
        assert_eq!(held.wake(), Some(at), "the next turn's cause wakes at once");
        assert_eq!(held.next_deadline(), Some(sooner));
    }

    /// A deadline keeps its exact instant for the wake and reports whole milliseconds, rounded
    /// down, for the report.
    #[test]
    fn a_deadline_reports_whole_milliseconds_and_wakes_at_its_instant() {
        let at = Instant::now();
        let due = at + Duration::from_micros(1_999);
        let mut unsettled = Unsettled::new(at);
        unsettled.deadline(UnsettledReason::Timer, None, due);

        let timer = unsettled.causes().next().expect("the timer is listed");
        assert_eq!(timer.cleared_by, ClearedBy::Deadline);
        assert_eq!(timer.due_in_ms, Some(1));
        assert_eq!(unsettled.wake(), Some(due));
        assert_eq!(unsettled.due_by(at).count(), 0, "not due at the gather");
        assert_eq!(unsettled.due_by(due).count(), 1, "due at its instant");

        // A deadline further than the report's milliseconds can count reports the most.
        let beyond = at + Duration::from_secs(u64::MAX / 1_000 + 1);
        let mut far = Unsettled::new(at);
        far.deadline(UnsettledReason::Timer, None, beyond);
        let timer = far.causes().next().expect("the timer is listed");
        assert_eq!(timer.due_in_ms, Some(u64::MAX));
        assert_eq!(far.wake(), Some(beyond));
    }
}
