//! The development agent's owner side, as the UI runtime's drain, frames and closes reach it
//! (ADR-0402).
//!
//! Every path a UI runtime runs whether or not an agent serves it (the command drain, a frame's
//! end, an owner turn, platform input, a scene's settlement, a presentation's close and the
//! runtime's own drop) reaches the agent through [`AgentSide`], a reference the runtime holds
//! only once [`UiRuntime::dev_agent_window`] has vended a window. That method is the one place
//! that names [`AGENT_OWNER_SIDE`], so a binary that never vends a window (a release build with no
//! hook installed) links none of the agent's owner side: its window methods, the protocol's
//! answers, the semantics reads and the recordings' wire forms. Agent state exists only after a
//! window was vended, so a runtime without the reference has nothing of the agent's to serve or
//! abandon; what such a binary keeps is the drop glue of the agent's state fields.
//!
//! Until a presentation's first window is vended no frame reports to its agent, and once
//! collection stops its tree may be a new owner's: a vend that starts collection with no subscriber
//! listening brings its event hub up to the frames and the tree already shown, so a subscriber
//! hears only what comes after it.

use web_time::Instant;

use flui_foundation::PresentationId;
use flui_foundation::panic::FirstPanic;
use flui_platform::PlatformInput;
use flui_protocol::events::EventFilter;
use flui_protocol::record::RecordParams;
use flui_protocol::wait::SettleScope;
use flui_view::__runtime::{Reply, WindowCall};
use flui_view::dev_agent::{AgentEvents, AgentRecording};

use super::agent_inspect::StagedLayerRead;
use super::{DrainReport, UiRuntime};
use crate::presentation::PresentationState;
use crate::settledness::Unsettled;

/// What the UI runtime asks of the development agent at each point its own paths reach it.
///
/// One implementation, [`AGENT_OWNER_SIDE`]; the trait is the seam that keeps it out of a binary
/// that never vends a window, not a point of extension.
pub(crate) trait AgentSide {
    /// The leases each window's agent let go since the last drain, counted before the drain
    /// reads its commands.
    fn count_released_leases(&self, ui_runtime: &UiRuntime) -> Vec<(PresentationId, usize)>;

    /// Release the input of the leases counted at the drain's start, then end the recordings
    /// their readers stopped or let go.
    fn after_drain(
        &self,
        ui_runtime: &UiRuntime,
        released: Vec<(PresentationId, usize)>,
    ) -> FirstPanic;

    /// Run one window method an agent sent.
    fn serve_window_call(
        &self,
        ui_runtime: &UiRuntime,
        presentation: PresentationId,
        call: WindowCall,
        report: &mut DrainReport,
    );

    /// Register an event subscription an agent sent and answer with its stream.
    fn subscribe(
        &self,
        ui_runtime: &UiRuntime,
        presentation: PresentationId,
        filters: Vec<EventFilter>,
        reply: Reply<AgentEvents>,
        report: &mut DrainReport,
    );

    /// Start a recording an agent asked for and answer with it.
    fn record(
        &self,
        ui_runtime: &UiRuntime,
        presentation: PresentationId,
        params: &RecordParams,
        reply: &Reply<AgentRecording>,
    );

    /// A frame has passed: the readings agents' actions owe, the events the frame produced and
    /// the waits agents parked, counted for that frame, those that hold without reading
    /// settledness evaluated.
    ///
    /// A running perf span takes the frames presented since its last pull.
    fn after_frame(&self, ui_runtime: &UiRuntime);

    /// An owner turn ended without a frame: the acts, their queued input and the readings they owe
    /// whose deadline passed are answered, and the waits agents parked that hold without reading
    /// settledness see the window as it is.
    fn after_turn(&self, ui_runtime: &UiRuntime);

    /// The frame boundary: input agents injected goes on, one tick.
    fn advance_input(&self, ui_runtime: &UiRuntime, now: Instant);

    /// The frame boundary, before injected input goes on: the acts agents hold are attempted
    /// against the frame just committed, and one whose target passed queues its input.
    fn advance_held_acts(&self, ui_runtime: &UiRuntime);

