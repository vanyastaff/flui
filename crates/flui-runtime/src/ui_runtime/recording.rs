//! Emitting the [`InputRecord`] stream (ADR-0215 §1).
//!
//! Each applied host input is admitted as an [`InputRow`] after its dispatch
//! committed, followed by the arena decisions that dispatch produced, which a
//! [`GestureObserver`](flui_interaction::gesture::GestureObserver) on each
//! presentation's arena reports; decisions made inside a frame (a deadline
//! with no input of its own) follow the frame row once the pump ends. A
//! delivery pass then hands the rows to the tap with no runtime borrow held.
//! Nothing is cloned, kept or observed while no tap is registered.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::PresentationId;
use flui_interaction::gesture::{GestureObservation, ObserverHandle};
use flui_interaction::{GestureDisposition, PointerId};
use flui_platform_api::{EventTime, WindowId};

use super::UiRuntime;
use crate::input_record::{DecisionRow, InputRecord, InputRow, PreferencesRow, input_time};
use crate::owner::InputOutcome;
use crate::{HostInput, InputTaps};

/// One arena decision an observer reported, not yet admitted.
struct Observed {
    window: WindowId,
    pointer: PointerId,
    disposition: GestureDisposition,
    member: &'static str,
}

/// The observers a recording keeps on each presentation's arena, and the
/// decisions they reported since the last admission.
#[derive(Default)]
pub(crate) struct DecisionRecorder {
    observed: Rc<RefCell<Vec<Observed>>>,
    observers: RefCell<Vec<(PresentationId, ObserverHandle)>>,
}

impl UiRuntime {
    /// The tap registry this runtime records to. Records are emitted only
    /// while a tap is registered.
    #[must_use]
    pub fn input_taps(&self) -> &InputTaps {
        &self.input_taps
    }

    /// Prepare to record an input about to be applied: every presentation's
    /// arena is observed exactly while a tap is registered, and decisions
    /// reported earlier are admitted first, so they precede the input.
    /// Returns whether the runtime records.
    pub(crate) fn begin_recorded_input(&self) -> bool {
        let recording = self.input_taps.is_recording();
        self.observe_decisions(recording);
        if recording {
            self.admit_decisions();
        }
        recording
    }

    /// Observe each live presentation's arena while `recording`, and no
    /// arena otherwise.
    fn observe_decisions(&self, recording: bool) {
        let recorder = &self.decision_recorder;
        let outgoing: Vec<(PresentationId, ObserverHandle)> = {
            let mut observers = recorder.observers.borrow_mut();
            let (kept, outgoing) = std::mem::take(&mut *observers)
                .into_iter()
                .partition(|(id, _)| recording && self.presentations.get(*id).is_some());
            *observers = kept;
            outgoing
        };
        // Unregistering runs no observer: the borrow is already released.
        drop(outgoing);
        if !recording {
            recorder.observed.borrow_mut().clear();
            return;
        }
        for presentation in self.presentations.iter() {
            let id = presentation.id();
            if recorder
                .observers
                .borrow()
                .iter()
                .any(|(observed, _)| *observed == id)
            {
                continue;
            }
            let window = presentation.window_id();
            let observed = Rc::clone(&recorder.observed);
            let registration =
                presentation
                    .gestures()
                    .arena()
                    .observe(move |observation: &GestureObservation| {
                        let (contest, member, disposition) = match observation {
                            GestureObservation::Won { contest, member } => {
                                (contest, member, GestureDisposition::Accepted)
                            }
                            GestureObservation::Rejected { contest, member } => {
                                (contest, member, GestureDisposition::Rejected)
                            }
                            _ => return,
                        };
                        observed.borrow_mut().push(Observed {
                            window,
                            pointer: contest.pointer(),
                            disposition,
                            member: member.type_name().unwrap_or("a retired member"),
                        });
                    });
            match registration {
                Ok(handle) => recorder.observers.borrow_mut().push((id, handle)),
                Err(error) => tracing::warn!(
                    target: crate::INPUT_TAP_TARGET,
                    %error,
                    "a recording cannot observe this presentation's arena decisions"
                ),
            }
        }
    }

    /// Admit the applied `input` addressed to `presentation_id`, then the
    /// decisions it produced, and deliver.
    pub(crate) fn record_applied(
        &self,
        presentation_id: PresentationId,
        input: HostInput,
        outcome: InputOutcome,
    ) {
        if let Some(window) = self
            .presentations
            .get(presentation_id)
            .map(crate::presentation::PresentationState::window_id)
        {
            let arrival = self.event_now();
            let t = input_time(&input).unwrap_or(arrival);
            self.input_taps.admit(InputRecord::Input(InputRow::new(
                window, t, arrival, input, outcome,
            )));
        }
        self.admit_decisions();
        self.input_taps.deliver();
    }

    /// Admit an applied system-preference revision and deliver.
    pub(crate) fn record_preferences(
        &self,
        revision: u64,
        values: std::sync::Arc<flui_platform_api::SystemPreferences>,
    ) {
        self.input_taps
            .admit(InputRecord::Preferences(PreferencesRow::new(
                revision, values,
            )));
        self.input_taps.deliver();
    }

    /// Admit the decisions the observers reported since the last admission,
    /// stamped now: a decision is reported after the operation that made it,
    /// within the same owner turn.
    pub(crate) fn admit_decisions(&self) {
        let observed = std::mem::take(&mut *self.decision_recorder.observed.borrow_mut());
        if observed.is_empty() {
            return;
        }
        let t = self.event_now();
        for decision in observed {
            self.input_taps
                .admit(InputRecord::Decision(DecisionRow::new(
                    decision.window,
                    decision.pointer,
                    t,
                    decision.disposition,
                    decision.member,
                )));
        }
    }

    /// Admit pending decisions and run a delivery pass: the end of a pump,
    /// and the owner wake that carries records a tap admitted reentrantly.
    pub(crate) fn deliver_input_records(&self) {
        self.admit_decisions();
        self.input_taps.deliver();
    }

    /// Now, on the event timeline input is stamped on.
    fn event_now(&self) -> EventTime {
        self.presentations.primary().gestures().arena().now()
    }
}
