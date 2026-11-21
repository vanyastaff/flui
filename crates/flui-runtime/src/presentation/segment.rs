//! Per-segment bookkeeping of a presentation: the completion scope, the last
//! segment's span, the tree revision, the segment phase and its probes.

use flui_scheduler::{FramePhaseDurations, PresentationFrameError, PresentationFrameScope};
use flui_view::__runtime::FramePhaseMarker;
use web_time::Instant;

use super::PresentationState;
#[cfg(test)]
use super::{SegmentProbe, SegmentProbeCallback};
use crate::epoch::{FrameCommitState, TreeRevision};
use crate::frame_failure::SegmentPhase;

impl PresentationState {
    pub(crate) fn begin_completion_segment(&self) {
        if let Some(scope) = &self.completion_scope {
            match scope.enter_segment() {
                Ok(()) => {}
                Err(PresentationFrameError::SegmentActive) => scope
                    .resume_segment()
                    .expect("BUG: a retained presentation completion is suspended"),
                Err(error) => panic!("BUG: live presentation cannot enter completion: {error}"),
            }
        }
    }

    pub(crate) fn complete_segment(&self) {
        if let Some(scope) = &self.completion_scope {
            scope
                .complete_segment()
                .expect("BUG: completing a running presentation segment");
        }
    }

    pub(crate) fn service_completion_segment(&self) {
        if let Some(scope) = &self.completion_scope {
            scope
                .service_segment()
                .expect("BUG: servicing a suspended presentation segment");
        }
    }

    pub(crate) fn suspend_completion_segment(&self) {
        if let Some(scope) = &self.completion_scope {
            scope
                .cancel_segment()
                .expect("BUG: incomplete presentation retains its completion");
        }
    }

    pub(crate) fn has_eligible_completion(&self) -> bool {
        !self.clock.is_hidden()
            && self.alive.borrow().is_some()
            && self
                .completion_scope
                .as_ref()
                .is_some_and(PresentationFrameScope::has_completion_demand)
            && self.pipeline.try_with_layout(|_| ()).is_some()
    }

    pub(crate) fn rearm_completion(&self) {
        if self.has_eligible_completion()
            && let Some(scope) = &self.completion_scope
        {
            scope.rearm_completion_demand();
        }
    }

    /// Record this pump's just-completed segment span for THIS
    /// presentation. Called exactly once per pump in which this
    /// presentation's own segment ran, by `UiRuntime::draw_frame_entered`'s
    /// per-presentation loop, immediately after
    /// `UiRuntime::draw_frame_for_presentation` returns. See
    /// [`Self::last_segment_span`]'s field doc for why this lives here and
    /// not on the shared `FrameClock`.
    pub(crate) fn set_last_segment_span(&self, start: Instant, end: Instant) {
        self.last_segment_span.set(Some((start, end)));
    }

    /// Read AND CLEAR the span [`Self::set_last_segment_span`] most
    /// recently recorded for this presentation. `take`, not `get`: called
    /// by `UiRuntime::render_frame_entered` at most once per pump, exactly
    /// when it is about to decide whether to attach a `FrameSnapshot` to
    /// THIS presentation's clock — a pump in which this presentation's own
    /// segment did NOT run must see `None`, never a stale span this same
    /// presentation latched on an earlier pump.
    pub(crate) fn take_last_segment_span(&self) -> Option<(Instant, Instant, FramePhaseDurations)> {
        self.last_segment_span
            .take()
            .map(|(start, end)| (start, end, self.segment_phases.get()))
    }

    /// Record how long this presentation's segment spent building, laying
    /// out and painting so far; [`Self::take_last_segment_span`] reports it.
    pub(crate) fn record_segment_phases(&self, phases: FramePhaseDurations) {
        self.segment_phases.set(phases);
    }

    /// The latest terminal frame result's revision.
    pub(crate) fn tree_revision(&self) -> TreeRevision {
        self.tree_revision.get()
    }

    /// Advance after one terminal frame result (`Painted` or `Errored`).
    pub(crate) fn advance_tree_revision(&self) -> TreeRevision {
        let tree_revision = self.tree_revision.get().next();
        self.tree_revision.set(tree_revision);
        tree_revision
    }

    /// Acknowledge every terminal tree revision through `through`, the revision a frame the sink
    /// accepted was painted from. A frame that settles after later ones were painted commits only
    /// its own revision, and an acknowledgement never moves back.
    ///
    /// This method is the single commit point where input replay attaches:
    /// callers invoke it only after a painted frame receives a successful
    /// submit classification.
    pub(crate) fn commit_tree_revision(&self, through: TreeRevision) -> TreeRevision {
        debug_assert!(
            through <= self.tree_revision.get(),
            "BUG: a frame commits a revision the tree has reached"
        );
        let committed_revision = through.max(self.presented_revision.get());
        self.presented_revision.set(committed_revision);
        committed_revision
    }

    /// Whether the current terminal tree state has been acknowledged.
    #[must_use]
    pub(crate) fn frame_commit_state(&self) -> FrameCommitState {
        let tree_revision = self.tree_revision.get();
        let presented_revision = self.presented_revision.get();
        assert!(
            presented_revision <= tree_revision,
            "BUG: presented tree revision exceeds terminal tree revision"
        );
        if presented_revision == tree_revision
            && !self.text_preparation.borrow().holds_document_barrier()
            && self.text_preparation.borrow().segment.is_none()
        {
            FrameCommitState::Committed
        } else {
            FrameCommitState::Uncommitted {
                since: presented_revision.next(),
            }
        }
    }

    /// Install (or clear) the segment fault-injection probe. See
    /// [`Self::segment_probe`]'s field doc.
    #[cfg(test)]
    pub(crate) fn set_segment_probe(
        &self,
        phase: SegmentPhase,
        probe: Option<SegmentProbeCallback>,
    ) {
        let _prev = std::mem::replace(
            &mut *self.segment_probe.borrow_mut(),
            probe.map(|callback| SegmentProbe { phase, callback }),
        );
    }

    /// Arm the data-only one-shot fault at the build-to-finalize boundary.
    #[cfg(test)]
    pub(crate) fn arm_finalize_phase_panic(&self) {
        self.segment_phase.arm_test_panic_once();
    }

    /// Enter a frame segment, then run its installed test probe, if any.
    ///
    /// The phase write deliberately precedes the probe and has no restoring
    /// guard: if either the probe or segment work unwinds, the last-entered
    /// phase remains available to the presentation-level catch.
    pub(crate) fn enter_segment_phase(&self, phase: SegmentPhase) {
        self.segment_phase.set(phase);
        #[cfg(test)]
        if let Some(probe) = self.segment_probe.borrow().as_ref()
            && probe.phase == phase
        {
            (probe.callback)(self);
        }
    }

    /// Last frame segment entered by this presentation.
    #[must_use]
    pub(crate) fn segment_phase(&self) -> SegmentPhase {
        self.segment_phase.get()
    }

    /// Data-only marker passed to the widget binding at the exact
    /// build-to-finalize boundary.
    #[must_use]
    pub(crate) fn segment_phase_marker(&self) -> &FramePhaseMarker<SegmentPhase> {
        &self.segment_phase
    }

    /// Record that this presentation's build+layout+paint segment ran. See
    /// [`Self::flush_count`]'s field doc for the oracle this backs.
    #[cfg(test)]
    pub(crate) fn record_flush(&self) {
        self.flush_count.set(self.flush_count.get() + 1);
    }

    /// This presentation's own flush count. Test-only oracle.
    #[cfg(test)]
    pub(crate) fn flush_count(&self) -> u32 {
        self.flush_count.get()
    }
}