    /// The earliest deadline of an act an agent holds, of the input an act queued, or of the
    /// reading an act owes, on a window that is not closing.
    fn next_act_deadline(&self, ui_runtime: &UiRuntime) -> Option<Instant>;

    /// The earliest deadline of a wait an agent parked on a window that is not closing.
    fn next_wait_deadline(&self, ui_runtime: &UiRuntime) -> Option<Instant>;

    /// The host's list after its turn, counting `scope`'s halves: the waits agents parked whose
    /// answer reads settledness are answered from it (ADR-0400 §3).
    fn answer_settled(&self, ui_runtime: &UiRuntime, unsettled: &Unsettled, scope: SettleScope);

    /// Whether a wait an agent parked is still unanswered, on any window.
    fn awaits_settled_answer(&self, ui_runtime: &UiRuntime) -> bool;

    /// A host that composes no half: the waits agents parked whose answer reads settledness are
    /// answered from the runtime's half at `now` alone (`SettleScope::RuntimeOnly`).
    fn answer_from_runtime_half(&self, ui_runtime: &UiRuntime, now: Instant);

    /// Platform input reached `presentation`: its agent's journal records it.
    fn journal_input(
        &self,
        ui_runtime: &UiRuntime,
        presentation: &PresentationState,
        input: &PlatformInput,
        injected: bool,
    );

    /// `presentation` left the runtime: the recordings of it end.
    fn end_recordings_of(&self, ui_runtime: &UiRuntime, presentation: PresentationId)
    -> FirstPanic;

    /// A frame of `producer` is captured for `scene`: the layer reads parked for it are read.
    fn stage_layer_reads(
        &self,
        producer: &PresentationState,
        scene: &flui_layer::SceneVersion,
    ) -> Vec<StagedLayerRead>;

    /// A captured frame was never shown: its layer reads wait for the next scene.
    fn restore_layer_reads(&self, producer: &PresentationState, staged: Vec<StagedLayerRead>);

    /// A captured frame's verdict is known: its layer reads are answered if the window showed
    /// it, and parked again if it did not.
    fn settle_layer_reads(
        &self,
        producer: &PresentationState,
        staged: Vec<StagedLayerRead>,
        shown: bool,
    ) -> FirstPanic;

    /// `presentation` closes: its window handles stop admitting calls.
    fn withdraw_window(&self, presentation: &PresentationState);

    /// `presentation` closes: every job an agent parked on it is answered, each delivery
    /// contained; the first panic of one is returned for the close to hold.
    fn abandon_jobs(&self, presentation: &PresentationState) -> FirstPanic;

    /// The runtime goes: every recording of it ends with its `end` row.
    fn end_all_recordings(&self, ui_runtime: &UiRuntime) -> FirstPanic;
}

/// The development agent's owner side; named only by [`UiRuntime::dev_agent_window`].
pub(super) static AGENT_OWNER_SIDE: AgentOwnerSide = AgentOwnerSide;

/// See [`AgentSide`].
pub(super) struct AgentOwnerSide;

impl AgentSide for AgentOwnerSide {
    fn count_released_leases(&self, ui_runtime: &UiRuntime) -> Vec<(PresentationId, usize)> {
        ui_runtime.count_released_leases()
    }

    fn after_drain(
        &self,
        ui_runtime: &UiRuntime,
        released: Vec<(PresentationId, usize)>,
    ) -> FirstPanic {
        let mut first = ui_runtime.release_counted_leases(released);
        first.absorb(ui_runtime.sweep_agent_recordings());
        first
    }

    fn serve_window_call(
        &self,
        ui_runtime: &UiRuntime,
        presentation: PresentationId,
        call: WindowCall,
        report: &mut DrainReport,
    ) {
        ui_runtime.serve_window_call(presentation, call, report);
    }

    fn subscribe(
        &self,
        ui_runtime: &UiRuntime,
        presentation: PresentationId,
        filters: Vec<EventFilter>,
        reply: Reply<AgentEvents>,
        report: &mut DrainReport,
    ) {
        ui_runtime.serve_subscription(presentation, filters, reply, report);
    }

