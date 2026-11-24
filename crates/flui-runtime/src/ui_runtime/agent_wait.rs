//! An agent's waits (ADR-0238 §7, ADR-0400 §3): conditions the owner holds out for, evaluated
//! after frames and never polled.
//!
//! A wait is parked on its presentation. One whose condition holds without reading settledness
//! is evaluated when it arrives (a condition that already holds answers at once) and after each
//! frame and owner turn. Everything that reads what keeps the application from being idle is
//! answered from the host's list after its turn ([`UiRuntime::answer_settled`]): a condition
//! containing `Settled`, every wait's timeout and the idle end. The deadline is on the
//! presentation's own clock, so a manual clock expires it deterministically, and it is a wake of
//! the list ([`Unsettled::wake_at`]) so a window that draws nothing still times out. A timeout is
//! an answer (`unsatisfied`, with the last observation and why the window had not settled), not
//! an error.

use std::panic::resume_unwind;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_protocol::{
    tree::WindowId, wait::ClearedBy, wait::Condition, wait::SettleScope, wait::UnsettledCause,
    wait::WaitEnd, wait::WaitOutcome, wait::WaitParams,
};
use flui_semantics::{EventSet, Placement, WaitFacts, counts_frames};
use web_time::Instant;

use super::UiRuntime;
use super::agent::{AgentError, ReplySender, send_reply};
use crate::presentation::PresentationState;
use crate::settledness::{TURNS_AT_ONE_INSTANT, Unsettled};

/// The most waits one presentation holds at once. A wait past it is refused with
/// [`AgentError::TooManyWaits`], never dropped silently or in place of an older one.
pub(crate) const MAX_PARKED_WAITS: usize = 256;

/// A wait, parked on its presentation until its condition holds or its deadline passes.
/// Owner thread only.
pub(crate) struct PendingWait {
    condition: Condition,
    /// Whether the answer reads settledness: the condition contains `Settled`.
    reads_settled: bool,
    /// Frames the owner presented since the wait began.
    frames: u64,
    /// The kinds of event the window emitted since the wait began.
    events: EventSet,
    /// On the presentation's clock.
    deadline: Instant,
    /// The instant of the last answer pass that saw this wait, and how many passes saw it at that
    /// instant.
    turns: Option<(Instant, u32)>,
    reply: ReplySender<WaitOutcome>,
}

impl PendingWait {
    /// The window emitted an event of `kind`; the wait sees it at its next evaluation.
    pub(crate) fn note_event(&mut self, kind: flui_protocol::events::EventKind) {
        self.events.insert(kind);
    }

    /// Whether anyone can still take the outcome.
    fn is_awaited(&self) -> bool {
        self.reply.is_awaited()
    }

    /// The window closed before the wait ended.
    pub(crate) fn abandon(self) {
        send_reply(&self.reply, None, Err(AgentError::PresentationGone));
    }

    /// When the wait gives up, on the presentation's clock.
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Counts an answer pass at `now`; whether the wait has now been seen
    /// [`TURNS_AT_ONE_INSTANT`] times at one instant.
    fn count_turn(&mut self, now: Instant) -> bool {
        let seen = match self.turns {
            Some((at, seen)) if at == now => seen.saturating_add(1),
            _ => 1,
        };
        self.turns = Some((now, seen));
        seen >= TURNS_AT_ONE_INSTANT
    }
}

/// What an evaluation of a parked wait may answer.
#[derive(Clone, Copy)]
enum Answering<'a> {
    /// A frame's or a turn's end: only a condition that holds without reading settledness ends
    /// the wait, and only by holding.
    Holds,
    /// The host's list after its turn: any wait may end, by holding, idle, by its deadline or by
    /// the turn bound.
    Settled {
        unsettled: &'a Unsettled,
        scope: SettleScope,
        /// Every cause listed, in any window, is one only the test discharges, and the host's
        /// half is counted.
        idle: bool,
    },
}

