//! An agent's waits (ADR-0238 §7, R18): conditions the owner holds out for, evaluated after
//! frames and never polled.
//!
//! A wait is parked on its presentation. It is evaluated when it arrives (a condition that
//! already holds answers at once), after each frame the owner presents, and when its deadline
//! passes. The deadline is on the presentation's own clock, so a manual clock expires it
//! deterministically, and it joins [`UiRuntime::next_wake`] so a window that draws nothing still
//! times out. A timeout is an answer (`satisfied: false` with the last observation and why the
//! window had not settled), not an error.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::time::Duration;

use flui_foundation::geometry::DevicePixelRatio;
use flui_protocol::{
    Condition, UnsettledCause, UnsettledReason, WaitOutcome, WaitParams, WindowId,
};
use flui_semantics::{Placement, WaitFacts, counts_frames, waits_on_events};
use web_time::Instant;

use super::UiRuntime;
use super::agent::{AgentError, ReplySender, send_reply};
use crate::presentation::PresentationState;

/// How long a wait holds out when the caller names no timeout.
const DEFAULT_WAIT: Duration = Duration::from_millis(5_000);

/// The longest a wait may hold out; a longer timeout is this long.
const MAX_WAIT: Duration = Duration::from_hours(24);

/// The most waits one presentation holds at once. A wait past it is refused with
/// [`AgentError::TooManyWaits`], never dropped silently or in place of an older one.
pub(crate) const MAX_PARKED_WAITS: usize = 256;

/// A wait, parked on its presentation until its condition holds or its deadline passes.
/// Owner thread only.
pub(crate) struct PendingWait {
    condition: Condition,
    /// Frames the owner presented since the wait began.
    frames: u64,
    /// On the presentation's clock.
    deadline: Instant,
    reply: ReplySender<WaitOutcome>,
}

impl PendingWait {
    /// The window closed before the wait ended.
    pub(crate) fn abandon(self) {
        send_reply(&self.reply, None, Err(AgentError::PresentationGone));
    }

    /// When the wait gives up, on the presentation's clock.
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
}

impl UiRuntime {
    /// The owner half of [`SemanticsAgent::wait`](super::SemanticsAgent::wait): evaluates the
    /// condition against what the window has committed, and parks the wait when it does not hold
    /// yet. Returns whether the presentation was still open.
    pub(super) fn park_agent_wait(
        &self,
        presentation: flui_foundation::PresentationId,
        params: WaitParams,
        reply: ReplySender<WaitOutcome>,
    ) -> bool {
        let Some(state) = self.presentations.get(presentation) else {
            send_reply(&reply, None, Err(AgentError::PresentationGone));
            return false;
        };
        if waits_on_events(&params.condition) {
            send_reply(
                &reply,
                None,
                Err(AgentError::InvalidArgument {
                    reason: "a wait on events needs the event stream",
                }),
            );
            return true;
        }
        let timeout = params
            .timeout_ms
            .map_or(DEFAULT_WAIT, |ms| Duration::from_millis(ms).min(MAX_WAIT));
        let now = state.clock().now();
        let pending = PendingWait {
            condition: params.condition,
            frames: 0,
            deadline: now.checked_add(timeout).unwrap_or(now),
            reply,
        };
        // A condition that holds already, or a tree that cannot be read, answers here.
        let Some(pending) = self.settle_wait(state, pending, now) else {
            return true;
        };
        let parked = state.agent_waits.borrow().len();
        if parked >= MAX_PARKED_WAITS {
            send_reply(&pending.reply, None, Err(AgentError::TooManyWaits));
            return true;
        }
        let keeps_frames_coming = counts_frames(&pending.condition);
        state.agent_waits.borrow_mut().push(pending);
        if keeps_frames_coming {
            self.request_redraw_for(state);
        }
        true
    }

