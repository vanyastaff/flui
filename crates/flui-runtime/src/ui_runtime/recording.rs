//! Emitting the [`InputRecord`] stream (ADR-0215 §1).
//!
//! Each applied host input is admitted as an [`InputRow`] after its dispatch
//! committed, followed by the arena verdicts that dispatch produced; verdicts
//! made inside a frame (a deadline with no input of its own) follow the frame
//! row once the pump ends. A delivery pass then hands the rows to the tap with
//! no runtime borrow held. Nothing is cloned or kept while no tap is
//! registered.

use flui_foundation::PresentationId;
use flui_platform_api::EventTime;

use super::UiRuntime;
use crate::input_record::{DecisionRow, InputRecord, InputRow, PreferencesRow, input_time};
use crate::owner::InputOutcome;
use crate::{HostInput, InputTaps};

impl UiRuntime {
    /// The tap registry this runtime records to. Records are emitted only
    /// while a tap is registered.
    #[must_use]
    pub fn input_taps(&self) -> &InputTaps {
        &self.input_taps
    }

    /// Prepare to record an input about to be applied: arena verdicts are
    /// kept exactly while a tap listens, and earlier ones are admitted first
    /// so they precede the input. Returns whether the runtime records.
    pub(crate) fn begin_recorded_input(&self) -> bool {
        let recording = self.input_taps.is_recording();
        for presentation in self.presentations.iter() {
            flui_interaction::__runtime::listen_for_decisions(presentation.gestures(), recording);
        }
        if recording {
            self.admit_decisions();
        }
        recording
    }

    /// Admit the applied `input` addressed to `presentation_id`, then the
    /// verdicts it produced, and deliver.
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

    /// Admit the verdicts every presentation's arena delivered since the
    /// last admission.
    pub(crate) fn admit_decisions(&self) {
        if !self.input_taps.is_recording() {
            return;
        }
        for presentation in self.presentations.iter() {
            let window = presentation.window_id();
            for decision in flui_interaction::__runtime::take_decisions(presentation.gestures()) {
                self.input_taps
                    .admit(InputRecord::Decision(DecisionRow::new(
                        window,
                        decision.pointer,
                        decision.time,
                        decision.disposition,
                        decision.member,
                    )));
            }
        }
    }

    /// Admit pending verdicts and run a delivery pass: the end of a pump, and
    /// the owner wake that carries records a tap admitted reentrantly.
    pub(crate) fn deliver_input_records(&self) {
        self.admit_decisions();
        self.input_taps.deliver();
    }

    /// Now, on the event timeline input is stamped on.
    fn event_now(&self) -> EventTime {
        self.presentations.primary().gestures().arena().now()
    }
}