impl UiRuntime {
    /// The owner half of `wait.until`, `wait.settled` and `wait.frames`: evaluates a condition
    /// that does not read settledness against what the window has committed, and parks the wait
    /// when it does not hold yet. A condition that reads settledness is parked for the host's list.
    pub(super) fn park_agent_wait(
        &self,
        state: &PresentationState,
        params: WaitParams,
        reply: ReplySender<WaitOutcome>,
    ) {
        let timeout = Duration::from_millis(params.effective_timeout_ms());
        let now = state.clock().now();
        let reads_settled = reads_settled(&params.condition);
        let pending = PendingWait {
            condition: params.condition,
            reads_settled,
            frames: 0,
            events: EventSet::EMPTY,
            deadline: now.checked_add(timeout).unwrap_or(now),
            turns: None,
            reply,
        };
        // A condition that holds already, or a tree that cannot be read, answers here.
        let Some(pending) = self.settle_wait(state, pending, now, Answering::Holds) else {
            return;
        };
        // Waits nobody takes any more hold no place against the limit.
        state
            .agent_waits
            .borrow_mut()
            .retain(PendingWait::is_awaited);
        let parked = state.agent_waits.borrow().len();
        if parked >= MAX_PARKED_WAITS {
            send_reply(&pending.reply, None, Err(AgentError::TooManyWaits));
            return;
        }
        let keeps_frames_coming = counts_frames(&pending.condition);
        state.agent_waits.borrow_mut().push(pending);
        if keeps_frames_coming {
            self.request_redraw_for(state);
        }
    }

    /// Evaluates every parked wait whose condition holds without reading settledness: after the
    /// owner presented a frame (`frame`, which also counts that frame for every wait), and at each
    /// owner wake that ran none. A condition that is pure code and panics answers its own wait
    /// `platform`, and the panic continues once the rest are put back.
    pub(crate) fn evaluate_agent_waits(&self, frame: bool) {
        self.evaluate_parked_waits(frame, Answering::Holds);
    }

    /// Answers every parked wait from the host's list (ADR-0400 §3), each at its presentation's
    /// clock reading: see [`UiRuntime::answer_settled`].
    pub(crate) fn answer_settled_agent_waits(&self, unsettled: &Unsettled, scope: SettleScope) {
        let idle = scope == SettleScope::Complete
            && unsettled
                .causes()
                .all(|cause| cause.cleared_by == ClearedBy::Test);
        self.evaluate_parked_waits(
            false,
            Answering::Settled {
                unsettled,
                scope,
                idle,
            },
        );
    }

    fn evaluate_parked_waits(&self, frame: bool, answering: Answering<'_>) {
        let mut first_panic = FirstPanic::new();
        for state in self.presentations.iter() {
            if state.agent_waits.borrow().is_empty() {
                continue;
            }
            let now = state.clock().now();
            let waits = std::mem::take(&mut *state.agent_waits.borrow_mut());
            let mut keep = Vec::with_capacity(waits.len());
            let mut waits = waits.into_iter();
            while let Some(mut wait) = waits.next() {
                // Nobody takes this outcome any more: let the wait go unanswered.
                if !wait.is_awaited() {
                    continue;
                }
                if frame {
                    wait.frames += 1;
                }
                let reply = wait.reply.clone();
                // A wait's settle runs contained so the reply is still sent; its
                // panic joins `first_panic` after that.
                let mut settle_failure = FirstPanic::new();
                match settle_failure.contain(|| self.settle_wait(state, wait, now, answering)) {
                    Some(Some(wait)) => keep.push(wait),
                    Some(None) => {}
                    None => {
                        // The evaluation consumed the wait; the sender kept apart still
                        // answers it, and the panic continues once the rest are put back.
                        send_reply(&reply, None, Err(AgentError::Malformed));
                        first_panic.absorb(settle_failure);
                        keep.extend(waits);
                        break;
                    }
                }
            }
            // Waits parked while this ran (none can, on the owner) stay behind the old ones.
            let mut parked = state.agent_waits.borrow_mut();
            keep.append(&mut parked);
            *parked = keep;
            let keeps_frames_coming = parked.iter().any(|wait| counts_frames(&wait.condition));
            drop(parked);
            if keeps_frames_coming {
                self.request_redraw_for(state);
            }
        }
        if let Some(payload) = first_panic.into_payload() {
            resume_unwind(payload);
        }
    }

