//! Immutable transition receipts join the controller's committed delivery FIFO.

use std::rc::Rc;

use super::{AnimationControllerInner, ClockBinding, MotionRunState, RunDelivery};
use crate::observability::{ControllerTrace, observe};
use crate::origin::ControllerOrigin;
use crate::{AnimationStatus, PlaybackRate};

#[derive(Clone, Copy, PartialEq)]
struct State {
    generation: u64,
    active: bool,
    phase: AnimationStatus,
    disposed: bool,
    motion: MotionRunState,
    playback: PlaybackRate,
    clock: ClockBinding,
}

impl State {
    fn capture(inner: &AnimationControllerInner) -> Self {
        Self {
            generation: inner.run_generation,
            active: inner.active_run.is_some(),
            phase: inner.status,
            disposed: inner.disposed,
            motion: inner.motion_state,
            playback: inner.pending_rate.unwrap_or(inner.playback_rate),
            clock: inner.clock_binding,
        }
    }
}

pub(super) struct ControllerDiagnostics {
    trace: Rc<ControllerTrace>,
    previous: Option<State>,
    /// Why the run about to complete settles, noted by the path completing it
    /// and taken by the next transition receipt; `None` means its clock ran out.
    pub(super) settle_reason: Option<&'static str>,
}

impl ControllerDiagnostics {
    pub(super) fn new(origin: &ControllerOrigin) -> Self {
        Self {
            trace: Rc::new(ControllerTrace::new(origin)),
            previous: None,
            settle_reason: None,
        }
    }

    /// The controller span, for parenting its warnings.
    pub(super) fn span(&self) -> Option<tracing::Span> {
        self.trace.span().cloned()
    }

    /// Seed the actual constructed state before any public command can change it.
    pub(super) fn initialize(inner: &mut AnimationControllerInner) {
        inner.diagnostics.previous = Some(State::capture(inner));
    }
}

pub(super) struct Transition {
    trace: Rc<ControllerTrace>,
    state: State,
    previous_generation: u64,
    kind: &'static str,
    outcome: &'static str,
    settle_reason: Option<&'static str>,
    outcome_generation: Option<u64>,
    value: f64,
    target: f64,
    elapsed: f64,
}

impl Transition {
    pub(super) fn capture(
        inner: &mut AnimationControllerInner,
        delivery: Option<&RunDelivery>,
    ) -> Option<Self> {
        let state = State::capture(inner);
        let settle_reason = inner.diagnostics.settle_reason.take();
        let previous = inner.diagnostics.previous.replace(state)?;
        let outcome = delivery.map_or("none", RunDelivery::diagnostic_outcome);
        // A run that completes on its own reached the end of its clock.
        let settle_reason = (outcome == "complete").then(|| settle_reason.unwrap_or("clock"));
        let kind = if state.disposed != previous.disposed {
            "dispose"
        } else if state.generation != previous.generation {
            if previous.active { "replace" } else { "start" }
        } else if outcome == "complete" {
            "complete"
        } else if outcome == "cancelled" {
            "cancel"
        } else if state.motion != previous.motion {
            if state.motion == MotionRunState::Parked {
                "park"
            } else {
                "resume"
            }
        } else if state.playback != previous.playback {
            "playback"
        } else if state.clock != previous.clock {
            "clock"
        } else if state.phase != previous.phase {
            "direction"
        } else {
            return None;
        };
        Some(Self {
            trace: Rc::clone(&inner.diagnostics.trace),
            state,
            previous_generation: previous.generation,
            kind,
            outcome,
            settle_reason,
            outcome_generation: match outcome {
                "cancelled" => Some(previous.generation),
                "complete" => Some(state.generation),
                _ => None,
            },
            value: inner.value,
            target: inner.target_value,
            elapsed: inner.local_elapsed.as_secs_f64(),
        })
    }

    pub(super) fn emit(self) {
        observe(|| {
            let emit = || self.emit_receipt();
            if self
                .trace
                .span()
                .and_then(|span| {
                    span.with_subscriber(|(_, dispatch)| {
                        tracing::dispatcher::with_default(dispatch, emit);
                    })
                })
                .is_none()
            {
                // A disabled controller span has no subscriber-local identity.
                emit();
            }
        });
    }

    fn emit_receipt(&self) {
        if !tracing::enabled!(target: "flui_animation::diagnostics", tracing::Level::DEBUG) {
            return;
        }
        let clock = match self.state.clock {
            ClockBinding::Manual => "manual",
            ClockBinding::Bound => "bound",
            ClockBinding::Missing => "missing",
        };
        let controller = self
            .trace
            .span()
            .and_then(tracing::Span::id)
            .map(|id| id.into_u64());
        let context = tracing::dispatcher::get_default(|dispatch| {
            dispatch
                .current_span()
                .id()
                .map(tracing::span::Id::into_u64)
        });
        tracing::debug!(
            target: "flui_animation::diagnostics", parent: self.trace.span().and_then(tracing::Span::id),
            controller = controller, context_span = context,
            transition = self.kind, run_generation = self.state.generation,
            previous_generation = self.previous_generation,
            outcome = self.outcome, outcome_generation = self.outcome_generation,
            settle_reason = self.settle_reason,
            active = self.state.active, disposed = self.state.disposed,
            parked = self.state.motion == MotionRunState::Parked, clock = clock,
            phase = ?self.state.phase, value = self.value, target = self.target,
            elapsed_seconds = self.elapsed, playback_rate = self.state.playback.get(),
            "animation transition"
        );
    }
}
