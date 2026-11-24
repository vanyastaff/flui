//! The development agent's owner side, as the UI runtime's drain, frames and closes reach it
//! (ADR-0402).
//!
//! Every path a UI runtime runs whether or not an agent serves it (the command drain, a frame's
//! end, an owner turn, platform input, a scene's settlement, a presentation's close) reaches the
//! agent through [`AgentSide`], a reference the runtime holds only once
//! [`UiRuntime::dev_agent_window`] has vended a window. That method is the one place that names
//! [`SERVED`], so a binary that never vends a window (a release build with no hook installed)
//! links none of the agent's owner side: its window methods, the protocol's answers, the
//! semantics reads and the recordings' wire forms. Agent state exists only after a window was
//! vended, so a runtime without the reference has nothing of the agent's to serve or abandon.

use web_time::Instant;

use flui_foundation::PresentationId;
use flui_foundation::panic::FirstPanic;
use flui_platform::PlatformInput;
use flui_protocol::events::EventFilter;
use flui_protocol::record::RecordParams;
use flui_view::__runtime::{Reply, WindowCall};
use flui_view::dev_agent::{AgentEvents, AgentRecording};

use super::agent_inspect::StagedLayerRead;
use super::{DrainReport, UiRuntime};
use crate::presentation::PresentationState;

/// What the UI runtime asks of the development agent at each point its own paths reach it.
///
/// One implementation, [`SERVED`]; the trait is the seam that keeps it out of a binary that
/// never vends a window, not a point of extension.
pub(crate) trait AgentSide: Sync {
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
    /// the waits agents parked.
    fn after_frame(&self, ui_runtime: &UiRuntime);

    /// An owner turn ended without a frame: the waits agents parked see the window as it is.
    fn after_turn(&self, ui_runtime: &UiRuntime);

    /// The frame boundary: input agents injected goes on, one tick.
    fn advance_input(&self, ui_runtime: &UiRuntime, now: Instant);

    /// The earliest deadline of a wait an agent parked on a window that is not closing.
    fn next_wait_deadline(&self, ui_runtime: &UiRuntime) -> Option<Instant>;

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

    /// `presentation` closes: every job an agent parked on it is answered.
    fn abandon_jobs(&self, presentation: &PresentationState);
}

/// The development agent's owner side; named only by [`UiRuntime::dev_agent_window`].
pub(super) static SERVED: Served = Served;

/// See [`AgentSide`].
pub(super) struct Served;

impl AgentSide for Served {
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
        for presentation in ui_runtime.presentations.iter() {
            UiRuntime::complete_agent_reads(presentation);
        }
        ui_runtime.publish_agent_events();
        ui_runtime.evaluate_agent_waits(true);
    }

    fn after_turn(&self, ui_runtime: &UiRuntime) {
        ui_runtime.evaluate_agent_waits(false);
    }

    fn advance_input(&self, ui_runtime: &UiRuntime, now: Instant) {
        ui_runtime.advance_agent_input(now);
    }

    fn next_wait_deadline(&self, ui_runtime: &UiRuntime) -> Option<Instant> {
        ui_runtime.next_agent_wait_deadline()
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

    fn abandon_jobs(&self, presentation: &PresentationState) {
        presentation.abandon_agent_jobs();
    }
}