    /// Evaluates `wait` as `answering` allows and answers it when it ends (its condition holds,
    /// the application is idle, its deadline has passed, the turn bound is reached, or its tree
    /// cannot be read); hands it back to stay parked otherwise.
    fn settle_wait(
        &self,
        state: &PresentationState,
        mut wait: PendingWait,
        now: Instant,
        answering: Answering<'_>,
    ) -> Option<PendingWait> {
        if wait.reads_settled && matches!(answering, Answering::Holds) {
            return Some(wait);
        }
        let window = WindowId::from_u64(state.id().as_u64())
            .expect("BUG: a presentation id packs a non-zero generation");
        let in_window = |cause: &&UnsettledCause| cause.window.is_none_or(|named| named == window);
        let settled = match answering {
            Answering::Holds => false,
            Answering::Settled { unsettled, .. } => !unsettled.causes().any(|c| in_window(&c)),
        };
        let facts = WaitFacts {
            frames: wait.frames,
            settled,
            events: wait.events,
        };
        let evaluation = state.pipeline().with(|pipeline| {
            let ratio = pipeline.device_pixel_ratio();
            pipeline.semantics_owner().map(|owner| {
                owner.evaluate_wait(&wait.condition, facts, Placement::new(ratio, window))
            })
        });
        let evaluation = match evaluation {
            // Before the first tree the tree conditions do not hold; only frames, events and
            // settledness can.
            None => None,
            Some(Ok(evaluation)) => Some(evaluation),
            Some(Err(_)) => {
                send_reply(&wait.reply, None, Err(AgentError::Malformed));
                return None;
            }
        };
        let holds = evaluation.as_ref().map_or_else(
            || holds_without_a_tree(&wait.condition, facts, window),
            |evaluation| evaluation.holds,
        );
        let mut outcome = WaitOutcome::default();
        outcome.frames = wait.frames;
        if holds {
            outcome.end = WaitEnd::Satisfied;
            outcome.scope = match answering {
                Answering::Holds => self.settle_scope(),
                Answering::Settled { scope, .. } => scope,
            };
        } else {
            let Answering::Settled {
                unsettled,
                scope,
                idle,
            } = answering
            else {
                return Some(wait);
            };
            let stalled = wait.count_turn(now);
            let (end, causes): (WaitEnd, Vec<UnsettledCause>) = if stalled {
                (
                    WaitEnd::Unsatisfied,
                    unsettled.due_by(now).filter(in_window).cloned().collect(),
                )
            } else if idle {
                (
                    WaitEnd::Idle,
                    unsettled.causes().filter(in_window).cloned().collect(),
                )
            } else if now >= wait.deadline {
                (
                    WaitEnd::Unsatisfied,
                    unsettled.causes().filter(in_window).cloned().collect(),
                )
            } else {
                return Some(wait);
            };
            outcome.end = end;
            outcome.unsettled = causes;
            outcome.scope = scope;
        }
        if let Some(evaluation) = evaluation {
            outcome.observation = evaluation.observation;
        }
        send_reply(&wait.reply, None, Ok(outcome));
        None
    }

    /// The earliest deadline among the parked waits, on the presentations' clocks.
    pub(crate) fn next_agent_wait_deadline(&self) -> Option<Instant> {
        self.presentations
            .iter()
            .filter(|presentation| !presentation.closing_requested.get())
            .filter_map(|presentation| {
                presentation
                    .agent_waits
                    .borrow()
                    .iter()
                    .map(PendingWait::deadline)
                    .min()
            })
            .min()
    }
}

/// Whether `condition` reads settledness anywhere in it.
fn reads_settled(condition: &Condition) -> bool {
    match condition {
        Condition::Settled => true,
        Condition::All(parts) | Condition::Any(parts) => parts.iter().any(reads_settled),
        _ => false,
    }
}

/// Whether `condition` holds with no tree published: only frames, events and settledness can.
fn holds_without_a_tree(condition: &Condition, facts: WaitFacts, window: WindowId) -> bool {
    match condition {
        Condition::Settled => facts.settled,
        Condition::Frames(frames) => facts.frames >= u64::from(*frames),
        Condition::Event(filter) => {
            filter.window.is_none_or(|named| named == window) && facts.events.contains(filter.kind)
        }
        Condition::All(all) => all
            .iter()
            .all(|part| holds_without_a_tree(part, facts, window)),
        Condition::Any(any) => any
            .iter()
            .any(|part| holds_without_a_tree(part, facts, window)),
        _ => false,
    }
}