    /// Evaluates every parked wait: after the owner presented a frame (`frame`, which also counts
    /// that frame for `Frames`), and at each owner wake that ran none, so a deadline or a timer
    /// that ran out is seen. A condition that is pure code and panics answers its own wait
    /// `platform`, and the panic continues once the rest are put back.
    pub(crate) fn evaluate_agent_waits(&self, frame: bool) {
        let mut first_panic = None;
        for state in self.presentations.iter() {
            if state.agent_waits.borrow().is_empty() {
                continue;
            }
            let now = state.clock().now();
            let waits = std::mem::take(&mut *state.agent_waits.borrow_mut());
            let mut keep = Vec::with_capacity(waits.len());
            let mut waits = waits.into_iter();
            while let Some(mut wait) = waits.next() {
                if frame {
                    wait.frames += 1;
                }
                let reply = wait.reply.clone();
                let settled = catch_unwind(AssertUnwindSafe(|| self.settle_wait(state, wait, now)));
                match settled {
                    Ok(Some(wait)) => keep.push(wait),
                    Ok(None) => {}
                    Err(payload) => {
                        // The evaluation consumed the wait; the sender kept apart still
                        // answers it, and the panic continues once the rest are put back.
                        send_reply(&reply, None, Err(AgentError::Malformed));
                        first_panic.get_or_insert(payload);
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
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
    }

    /// Evaluates `wait` and answers it when its condition holds, its deadline has passed
    /// (`expire`) or its tree cannot be read; hands it back to stay parked otherwise.
    fn settle_wait(
        &self,
        state: &PresentationState,
        wait: PendingWait,
        now: Instant,
    ) -> Option<PendingWait> {
        let window = WindowId::from_u64(state.id().as_u64())
            .expect("BUG: a presentation id packs a non-zero generation");
        let causes = self.unsettled_causes(now);
        let facts = WaitFacts {
            frames: wait.frames,
            settled: causes.is_empty(),
        };
        let evaluation = state.pipeline().with(|pipeline| {
            let ratio = DevicePixelRatio::new(pipeline.device_pixel_ratio())
                .unwrap_or(DevicePixelRatio::ONE);
            pipeline.semantics_owner().map(|owner| {
                owner.evaluate_wait(&wait.condition, facts, Placement::new(ratio, window))
            })
        });
        let evaluation = match evaluation {
            // Before the first tree the tree conditions do not hold; only frames and
            // settledness can.
            None => None,
            Some(Ok(evaluation)) => Some(evaluation),
            Some(Err(_)) => {
                send_reply(&wait.reply, None, Err(AgentError::Malformed));
                return None;
            }
        };
        let holds = evaluation.as_ref().map_or_else(
            || holds_without_a_tree(&wait.condition, facts),
            |evaluation| evaluation.holds,
        );
        let due = now >= wait.deadline;
        if !holds && !due {
            return Some(wait);
        }
        let mut outcome = WaitOutcome::default();
        outcome.satisfied = holds;
        outcome.frames = wait.frames;
        if let Some(evaluation) = evaluation {
            outcome.observation = evaluation.observation;
        }
        if !holds {
            outcome.unsettled = causes;
        }
        send_reply(&wait.reply, None, Ok(outcome));
        None
    }

    /// Why the owner is not idle, from the runtime's own predicates: what
    /// `HeadlessHost::pump_until_settled` reads. Agent waits are left out, so a wait never keeps
    /// the window from settling for another.
    pub(crate) fn unsettled_causes(&self, now: Instant) -> Vec<UnsettledCause> {
        let mut causes = Vec::new();
        if self.needs_redraw() {
            causes.push(UnsettledCause::new(UnsettledReason::NeedsRedraw));
        }
        if self.scheduler().is_frame_scheduled() {
            causes.push(UnsettledCause::new(UnsettledReason::FrameScheduled));
        }
        if self.has_pending_work() {
            causes.push(UnsettledCause::new(UnsettledReason::PendingWork));
        }
        let callbacks = self.scheduler().transient_callback_count();
        if callbacks > 0 {
            let mut cause = UnsettledCause::new(UnsettledReason::TransientCallbacks);
            cause.count = Some(u32::try_from(callbacks).unwrap_or(u32::MAX));
            causes.push(cause);
        }
        if let Some(due) = self.next_wake_excluding_agent_waits() {
            let mut cause = UnsettledCause::new(UnsettledReason::Timer);
            let remaining = due.saturating_duration_since(now);
            cause.due_in_ms = Some(u64::try_from(remaining.as_millis()).unwrap_or(u64::MAX));
            causes.push(cause);
        }
        causes
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

/// Whether `condition` holds with no tree published: only frames and settledness can.
fn holds_without_a_tree(condition: &Condition, facts: WaitFacts) -> bool {
    match condition {
        Condition::Settled => facts.settled,
        Condition::Frames(frames) => facts.frames >= u64::from(*frames),
        Condition::All(all) => all.iter().all(|part| holds_without_a_tree(part, facts)),
        Condition::Any(any) => any.iter().any(|part| holds_without_a_tree(part, facts)),
        _ => false,
    }
}
