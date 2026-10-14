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
use std::num::NonZeroU64;
use std::rc::Rc;

use flui_foundation::{IdMint, InputSequenceId, PresentationId};
use flui_interaction::gesture::{GestureObservation, ObservedMember, ObserverHandle};
use flui_interaction::{GestureDisposition, InputSurface, PointerId};
use flui_platform_api::{EventTime, WindowId};

use super::UiRuntime;
use crate::input_record::{
    DecisionRow, InputRecord, InputRow, PreferencesRow, RecordedMember, RecordedSequence,
    input_time,
};
use crate::owner::InputOutcome;
use crate::{HostInput, InputTaps};

/// One arena decision an observer reported, not yet admitted.
struct Observed {
    window: WindowId,
    pointer: PointerId,
    sequence: Option<InputSequenceId>,
    t: EventTime,
    disposition: GestureDisposition,
    member: RecordedMember,
    member_type: &'static str,
}

/// The numbers a recording gives arena members, in the order it first sees
/// each one. Members are told apart by allocation, never by type; an entry
/// keeps its member's allocation from being reused until the member retired.
#[derive(Default)]
struct MemberNumbers {
    mint: IdMint<NonZeroU64>,
    known: Vec<(ObservedMember, RecordedMember)>,
}

impl MemberNumbers {
    /// `member`'s number, given it now if it has none; `None` once every
    /// number was issued.
    fn number(&mut self, member: &ObservedMember) -> Option<RecordedMember> {
        if let Some((_, number)) = self.known.iter().find(|(known, _)| known == member) {
            return Some(*number);
        }
        let number = RecordedMember::new(self.mint.next().ok()?);
        self.known.push((member.clone(), number));
        Some(number)
    }

    /// Forget retired members: none of them joins a contest again.
    fn forget_retired(&mut self) {
        self.known.retain(|(member, _)| !member.is_retired());
    }
}

/// The observers a recording keeps on each presentation's arena, the
/// decisions they reported since the last admission, and the members' numbers.
#[derive(Default)]
pub(crate) struct DecisionRecorder {
    observed: Rc<RefCell<Vec<Observed>>>,
    members: Rc<RefCell<MemberNumbers>>,
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
            *recorder.members.borrow_mut() = MemberNumbers::default();
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
            let members = Rc::clone(&recorder.members);
            let registration =
                presentation
                    .gestures()
                    .arena()
                    .observe(move |observation: &GestureObservation| {
                        let (contest, member, t, disposition) = match observation {
                            // A member is numbered when it joins, so members
                            // keep the join order whoever decides first.
                            GestureObservation::Joined { member, .. } => {
                                let _ = members.borrow_mut().number(member);
                                return;
                            }
                            GestureObservation::Won {
                                contest,
                                member,
                                at,
                            } => (contest, member, *at, GestureDisposition::Accepted),
                            GestureObservation::Rejected {
                                contest,
                                member,
                                at,
                            } => (contest, member, *at, GestureDisposition::Rejected),
                            _ => return,
                        };
                        let Some(number) = members.borrow_mut().number(member) else {
                            tracing::warn!(
                                target: crate::INPUT_TAP_TARGET,
                                "a recording numbered every arena member it can; this verdict is not recorded"
                            );
                            return;
                        };
                        observed.borrow_mut().push(Observed {
                            window,
                            pointer: contest.pointer(),
                            sequence: contest.sequence(),
                            t,
                            disposition,
                            member: number,
                            member_type: member.type_name().unwrap_or("a retired member"),
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

    /// Admit the applied `input` addressed to `presentation_id`'s window,
    /// outside any sequence, then the decisions it produced, and deliver.
    pub(crate) fn record_applied(
        &self,
        presentation_id: PresentationId,
        input: HostInput,
        outcome: InputOutcome,
    ) {
        self.record_dispatched(presentation_id, InputSurface::Window, input, outcome, None);
    }

    /// Admit the applied `input` addressed to `surface` of
    /// `presentation_id`'s window, which dispatch placed in `sequence`, then
    /// the decisions it produced, and deliver.
    pub(crate) fn record_dispatched(
        &self,
        presentation_id: PresentationId,
        surface: InputSurface,
        input: HostInput,
        outcome: InputOutcome,
        sequence: Option<InputSequenceId>,
    ) {
        if let Some(window) = self
            .presentations
            .get(presentation_id)
            .map(crate::presentation::PresentationState::window_id)
        {
            let arrival = self.event_now();
            let t = input_time(&input).unwrap_or(arrival);
            self.input_taps.admit(InputRecord::Input(
                InputRow::new(window, t, arrival, input, outcome)
                    .on_surface(surface)
                    .in_sequence(sequence.map(RecordedSequence::from)),
            ));
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
    /// each stamped with the time its arena committed it, however much later
    /// the report is admitted.
    pub(crate) fn admit_decisions(&self) {
        let observed = std::mem::take(&mut *self.decision_recorder.observed.borrow_mut());
        for decision in observed {
            self.input_taps.admit(InputRecord::Decision(
                DecisionRow::new(
                    decision.window,
                    decision.pointer,
                    decision.t,
                    decision.disposition,
                    decision.member,
                )
                .in_sequence(decision.sequence.map(RecordedSequence::from))
                .with_member_type(decision.member_type),
            ));
        }
        self.decision_recorder.members.borrow_mut().forget_retired();
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