    fn record(
        &self,
        ui_runtime: &UiRuntime,
        presentation: PresentationId,
        params: &RecordParams,
        reply: &Reply<AgentRecording>,
    ) {
        ui_runtime.start_agent_recording(presentation, params, reply);
    }

    fn after_frame(&self, ui_runtime: &UiRuntime) {
        // No user code runs in sampling: it goes before the replies below, whose wakers do.
        ui_runtime.sample_agent_perf();
        for presentation in ui_runtime.presentations.iter() {
            UiRuntime::complete_agent_reads(presentation);
        }
        ui_runtime.publish_agent_events();
        ui_runtime.evaluate_agent_waits(true);
    }

    fn after_turn(&self, ui_runtime: &UiRuntime) {
        // A held act whose deadline passed is answered on a turn without a frame too, and so are
        // the input an act queued that no frame started by its deadline and the reading an act
        // owes that no frame came for. The waits are evaluated whichever panics, and the first
        // panic continues after all four.
        let mut first = FirstPanic::new();
        first.contain(|| ui_runtime.advance_held_acts(super::agent_held::Pass::Expired));
        first.contain(|| ui_runtime.expire_act_input());
        first.contain(|| ui_runtime.expire_act_reads());
        first.contain(|| ui_runtime.evaluate_agent_waits(false));
        first.resume();
    }

    fn advance_input(&self, ui_runtime: &UiRuntime, now: Instant) {
        ui_runtime.advance_agent_input(now);
    }

    fn advance_held_acts(&self, ui_runtime: &UiRuntime) {
        ui_runtime.advance_held_acts(super::agent_held::Pass::Boundary);
    }

    fn next_act_deadline(&self, ui_runtime: &UiRuntime) -> Option<Instant> {
        ui_runtime.next_held_act_deadline()
    }

    fn next_wait_deadline(&self, ui_runtime: &UiRuntime) -> Option<Instant> {
        ui_runtime.next_agent_wait_deadline()
    }

    fn answer_settled(&self, ui_runtime: &UiRuntime, unsettled: &Unsettled, scope: SettleScope) {
        ui_runtime.answer_settled_agent_waits(unsettled, scope);
    }

    fn awaits_settled_answer(&self, ui_runtime: &UiRuntime) -> bool {
        ui_runtime
            .presentations
            .iter()
            .any(|presentation| !presentation.agent_waits.borrow().is_empty())
    }

    fn answer_from_runtime_half(&self, ui_runtime: &UiRuntime, now: Instant) {
        ui_runtime.answer_settled_agent_waits_from_own_half(now);
    }

    fn journal_input(
        &self,
        ui_runtime: &UiRuntime,
        presentation: &PresentationState,
        input: &PlatformInput,
        injected: bool,
    ) {
        ui_runtime.journal_input(presentation, input, injected);
    }

    fn end_recordings_of(
        &self,
        ui_runtime: &UiRuntime,
        presentation: PresentationId,
    ) -> FirstPanic {
        ui_runtime.end_agent_recordings_of(presentation)
    }

    fn stage_layer_reads(
        &self,
        producer: &PresentationState,
        scene: &flui_layer::SceneVersion,
    ) -> Vec<StagedLayerRead> {
        producer.stage_agent_layer_reads(scene)
    }

    fn restore_layer_reads(&self, producer: &PresentationState, staged: Vec<StagedLayerRead>) {
        producer.restore_staged_agent_layer_reads(staged);
    }

    fn settle_layer_reads(
        &self,
        producer: &PresentationState,
        staged: Vec<StagedLayerRead>,
        shown: bool,
    ) -> FirstPanic {
        producer.settle_agent_layer_reads(staged, shown)
    }

    fn withdraw_window(&self, presentation: &PresentationState) {
        if let Some(slot) = &*presentation.dev_agent.borrow() {
            slot.withdraw();
        }
    }

    fn abandon_jobs(&self, presentation: &PresentationState) -> FirstPanic {
        presentation.abandon_agent_jobs()
    }

    fn end_all_recordings(&self, ui_runtime: &UiRuntime) -> FirstPanic {
        ui_runtime.agent_recordings.end_all()
    }
}
