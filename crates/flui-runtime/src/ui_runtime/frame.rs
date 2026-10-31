//! Frame production: draw, paint, commit, render and frame telemetry.

use super::input::preserve_first_input_panic;
use super::{EpochDisposition, FramePaintOutcome, MAX_NOT_SHOWN_RETRIES, UiRuntime};
use crate::epoch::FrameCommitState;
use crate::frame_failure::{FrameFailureKind, SegmentPhase};
use crate::held_input::HeldPointerReplay;
use crate::presentation::PresentationState;
use flui_foundation::PresentationId;
use flui_foundation::geometry::Size;
use flui_layer::Scene;
use flui_rendering::binding::{RendererBinding as _, ViewHitTest};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_scheduler::{
    DemandKind, FramePhase, FramePhaseDurations, FrameSnapshot, FrameTimes, Instant, PresentOutcome,
};
use flui_view::__runtime::BindingRuntime as _;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

impl UiRuntime {
    // ========================================================================
    // Frame production (moved from the retired `AppBinding`)
    // ========================================================================

    /// Draw a frame and return the produced `Scene`, if any. Test-only —
    /// production drives frames through [`Self::pump`].
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Option<Scene> {
        match self
            .enter(|ui_runtime| ui_runtime.draw_frame_entered(constraints))
            .1
        {
            // (the third tuple element, `any_retry_needed`, is a retry-arming
            // concern for `render_frame`; this test helper only
            // reports what was painted)
            FramePaintOutcome::Painted(scene) => Some(scene),
            FramePaintOutcome::Idle | FramePaintOutcome::Errored { .. } => None,
        }
    }

    /// The draw step alone (the crate-private `render_frame`), for a test outside this
    /// crate that pins the submit classification against a scripted backend.
    /// Test-only — production drives frames through [`Self::pump`].
    #[cfg(any(test, feature = "testing"))]
    pub fn render_frame_for_test<S: crate::sink::FrameSink + ?Sized>(&self, sink: &mut S) -> bool {
        self.render_frame(sink)
    }

    /// The complete build+layout+paint pipeline for one frame.
    ///
    /// Sample every presentation at the pump's selected time before any build.
    /// Each operation owns provenance, one frame-clock admission decision and
    /// recovery. A delivery failure preserves committed state; a computation
    /// or unknown blocker rejects only its admitted attempt (ADR-0199).
    /// Reports seal those decisions before diagnostics. Revalidate authority
    /// and retained premises immediately before each segment; a newly failing
    /// validation is a separate preparation operation.
    ///
    /// Process eligible segments in mount order and return the last producer
    /// and outcome alongside aggregate retry debt. The single constraints set
    /// and sink do not provide simultaneous multi-surface submission.
    pub(crate) fn draw_frame_entered(
        &self,
        primary_constraints: BoxConstraints,
    ) -> (PresentationId, FramePaintOutcome, bool) {
        // Sampling may dirty any presentation. Finish every operation and
        // establish recovery before the build walk.
        let mut sampling = self.sample_presentations(primary_constraints);

        // OwnerFrame's complete frame operation polls async work once in
        // the mid-frame slot, before this persistent pipeline. The pipeline
        // cannot independently poll tasks and re-enter frame capabilities.

        let mut any_retry_needed = sampling
            .iter()
            .any(super::sampling::PresentationSampling::rejected);
        let mut last_outcome = if any_retry_needed {
            FramePaintOutcome::Errored { retry: true }
        } else {
            FramePaintOutcome::Idle
        };
        let mut producer = self.presentations.primary().id();
        // Whether ANY presentation needs a failure retry THIS pump — returned separately from
        // `last_outcome`, which only ever describes the LAST segment that
        // ran: with more than one presentation mounted, an earlier
        // presentation's failure followed by a sibling's clean `Painted`
        // must still arm the caller's retry instead of letting the pump
        // settle as rendered (`mark_rendered()` would clear the wake the
        // failure needs).
        for operation in &mut sampling {
            let presentation = operation.presentation();
            let constraints = self.root_constraints(presentation, primary_constraints);
            // Another presentation's animation/input callback may have changed
            // this retained attempt after its earlier pre-phase reconciliation.
            // Do not publish obsolete geometry or combine a retained old tick
            // with the new inputs: sample a fresh replacement next pump.
            if operation.reconcile_for_segment(self, constraints) {
                presentation.clock().mark_demand(DemandKind::Dirty);
                any_retry_needed = true;
                continue;
            }
            let Some(super::sampling::AdmittedSegment {
                decision,
                segment_start,
                woken,
                pending,
                resuming,
                completion,
            }) = operation.take_segment()
            else {
                continue;
            };

            let scoped =
                self.scheduler.phase() == flui_scheduler::SchedulerPhase::PersistentCallbacks;
            if scoped && completion && !pending && !woken && !resuming {
                presentation.complete_segment();
                continue;
            }

            // The presentation-frame transaction boundary (ADR-0048): a
            // panic that escaped every inner recovery layer (per-element
            // build recovery substitutes an `ErrorView`; the pipeline's own
            // `catch_unwind` surfaces layout/paint panics as
            // `RenderError::Poisoned`) is caught HERE, per presentation, so
            // it poisons at most this presentation's own frame. Without
            // this seam the unwind aborts every later sibling's segment in
            // this same loop and then kills the process through the
            // runner's `resume_unwind` — frame failure would be
            // process-global, not presentation-local. `AssertUnwindSafe`
            // follows the same reasoning as every inner boundary in this
            // workspace: the tree types are lock-free interior-mutability
            // structures whose guards release during unwind (parking_lot
            // does not poison), and the pipeline retains NEEDS_LAYOUT/dirty
            // marks on failure, so the next pump re-attempts from retained
            // premises rather than trusting partially-updated ones. What is
            // NOT re-established by unwinding is named honestly in
            // ADR-0048's consistency audit; nothing here claims full
            // transactionality of mid-segment mutations.
            let attempt = catch_unwind(AssertUnwindSafe(|| {
                let now = self.frame_time.get().unwrap_or(segment_start);
                let fresh = super::text_preparation::admit(presentation, constraints, now);
                Self::draw_prepared_frame_for_presentation(
                    presentation,
                    constraints,
                    &self.text,
                    fresh,
                )
            }));

            // A removal during the segment's one repeat left the focus on a
            // node that is gone; nothing else may ask for the frame that
            // moves it.
            if presentation.focus_manager().has_orphaned_focus() {
                self.request_redraw_for(presentation);
            }

            // Publish readiness before diagnostics or deferred owner code can
            // query render objects changed by this attempt.
            if matches!(&attempt, Ok(Ok(_))) {
                presentation.pipeline().publish_layout();
                if scoped {
                    presentation.complete_segment();
                }
            } else {
                presentation.pipeline().withdraw_layout();
                if scoped {
                    presentation.suspend_completion_segment();
                }
            }

            // The semantics diagnostics run a `tracing` subscriber, user code
            // that may read this pipeline, so they are reported once layout
            // availability reflects the attempt, a failed one included (a
            // finding an earlier subscriber left must not wait for a frame
            // that may never come). A subscriber that panics replaces neither
            // the frame's outcome nor its error; what it left unreported goes
            // back to the pipeline for the next frame.
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                presentation.pipeline().report_semantics_diagnostics();
            })) {
                // Forgotten, not dropped: its `Drop` may panic too.
                std::mem::forget(payload);
                // What it left unreported went back to the pipeline, which
                // no pending-work check counts: schedule the frame that
                // retries it.
                self.request_redraw_for(presentation);
            }

            // Drain exactly once after the entire attempt, outside the
            // catch. This includes recoveries produced by the layout
            // fixpoint and post-pipeline lazy service, even when later tail
            // or scene work unwinds. Vec order is the recovery order.
            for recovered in presentation.widgets().take_recovered_panics() {
                let kind = self
                    .frame_failure_detail
                    .get()
                    .recovered_panic_kind(recovered);
                self.report_frame_failure(presentation, kind);
            }

            let result = match attempt {
                Ok(Ok(outcome)) => {
                    super::text_preparation::finish(presentation);
                    // A terminal clean result resets only after every
                    // contained report from this attempt was delivered.
                    presentation.reset_frame_failure_streak();
                    outcome
                }
                Ok(Err(error)) => {
                    if let flui_rendering::RenderError::TextPreparationPending(frontier) = &error {
                        if super::text_preparation::pending(presentation, frontier.clone()) {
                            // Native preparation retains its logical segment;
                            // no scene, tree revision or failure streak is published.
                            continue;
                        }
                        super::text_preparation::finish(presentation);
                    } else {
                        super::text_preparation::finish(presentation);
                    }
                    if matches!(
                        error,
                        flui_rendering::RenderError::TextPreparationPending(_)
                    ) {
                        // An unresolved source has no producer to advance this
                        // layout. Retain its work until the owner changes input.
                        presentation
                            .pipeline()
                            .with_mut(PipelineOwner::defer_layout_until_input);
                    }
                    let waits_for_input = matches!(
                        error,
                        flui_rendering::RenderError::TextLayout(_)
                            | flui_rendering::RenderError::TextContextBusy
                            | flui_rendering::RenderError::TextPreparationPending(_)
                    );
                    self.report_frame_failure(presentation, FrameFailureKind::Pipeline { error });
                    if !waits_for_input {
                        Self::arm_failure_retry(presentation);
                    }
                    FramePaintOutcome::Errored {
                        retry: !waits_for_input || presentation.has_pending_work_snapshot(),
                    }
                }
                Err(payload) => {
                    super::text_preparation::finish(presentation);
                    let failed_phase = presentation.segment_phase();
                    let (message, internal_invariant) =
                        self.frame_failure_detail.get().panic_text(&*payload);
                    // The payload may contain several panicking destructors.
                    // Retain it before any recovery operation or diagnostics.
                    flui_foundation::panic::retain_opaque_payload(payload);
                    if matches!(failed_phase, SegmentPhase::Tail | SegmentPhase::Scene) {
                        // The pipeline already consumed this presentation's
                        // paint dirtiness before either post-pipeline segment
                        // began. Re-dirty the exact failed presentation here,
                        // while its identity is still local to this catch;
                        // a later clean sibling must not steal attribution.
                        self.mark_needs_full_repaint_for(presentation);
                    }
                    self.report_frame_failure(
                        presentation,
                        FrameFailureKind::SegmentPanic {
                            message,
                            phase: failed_phase,
                            internal_invariant,
                        },
                    );
                    Self::arm_failure_retry(presentation);
                    FramePaintOutcome::Errored { retry: true }
                }
            };
            if matches!(
                &result,
                FramePaintOutcome::Painted(_) | FramePaintOutcome::Errored { .. }
            ) {
                let tree_revision = presentation.advance_tree_revision();
                tracing::trace!(
                    target: "flui.frame",
                    event = "tree_revision_advanced",
                    { flui_foundation::diagnostics::PRESENTATION_ID } =
                        presentation.id().as_u64(),
                    tree_revision = tree_revision.as_u64(),
                    "Presentation tree revision advanced"
                );
            }
            // Telemetry: remember this segment's span so `render_frame`
            // (this method's own caller, which decides whether/how to submit)
            // can attach it to a `FrameSnapshot` at its own submit point --
            // see `PresentationState::last_segment_span`'s own doc for why
            // this is a side channel rather than a local variable, and why
            // it lives on the presentation rather than the shared
            // `FrameClock`. `take_last_segment_span` (never a plain `get`)
            // is what makes this per-pump: a presentation whose segment does
            // NOT run some later pump sees `None` here, not this pump's
            // stale value.
            presentation.set_last_segment_span(segment_start, presentation.clock().now());
            // Latch "first frame confirmed sent" only for an unconditional
            // `Produce` that didn't error -- mirrors the old
            // `RenderingBinding::mark_first_frame_sent`'s `!errored`
            // guard, now also excluding `ProduceWithheld`: a withheld
            // result was never sent, by construction, so confirming it as
            // sent would be self-contradictory and would wrongly disarm a
            // later `defer_first_frame` call. `render_frame` below
            // separately re-checks `is_deferred()` at its own submit point
            // before honoring whatever this segment produced.
            if decision.is_produce() && !matches!(result, FramePaintOutcome::Errored { .. }) {
                presentation.clock().mark_first_frame_sent();
            }
            // A dropped frame that backs off is woken by its retry deadline,
            // not by a frame asked for now.
            if matches!(result, FramePaintOutcome::Errored { retry: true })
                && presentation.failure_retry_at().is_none()
            {
                any_retry_needed = true;
            }
            last_outcome = result;
            producer = presentation.id();
        }
        (producer, last_outcome, any_retry_needed)
    }

    /// The constraints `presentation`'s root is laid out with this frame: the
    /// frame's surface for the primary, and each other presentation's own
    /// window, in logical pixels, falling back to `primary` for a window that
    /// is gone or reports no area.
    pub(super) fn root_constraints(
        &self,
        presentation: &PresentationState,
        primary: BoxConstraints,
    ) -> BoxConstraints {
        if presentation.id() == self.presentations.primary().id() {
            return primary;
        }
        presentation
            .with_window(|window| {
                let state = window.state();
                state.metrics.map_or(
                    state.content_size,
                    flui_foundation::geometry::WindowMetrics::logical,
                )
            })
            .filter(|size| {
                size.width.is_finite()
                    && size.height.is_finite()
                    && size.width > 0.0
                    && size.height > 0.0
            })
            .map_or(primary, BoxConstraints::tight)
    }

    /// Drive one frame on this UI runtime's scheduler — begin frame, persistent
    /// callbacks with `pipeline`, post-frame callbacks — as its
    /// presentations' text-store transaction (ADR-0027 §3), then run the
    /// commit anchor.
    ///
    /// Every presentation's commit gate is shut for the whole drive, so an
    /// input method's lock asked for anywhere inside it (a build, a
    /// post-frame callback, a nested platform pump) is refused (sync) or
    /// queued (async), and no platform edit lands in a tree mid-frame. The
    /// gates restore their enclosing state on every exit, a panic's unwind
    /// included. The queued
    /// grants run only after `OwnerFrame::drive_frame` returned, with the
    /// scheduler back in `Idle`: an edit a grant makes there marks the tree
    /// dirty and schedules the next frame like any other owner-thread edit,
    /// where the same edit inside the frame would have its visual-update
    /// request dropped.
    ///
    /// Crate-private: a host drives a frame only through [`Self::pump`],
    /// which runs its begin, draw and end frame steps inside this drive.
    pub(crate) fn drive_frame(
        &self,
        now: Instant,
        deadline: flui_scheduler::IdleDeadline,
        mut prepare: impl FnMut(),
        pipeline: impl FnMut() -> bool,
    ) -> bool {
        let mut commits_closed = None;
        let commit_slot = &mut commits_closed;
        // Owed until the anchor below completes: a drive that unwinds leaves
        // it to the next owner turn (ADR-0142 item 5).
        self.anchor_owed.set(true);
        // The frames every presentation has rendered or dropped so far: a drive that unwinds
        // having added none (a transient callback, say) had no frame to report.
        let frames_seen = || {
            self.presentations.iter().fold(0_u64, |seen, presentation| {
                seen.saturating_add(presentation.frames_rendered())
                    .saturating_add(presentation.frames_dropped())
            })
        };
        let seen_before = frames_seen();
        // A callback that panics after the frame presented unwinds out of the drive, but the
        // frame happened: what an agent is owed for it is still delivered below, and the panic
        // continues after.
        let driven = catch_unwind(AssertUnwindSafe(|| {
            self.owner_frame.drive_frame(
                now,
                deadline,
                move || {
                    prepare();
                    *commit_slot = Some(TextCommitsClosed::close(self));
                },
                pipeline,
            )
        }));
        drop(commits_closed);
        // A frame has passed: the readings the actions an agent ran owe are taken from the tree it
        // left, the events it produced are emitted, and the waits an agent parked see the window as
        // the frame left it, events included.
        let (result, mut first_panic) = match driven {
            Ok(result) => (Some(result), None),
            Err(payload) => (None, Some(payload)),
        };
        if result.is_some() || frames_seen() != seen_before {
            // Contained: the platform's wake is user code, and the drive's own failure, when
            // there is one, is the one that continues.
            let reported = catch_unwind(AssertUnwindSafe(|| {
                for presentation in self.presentations.iter() {
                    Self::complete_agent_reads(presentation);
                }
                self.publish_agent_events();
                self.evaluate_agent_waits(true);
            }))
            .err();
            preserve_first_input_panic(&mut first_panic, reported, "agent frame report");
        }
        if let Some(payload) = first_panic {
            resume_unwind(payload);
        }
        let result = result
            .expect("BUG: a drive that did not unwind returned")
            .expect("BUG: the runtime's live frame owner must admit its frame transaction");
        self.run_commit_anchor();
        result
    }

    /// The commit anchor: each presentation's queued grants, against the
    /// stores that queued them, with the failure an owner callback parked in
    /// its gate reported first (ADR-0142 items 2 and 5).
    ///
    /// Each presentation's anchor is contained, so one that fails does not
    /// hold back its siblings' grants; the first failure is resumed once
    /// every presentation ran. The debt is cleared only when every
    /// presentation's anchor completed with its commits open: after a
    /// failure, or with a presentation's commits still closed (its native
    /// text preparation waits), the next owner turn runs it again.
    fn run_commit_anchor(&self) {
        let mut first = None;
        let mut paid = true;
        for presentation in self.presentations.iter() {
            if presentation.text_input().is_transaction_open() {
                // Its commits are still closed (a frame, a native text
                // preparation): its anchor comes when they reopen.
                paid = false;
                continue;
            }
            let ran = catch_unwind(AssertUnwindSafe(|| {
                let _ran = presentation.text_input().run_deferred_grants();
            }))
            .err();
            paid &= ran.is_none() && !presentation.text_input().is_transaction_open();
            crate::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first,
                ran,
                "commit anchor",
            );
            presentation.rearm_completion();
        }
        self.anchor_owed.set(!paid);
        if let Some(failure) = first {
            resume_unwind(failure);
        }
    }

    /// Pay the commit anchor if this UI runtime owes it: a frame drive that
    /// did not complete its anchor, or a failure owner code raised after a
    /// grant the platform asked for outside any dispatch or anchor, which
    /// waits in a presentation's gate (ADR-0142 item 5). Every owner turn
    /// pays it first: the inbox drain, the background pump and the pump,
    /// before its frame.
    ///
    /// # Panics
    ///
    /// Resumes the anchor's first failure, as [`Self::run_commit_anchor`].
    pub(crate) fn pay_owed_anchor(&self) {
        let owed = self.anchor_owed.get()
            || self
                .presentations
                .iter()
                .any(|presentation| presentation.text_input().holds_failure());
        if owed {
            self.run_commit_anchor();
        }
    }

    /// Whether the commit anchor is owed ([`Self::pay_owed_anchor`]).
    pub(crate) fn owes_anchor(&self) -> bool {
        self.anchor_owed.get()
    }

    /// Record whether this UI runtime is checked out of its owner's slot:
    /// while it is, every presentation's stores admit only their owner's own
    /// calls (ADR-0142 item 6). A platform entry from elsewhere queues its
    /// grant for [`Self::finish_checkout`].
    pub(crate) fn set_text_commits_checked_out(&self, checked_out: bool) {
        for presentation in self.presentations.iter() {
            presentation.text_input().set_checked_out(checked_out);
        }
    }

    /// The end of an owner turn, before the runtime returns to its slot:
    /// the commit anchor runs, so a grant a platform entry queued while the
    /// runtime was checked out runs at the end of the turn it came in.
    ///
    /// # Panics
    ///
    /// Resumes the anchor's first failure, as [`Self::run_commit_anchor`].
    pub(crate) fn finish_checkout(&self) {
        self.enter(UiRuntime::run_commit_anchor);
    }

    /// One presentation's build+layout+paint segment — moves VERBATIM from
    /// the retired `AppBinding::draw_frame_entered`'s Phase 1–4, now
    /// parameterized by `presentation` instead of hard-wired to
    /// `self.presentations.primary()`.
    ///
    /// **Why the caller's dirty gate cannot skip a segment this body would
    /// have produced `Painted` for:** `run_frame_with_layout_builders`
    /// itself returns `None` (this function's `Idle` outcome) whenever
    /// nothing is dirty — that decision already lived inside the pipeline
    /// before this loop existed. `Self::has_pending_work` (build pending OR
    /// a dirty render node) is exactly the union of conditions that
    /// decision depends on, so gating the CALL on the same union, one level
    /// up, changes nothing observable: either both agree nothing is dirty
    /// (skip here, `Idle` there — same outcome, cheaper), or either finds
    /// real work and the segment runs exactly as it always did.
    #[cfg(test)]
    pub(super) fn draw_frame_for_presentation(
        presentation: &PresentationState,
        constraints: BoxConstraints,
        text: &flui_rendering::TextContextHandle,
    ) -> Result<FramePaintOutcome, flui_rendering::RenderError> {
        Self::draw_prepared_frame_for_presentation(presentation, constraints, text, true)
    }

    fn draw_prepared_frame_for_presentation(
        presentation: &PresentationState,
        constraints: BoxConstraints,
        text: &flui_rendering::TextContextHandle,
        fresh: bool,
    ) -> Result<FramePaintOutcome, flui_rendering::RenderError> {
        presentation.enter_segment_phase(SegmentPhase::Build);
        let mut phases = PhaseStopwatch::start(presentation);

        #[cfg(test)]
        presentation.record_flush();

        // Phase 1: enter the widget frame unconditionally. `draw_frame`
        // already skips `build_scope` when nothing is dirty, but its frame
        // entry also discards any undrained lifecycle-panic records from the
        // preceding frame. A pipeline-only segment must still perform that
        // cleanup: lazy child service below can produce records after the
        // preceding frame's build phase, leaving no pending build to make a
        // conditional call here run on the next frame.
        if fresh {
            presentation.widgets().draw_frame_with_phase_marker(
                presentation.segment_phase_marker(),
                SegmentPhase::Finalize,
            );
        }

        phases.lap(FramePhase::Build);

        // Phase 2 & 3: Layout, Compositing, Paint, Semantics through the
        // typestate-driven orchestrator.
        presentation.enter_segment_phase(SegmentPhase::Pipeline);
        presentation
            .renderer()
            .root_pipeline_owner()
            .with_mut(|owner| owner.set_root_constraints(Some(constraints)));
        let widgets = presentation.widgets();
        widgets.settle_layout_with_builders(presentation.pipeline())?;
        phases.lap(FramePhase::Layout);
        // Focus leaves a node the build or layout removed now, with the tree
        // settled and no tree borrow held: the move runs focus listeners. What
        // the move dirties is rebuilt and laid out once more before paint;
        // anything it dirties beyond that waits for the next frame, which the
        // frame tail requests.
        if Self::settle_orphaned_focus(presentation) {
            presentation.enter_segment_phase(SegmentPhase::Build);
            widgets.rebuild_within_frame();
            phases.lap(FramePhase::Build);
            presentation.enter_segment_phase(SegmentPhase::Pipeline);
            widgets.settle_layout_with_builders(presentation.pipeline())?;
            phases.lap(FramePhase::Layout);
        }
        // Paint commits the layer tree (which carries its own leader index)
        // before semantics runs; a semantics error retains it for the retry.
        let layer_tree = presentation.pipeline().run_frame()?;

        presentation.enter_segment_phase(SegmentPhase::Tail);

        // Production<->headless convergence point: the lazy-sliver safety net.
        // The fixpoint above already serviced child requests between its
        // layout passes; this drains only what a pass-bound-limited frame's
        // final `run_frame` layout emitted, and is a no-op otherwise.
        {
            let w = presentation.widgets();
            w.service_child_requests(presentation.pipeline());
        }

        // Phase 4: freeze the LayerTree into a Scene.
        if let Some(mut layer_tree) = layer_tree {
            presentation.enter_segment_phase(SegmentPhase::Scene);
            // Probe before the debug overlay joins the scene: it is not content.
            presentation
                .overlay_style
                .stage(&layer_tree, constraints.max_width);
            presentation.attach_performance_overlay(&mut layer_tree, text);
            presentation.stage_input_regions();

            // By value, not `Arc<Scene>` — see `FramePaintOutcome::Painted`'s
            // own doc for why.
            let scene = Scene::new(layer_tree);
            phases.lap(FramePhase::Paint);
            Ok(FramePaintOutcome::Painted(scene))
        } else {
            phases.lap(FramePhase::Paint);
            Ok(FramePaintOutcome::Idle)
        }
    }

    /// Move `presentation`'s focus off a node this frame removed; whether the
    /// primary focus changed.
    fn settle_orphaned_focus(presentation: &PresentationState) -> bool {
        let manager = presentation.focus_manager();
        let primary = || manager.primary_focus().map(|node| node.id());
        let before = primary();
        manager.settle_orphaned_focus();
        primary() != before
    }

    /// Commit a painted presentation after the sink accepts its scene.
    fn commit_painted_frame(&self, presentation: &PresentationState) {
        debug_assert!(
            self.presentations.get(presentation.id()).is_some(),
            "commit target must belong to this ui_runtime"
        );
        let committed_revision = presentation.commit_tree_revision();
        presentation.publish_input_regions();
        tracing::trace!(
            target: "flui.frame",
            event = "tree_revision_committed",
            { flui_foundation::diagnostics::PRESENTATION_ID } = presentation.id().as_u64(),
            tree_revision = committed_revision.as_u64(),
            presented_revision = committed_revision.as_u64(),
            "Presentation tree revision committed"
        );
    }

    fn replay_committed_held_pointer_input(&self, presentation: &PresentationState) {
        let Some(mut replay) = HeldPointerReplay::begin(presentation.held_pointer_input()) else {
            return;
        };
        let mut replayed = 0usize;
        for mut held in replay.by_ref() {
            let row = held.take_row();
            if row.is_some() {
                // Verdicts reached earlier in the frame precede this input's
                // row; the ones its dispatch reaches follow it.
                self.admit_decisions();
            }
            let dispatch =
                Self::dispatch_pointer_event_entered(presentation, held.surface, &held.event);
            // Recorded before a dispatch failure resumes: the event was
            // dispatched, so its row is not lost with the unwind.
            if let Some(row) = row {
                row.dispatched(&held.event, dispatch.as_ref().ok().copied().flatten());
            }
            replayed = replayed.saturating_add(1);
            if let Err(payload) = dispatch {
                self.request_redraw_for(presentation);
                resume_unwind(payload);
            }
        }
        replay.complete();
        if replayed != 0 {
            self.request_redraw_for(presentation);
        }
    }

    /// The draw step of the frame transaction: render one frame through
    /// `sink`, inside the UI runtime entry [`Self::pump`] already holds, so
    /// scheduler callbacks and the full build/layout/paint/submit transaction
    /// share one activation instead of creating a nested scope.
    ///
    /// Crate-private: a host reaches it only through [`Self::pump`], so no
    /// code outside this crate can draw a frame that skips begin or end frame
    /// (tests outside it use `render_frame_for_test`, under `testing`).
    ///
    /// The host chooses the sink it hands the pump: `flui-app` passes its
    /// raster lane on the desktop, Android and iOS runners and a direct sink
    /// over a borrowed backend on the web runner; tests pass a scripted one.
    /// Every sink feeds the same classification arms below
    /// through [`SubmitVerdict`](crate::sink::SubmitVerdict), so the
    /// retry/telemetry semantics cannot drift between them.
    ///
    /// Returns whether the frame reached `present()` — needed for the
    /// runner's no-present fallback throttle: `Fifo` present blocks every
    /// PRESENTED frame at display cadence, but a frame that never presents
    /// (nothing dirty, no damage, occluded surface, surface lost) carries no
    /// such pacing signal.
    ///
    /// Step by step: settle any lone arena member queued by an earlier event
    /// whose owner boundary could not finish (e.g. after a panic); flush
    /// coalesced pointer moves; draw the frame (`Self::draw_frame_entered`);
    /// classify its result and, when the actual producer is not deferred,
    /// submit a non-empty painted scene and commit an accepted verdict;
    /// re-hit-test stationary pointing devices against each committed
    /// presentation's own tree; then arm retry work or mark the
    /// pump rendered as applicable. The producer's `FrameClock::is_deferred`
    /// submit gate is DELIBERATELY
    /// separate from `draw_frame_entered`'s own segment gate: first-frame
    /// deferral withholds only the submit, never the build/layout/paint
    /// work (the framework still does all the work to produce frames, but
    /// those frames are never sent to the engine), so a deferred presentation's segment can very
    /// much have produced a real `Painted` outcome here, and this check is
    /// what keeps it off the engine. A stale/lost surface, a lost device,
    /// and a pipeline `Errored` outcome all count as a dropped (not
    /// settled) frame, arming a retry via [`Self::wake_frame`] instead of
    /// [`Self::mark_rendered`]'s idle-clear — but only the submit-failure
    /// verdicts (`SurfaceStale`/`DeviceLost`) additionally set the local
    /// `retry_needs_repaint` flag. A Tail/Scene panic also consumed the
    /// pipeline, but its per-presentation catch immediately re-dirties the
    /// exact failed presentation before a later sibling can become the
    /// pump's producer. A structured pipeline error and a generic `Failed`
    /// submit do neither; they did not produce a reusable scene or are not
    /// retried, respectively.
    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn render_frame<S: crate::sink::FrameSink + ?Sized>(&self, sink: &mut S) -> bool {
        let mut input_panic = None;
        let frame_time = self
            .frame_time
            .get()
            .unwrap_or_else(|| flui_foundation::MonotonicClock::now(&self.clock));
        for presentation in self.presentations.iter() {
            presentation.watch_redraw_requests();
        }
        for presentation in self.presentations.iter() {
            let deferred = catch_unwind(AssertUnwindSafe(|| {
                presentation.gestures().drain_deferred_arena_resolutions();
            }))
            .err();
            super::input::preserve_first_input_panic(
                &mut input_panic,
                deferred,
                "frame deferred arena resolution",
            );
            // A delivered frame's vsync, when the host reported one, anchors
            // motion sampling on the event timeline (ADR-0207 §2).
            let vsync = presentation
                .frame_request
                .take()
                .map(|request| request.vsync);
            let motion = catch_unwind(AssertUnwindSafe(|| {
                let gestures = presentation.gestures();
                if gestures.is_resampling_enabled() {
                    let frame_time = vsync
                        .and_then(|vsync| {
                            flui_interaction::__runtime::wake_instant(gestures, vsync)
                        })
                        .unwrap_or(frame_time);
                    let sample = frame_time
                        .checked_sub(flui_interaction::pointer::DEFAULT_RESAMPLE_LOOKBACK)
                        .unwrap_or(frame_time);
                    if let Some(next) = sample.checked_add(gestures.sampling_clock().period()) {
                        gestures
                            .flush_pending_moves_at(sample, next)
                            .expect("BUG: positive sampling period creates an advancing window");
                    }
                } else {
                    gestures.flush_pending_moves();
                }
            }))
            .err();
            super::input::preserve_first_input_panic(
                &mut input_panic,
                motion,
                "frame pointer motion",
            );
        }
        let pending_wake = self.wake_pending_pointer_samples();
        super::input::preserve_first_input_panic(
            &mut input_panic,
            pending_wake,
            "pending pointer sample wake",
        );
        if let Some(payload) = input_panic {
            resume_unwind(payload);
        }

        let (width, height) = sink.surface_size();
        let dpr = self
            .presentations
            .primary()
            .renderer()
            .root_pipeline_owner()
            .with(PipelineOwner::device_pixel_ratio);
        let constraints = BoxConstraints::tight(Size::new(width as f64 / dpr, height as f64 / dpr));
        let (producer_id, outcome, any_retry_needed) = self.draw_frame_entered(constraints);
        // The presentation whose segment produced `outcome` above is never
        // inferred as `primary()`: test scaffolding can attach content to a
        // secondary and exercises this attribution. Production secondary
        // windows are contentless today, and simultaneous paintable
        // presentations remain unsupported because this transaction has
        // one constraints set, one sink, and retains only the last produced
        // scene. Issue #559 must add per-presentation constraints, sinks,
        // and submit routing before production secondary content is enabled;
        // this producer lookup does not define last-scene-wins behavior.
        let producer = self
            .presentations
            .get(producer_id)
            .unwrap_or_else(|| self.presentations.primary());

        // The submit gate: withheld while the PRODUCER's own first frame is
        // deferred -- see this method's own doc for why this is a SEPARATE
        // check from `draw_frame_entered`'s segment gate, not a redundant
        // re-check of the same decision.
        let should_send = !producer.clock().is_deferred();

        let mut presented = false;
        // `any_retry_needed`, not "the producer's outcome was Errored": with more
        // than one presentation mounted, an earlier presentation's failed
        // segment followed by a clean sibling `Painted` must still arm the
        // retry — keying this off the LAST outcome alone would end such a
        // pump in `mark_rendered()`, clearing the very wake the failed
        // presentation's retry needs (and on a pump whose failure consumed
        // its build-dirty state, nothing else would ever reopen the gate).
        let mut retry_needed = any_retry_needed;
        // Tracks a NARROWER condition than `retry_needed`: whether a SUBMIT
        // failure consumed the scene and therefore needs
        // [`Self::mark_needs_full_repaint_for`] before a later retry. A
        // post-pipeline Tail/Scene panic has already re-dirtied the exact
        // failed presentation in the per-presentation catch; it never sets
        // this pump-local flag. A structured pipeline error, or a panic in
        // Build/Finalize/Pipeline, has not successfully produced a scene and
        // likewise does not use this submit-specific flag.
        let mut retry_needs_repaint = false;
        let mut replay_committed_input = false;
        // Only a scene the screen shows (or already showed: no damage) may
        // change the system bars; a withheld one keeps its style staged.
        let mut deliver_overlay_style = false;
        use crate::sink::SubmitVerdict;
        if should_send && let FramePaintOutcome::Painted(scene) = outcome {
            // The frame this scene will become once presented.
            let frame_number = producer.frames_rendered() + 1;
            // The scene is handed over BY VALUE: on the raster-lane path it
            // crosses the raster boundary as an owned, stamped
            // `SceneSnapshot` (the mailbox seam's own no-`Arc<Scene>`
            // contract); the direct path borrows it internally and drops it
            // when the render returns.
            let render_verdict = sink.submit(scene);
            // Telemetry: sampled AFTER the submit returns, never before
            // it -- a `Presented` verdict means the backend called
            // `output.present()`, and whatever pacing block that call carries
            // happens inside it rather than after it (under the default
            // `Fifo` mode the block is a wait for the next vsync on the
            // Vulkan/Wayland path; the native AppKit backend returns from the
            // present in ~42 µs — ADR-0058's per-backend facts. On the inline
            // raster lane the pump runs synchronously inside `submit`, so the
            // call's own duration lands here either way). Sampling before the
            // call (as an
            // earlier version of this method did) understates every
            // "input-to-present"/"produce-to-present" latency by up to a
            // full frame interval -- it would measure submit, not present,
            // while `flui_scheduler::frame_histogram`'s
            // `input_to_present_histogram`/`produce_to_present_histogram`
            // names (and `FrameSnapshot::submit_at`'s own doc) already
            // claim the present-inclusive meaning. Sampling once, here,
            // after the call returns -- for EVERY verdict, not just
            // `Presented` -- keeps that claim true uniformly: a failure
            // arm's `submit_at` is likewise "whenever the backend finished
            // attempting this submit", including whatever work (partial
            // blocking, driver validation) it did before failing. Never
            // recorded for `NoPresent`/`NotShown` (no damage, or surface
            // unavailable -- either way the frame never reached the
            // screen): those branches' pending input
            // epochs stay buffered on the clock for whichever later pump
            // does submit, per `FrameClock::record_frame`'s own doc.
            let submit_at = producer.clock().now();
            match render_verdict {
                SubmitVerdict::Presented => {
                    self.commit_painted_frame(producer);
                    // The frame reached `present()`: frame-aligned haptics
                    // play with it (ADR-0210 §4). Only this arm plays them;
                    // the others leave the queue to its deadline.
                    producer.haptics().frame_presented();
                    deliver_overlay_style = true;
                    presented = true;
                    producer.record_frame_rendered();
                    // The surface is demonstrably available: any withheld
                    // streak ends here, so a later withdrawal gets the full
                    // retry budget again.
                    producer.clear_not_shown_streak();
                    self.record_submit_telemetry(
                        frame_time,
                        producer,
                        submit_at,
                        PresentOutcome::Presented,
                        EpochDisposition::Drain,
                    );
                    tracing::trace!(
                        frame = frame_number,
                        total = producer.frames_rendered(),
                        "Frame rendered successfully"
                    );
                    replay_committed_input = true;
                }
                SubmitVerdict::NoPresent => {
                    self.commit_painted_frame(producer);
                    // No frame-aligned haptics: the backend found no damage,
                    // so the screen kept the previous frame and the change a
                    // queued request confirms never showed. A request queued
                    // before that previous frame already resolved with it; the
                    // rest resolve `Suppressed` at their deadline.
                    deliver_overlay_style = true;
                    // Nothing was owed and nothing was lost — the surface was
                    // never asked to show anything, so a withheld streak is
                    // over just as it is on a present.
                    producer.clear_not_shown_streak();
                    tracing::trace!(
                        frame = frame_number,
                        "Frame skipped: no damage (no present)"
                    );
                    replay_committed_input = true;
                }
                // The backend had content in hand and could not put it on
                // screen. Everything `NoPresent` does about the TREE still
                // applies — the pipeline ran, produced a scene, and that
                // scene is the latest one this presentation has — but the
                // frame itself is NOT finished, and treating it as finished
                // is how a window ends up permanently blank: the work that
                // produced this scene has already been consumed out of the
                // pipeline, so a loop that parks here has nothing left to
                // wake it and nothing left to draw if it did.
                //
                // Hence the same two-part retention the submit-failure arms
                // below use, and for the identical reason issue #637
                // records: `wake_frame()` alone re-opens the segment gate
                // but not `PipelineOwner`'s own dirty tracking, so the
                // retry pump would find nothing to do, produce `Idle`, and
                // clear the flag having never reached `render_scene`.
                // `mark_needs_full_repaint_for` is what gives the retry
                // real work.
                //
                // The retry is paced by the runner's fallback gate
                // (`keeps_frame_gate_open` / `FallbackWake`, ADR-0058) — one
                // pipeline pass per display period — and capped here at
                // `MAX_NOT_SHOWN_RETRIES` consecutive attempts.
                //
                // The cap is load-bearing, not belt-and-braces. A window that
                // AppKit reports as occluded does stop reaching this arm, but
                // by a route that does NOT cover this condition: occlusion
                // reaches the frame loop as
                // `windowDidChangeOcclusionState:` -> `WindowVisibility` ->
                // `AppLifecycleState::Hidden` -> `frames_enabled == false`, and
                // the visibility bit there is AppKit's `occlusionState`. What
                // withdraws the drawable, however, is the *swapchain's* own
                // availability, and the two disagree — the cold-start trace
                // that motivated this arm has `occlusionState` reporting the
                // window visible throughout while the drawable was
                // unavailable for 12 consecutive frames. Wherever they
                // disagree and stay disagreeing (a window on an inactive
                // Space, a display asleep) the lifecycle gate never engages and
                // an unbounded retry is an unbounded loop. The transient that
                // clears is the common case; the cap is what makes the
                // pathological one terminate.
                SubmitVerdict::NotShown => {
                    // The frame never reached the screen, so frame-aligned
                    // haptics wait: a retry that presents before their
                    // deadline plays them, otherwise they resolve `Suppressed`.
                    self.commit_painted_frame(producer);
                    producer.record_frame_dropped();
                    let streak = producer.record_frame_withheld();
                    replay_committed_input = true;
                    if streak <= MAX_NOT_SHOWN_RETRIES {
                        tracing::debug!(
                            frame = frame_number,
                            streak,
                            "Frame rendered but never shown (surface unavailable); \
                             retaining it — retry armed via wake_frame()"
                        );
                        retry_needed = true;
                        retry_needs_repaint = true;
                    } else {
                        // Budget exhausted: park rather than retry. The tree
                        // stays committed (the scene above is still the latest
                        // this presentation has).
                        //
                        // Reset the streak with the park, not on the next
                        // presented frame alone. Left above the limit, the
                        // very next event-driven frame that lands here — the
                        // drawable still unavailable, which is exactly the
                        // case this cap exists for — would increment the stale
                        // count and park again immediately, never opening the
                        // fresh retry burst the comment above promises. If the
                        // drawable returned asynchronously afterwards the
                        // window could stay blank until some unrelated event
                        // forced a successful frame. Clearing here makes the
                        // cap a bound on a *continuous* withdrawal, never a
                        // permanent disable: the next real event gets a full
                        // budget again.
                        producer.clear_not_shown_streak();
                        tracing::warn!(
                            frame = frame_number,
                            streak,
                            "Frame withheld by the surface for {} consecutive attempts; \
                             giving up until the next real event",
                            MAX_NOT_SHOWN_RETRIES
                        );
                    }
                }
                SubmitVerdict::Retry | SubmitVerdict::SurfaceStale => {
                    producer.record_frame_dropped();
                    self.record_submit_telemetry(
                        frame_time,
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Retain,
                    );
                    // The failed submit consumed painting work. A wake alone
                    // cannot repaint a clean static tree, so retain its epochs
                    // and dirty the producer for the ordinary paced retry.
                    retry_needed = true;
                    retry_needs_repaint = true;
                    tracing::debug!(
                        ?render_verdict,
                        "Transient render failure; paced retry armed"
                    );
                }
                SubmitVerdict::DeviceLost => {
                    producer.record_frame_dropped();
                    // `Retain`, not `Drain`: the frame is tried again, and the
                    // eventual real submit must still find the epochs that
                    // arrived before this failure — see
                    // `record_submit_telemetry`'s own doc.
                    self.record_submit_telemetry(
                        frame_time,
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Retain,
                    );
                    // The submit consumed the paint demand, so the frame the
                    // rebuilt device shows needs a full repaint whenever it
                    // comes.
                    self.mark_needs_full_repaint_for(producer);
                    // Paced like any dropped frame (ADR-0267): a frame asked
                    // for at once would run again before the renderer's owner
                    // next rebuilds the device, and fail the same way. A
                    // given-up rebuild was reported when it was given up and
                    // holds the frames until a rebuild is asked for again.
                    if !producer.device_abandoned() {
                        let recovery = producer.device_recovery().unwrap_or_default();
                        self.report_frame_failure(
                            producer,
                            FrameFailureKind::DeviceLost { recovery },
                        );
                        Self::arm_failure_retry(producer);
                    }
                }
                SubmitVerdict::Failed => {
                    producer.record_frame_dropped();
                    self.record_submit_telemetry(
                        frame_time,
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Drain,
                    );
                    // The sink already logged the backend-specific error at
                    // classification time; this records the transaction-
                    // level consequence.
                    tracing::error!("Render error (non-recoverable this frame)");
                }
            }
        }

        // Each window refreshes from its own acknowledged tree. A sibling's
        // failed segment cannot substitute its tree or suppress this work.
        let mut hover_panic = None;
        for presentation in self
            .presentations
            .iter()
            .filter(|presentation| presentation.frame_commit_state() == FrameCommitState::Committed)
            // Hover holds while a scroll is latched (ADR-0208 rule 6).
            .filter(|presentation| !presentation.gestures().is_hover_frozen())
        {
            let hit_panic = std::cell::Cell::new(None);
            let refresh = catch_unwind(AssertUnwindSafe(|| {
                presentation
                    .gestures()
                    .mouse_tracker()
                    .update_all_devices(|position, kind| {
                        let mut result = flui_interaction::pointer::HitTestResult::new();
                        // A committed frame has published its layout, so the
                        // tree answers; a render object's panic is a miss and
                        // reported after the refresh.
                        if let ViewHitTest::Answered { panic } = presentation
                            .renderer()
                            .hit_test_in_view(&mut result, position, kind, 0)
                        {
                            let mut held = hit_panic.take();
                            super::input::preserve_first_input_panic(
                                &mut held,
                                panic,
                                "frame ambient hover hit test",
                            );
                            hit_panic.set(held);
                        }
                        result
                    });
            }))
            .err();
            super::input::preserve_first_input_panic(
                &mut hover_panic,
                hit_panic.into_inner(),
                "frame ambient hover hit test",
            );
            super::input::preserve_first_input_panic(
                &mut hover_panic,
                refresh,
                "frame ambient hover refresh",
            );
        }
        if let Some(payload) = hover_panic {
            resume_unwind(payload);
        }

        // The one place a frame decides whether the loop continues itself.
        // A frame that presents is followed by the compositor's own pacing,
        // but a frame that did NOT present continues the loop only if this
        // decision arms a wake — so a stall is legible here and nowhere
        // else: `presented=false retry_needed=false` is a frame that
        // consumed the work and left nothing to come back for. Same target
        // and level as `flui.pace`'s wake trace, which fires once per
        // platform wake; this one fires once per frame.
        tracing::trace!(
            target: "flui.pace",
            event = "frame_tail",
            presented,
            any_retry_needed,
            retry_needed,
            retry_needs_repaint,
            "frame tail resolved"
        );

        if retry_needed {
            // Issue #637: `wake_frame()` alone re-opens `draw_frame_entered`'s
            // per-presentation segment gate but never touches `PipelineOwner`'s
            // own independent dirty tracking — the frame that just failed to
            // submit had already consumed whatever was dirty producing the
            // scene it tried to send, so an otherwise-unchanged tree finds
            // nothing dirty on the retry pump, produces `Idle`, and
            // `mark_rendered()` clears the flag having never reached
            // `render_scene`: one no-op frame, then the retry silently parks.
            //
            // `retry_needs_repaint` (set only by the submit-failure arms;
            // post-pipeline segment panics were already re-dirtied at their
            // exact per-presentation catch) gates
            // [`Self::mark_needs_full_repaint_for`], the same fix #630
            // already established for the pre-frame device-recovery-success
            // arm (`runner.rs`'s `render_frame_with_device_recovery`) —
            // shared rather than duplicated via that one function, since
            // both are "a retry needs the pipeline to actually have work to
            // redo, not just a wake"; see that function's own doc for why
            // `mark_needs_paint` (not `mark_needs_layout`) is the right
            // weight. Addressed to `producer` — the presentation whose
            // segment actually produced the scene that just failed to
            // submit, resolved above and NEVER assumed to be `primary()`
            // (this method's own doc, at the `producer` binding, explains
            // why the two can differ) — not `self.presentations.primary()`:
            // an earlier version of this fix wrongly hard-coded `primary()`
            // here, which left the fix inert on any pump where a secondary
            // presentation was the actual producer.
            //
            // Steady-state cost of a PERMANENTLY failing submit: unlike the
            // pre-frame device-recovery path, this arm is not gated by
            // `DeviceRecoveryBackoff` — a surface that keeps failing every
            // submit now retries every wake, throttled only by
            // `runner.rs`'s `NO_PRESENT_FALLBACK_PACE` (~62 Hz), running a
            // full-tree paint each time, indefinitely. Before this fix that
            // same surface went quiet after one no-op frame. A working
            // retry that costs CPU forever on a permanently broken surface
            // is still strictly better than one that silently gives up, but
            // whether this steady state deserves its own backoff is a
            // separate question this change does not answer.
            if retry_needs_repaint {
                self.mark_needs_full_repaint_for(producer);
            }
            // Still called unconditionally for every `retry_needed` cause,
            // retryable pipeline `Errored` included: unlike the pre-frame success arm
            // (which runs synchronously right before this same call's own
            // `render_frame`, so nothing external needs poking),
            // every cause here fails INSIDE this call — the retry can only
            // happen on a LATER wake, so the platform still needs the poke
            // `wake_frame()` provides regardless of whether a repaint was
            // also marked above.
            self.wake_frame();
        } else {
            self.mark_rendered();
        }

        if replay_committed_input {
            self.replay_committed_held_pointer_input(producer);
        }

        // Animation continuation wake — deliberately placed AFTER
        // `mark_rendered()`/the retry check above, not inside
        // `draw_frame_entered`'s vsync loop where an earlier version of
        // this method raised it. Reason, found by a production-sequence
        // probe rather than assumed:
        // `draw_frame_entered`'s own tick already ran (this method called
        // it above), so `presentation.vsync().has_running()` here reads
        // the SAME post-tick state the ticker's should-schedule-tick check
        // reads — but `mark_rendered()` unconditionally clears
        // `needs_redraw` to `false`, and a `wake_frame()` call made BEFORE
        // that point (inside `draw_frame_entered`) gets silently clobbered
        // within this SAME callback. On the real desktop path, where the
        // next platform callback's own dirty gate (`wake_action`, in
        // `runner.rs`) reads `needs_redraw()`/`has_pending_work()` fresh —
        // neither of which a bare running controller with no other
        // tree-visible effect keeps true — that clobber is a genuine,
        // silent stall: the controller never receives another tick after
        // its anchor frame.
        //
        // Marks a fresh `Animation` demand here (this pump's own mask was
        // already cleared by the produce above) specifically so
        // `try_arm_redraw_request` has something nonempty to arm — this is
        // demand for the NEXT pump, a different concern from the
        // before-tick mark in `draw_frame_entered` that feeds THIS pump's
        // own segment. Still edge-triggered (coalesces repeated callbacks
        // under backpressure into one wake, same as before this move) —
        // only the ordering relative to `mark_rendered()` changed.
        for presentation in self.presentations.iter() {
            if self.scheduler.frames_enabled()
                && presentation.text_preparation.borrow().segment.is_none()
                && !presentation.clock().is_hidden()
                && presentation.vsync().has_running()
                && !presentation.motion_is_paused()
            {
                presentation.clock().mark_demand(DemandKind::Animation);
                if presentation.clock().try_arm_redraw_request() {
                    self.wake_frame();
                }
            }
        }

        // Last, so a panicking chrome callback skips none of the frame's own
        // obligations; this frame's mark_rendered must not erase accepted
        // sampling debt either, so the wakes still run before it resumes.
        let delivery_panic = if deliver_overlay_style {
            catch_unwind(AssertUnwindSafe(|| {
                producer.with_window(|window| producer.overlay_style.deliver(window));
            }))
            .err()
        } else {
            None
        };
        let wake_panic = self.wake_pending_pointer_samples();
        self.wake_for_late_demand(retry_needed);
        match (delivery_panic, wake_panic) {
            (Some(first), later) => {
                if let Some(later) = later {
                    flui_foundation::panic::retain_opaque_payload(later);
                }
                resume_unwind(first);
            }
            (None, Some(payload)) => resume_unwind(payload),
            (None, None) => {}
        }
        presented
    }

    /// Ask for the next frame when this one raised demand its pipeline will
    /// not serve.
    ///
    /// Two kinds arrive after a presentation's segment has consumed its dirt
    /// and so reach no host gate on their own: a redraw request made after the
    /// segment took the presentation's redraw bit (an orphaned focus, a
    /// semantics subscriber that panicked), which [`Self::mark_rendered`]
    /// would otherwise clear; and a visual update the scheduler declined
    /// because the frame was still in its pipeline slot (a hover callback's
    /// state change, a paint-time mark), which leaves the tree dirty with no
    /// frame scheduled. A frame that already armed its retry needs neither,
    /// and a presentation waiting out a dropped frame's backoff is woken by
    /// its retry deadline instead.
    fn wake_for_late_demand(&self, retry_needed: bool) {
        let late_visual_update = self.scheduler.take_late_visual_update();
        if retry_needed {
            return;
        }
        let late = self.presentations.iter().any(|presentation| {
            presentation.failure_retry_at().is_none()
                && !presentation.device_abandoned()
                && (presentation.redraw_requested_since_watch()
                    || (late_visual_update && presentation.has_pending_work_snapshot()))
        });
        if late {
            self.wake_frame();
        }
    }

    fn wake_pending_pointer_samples(&self) -> Option<Box<dyn std::any::Any + Send>> {
        let mut first = None;
        for presentation in self.presentations.iter() {
            if presentation.gestures().has_pending_pointer_samples() {
                let wake =
                    catch_unwind(AssertUnwindSafe(|| self.request_redraw_for(presentation))).err();
                super::input::preserve_first_input_panic(
                    &mut first,
                    wake,
                    "pointer sample presentation wake",
                );
            }
        }
        first
    }

    /// Finalize and record this pump's [`flui_scheduler::FrameSnapshot`] for
    /// `presentation`, IF a segment actually ran THIS PUMP for
    /// `presentation` specifically (`PresentationState::
    /// take_last_segment_span` returns `Some`) — a presentation whose
    /// segment was skipped entirely this pump (`draw_frame_entered`'s own
    /// gate) has nothing to record.
    ///
    /// `presentation` must be the exact presentation whose segment produced
    /// the outcome being submitted this call — [`Self::render_frame`]
    /// resolves this from `draw_frame_entered`'s own returned producer id,
    /// never `self.presentations.primary()` unconditionally. The bug this
    /// guards against: with more than one presentation mounted, a pump where
    /// the primary's segment is skipped (nothing dirty) while a secondary's
    /// produces used to still record against `primary()`; because the old
    /// per-clock segment-span latch was never cleared once set, a primary
    /// that HAD produced on some EARLIER pump still read `Some` here (stale,
    /// from that earlier pump, not this one), so the guard below wrongly
    /// passed — recording a `FrameSnapshot` with the primary's stale span, a
    /// duplicate `frame_id` (the primary's own `produced_count` unchanged
    /// since it didn't poll-produce this pump), and the primary's own
    /// pending input epochs drained into a frame it never produced. Reading
    /// and clearing PER-PRESENTATION via `take_last_segment_span` (rather
    /// than a shared, never-cleared `get`) and resolving the correct
    /// producer together close this: a presentation not addressed here never
    /// has its span read, and the addressed one's stale cross-pump reads are
    /// impossible because the span is cleared the moment it is used.
    ///
    /// Called only from the branches of [`Self::render_frame`] that
    /// reached a real submit attempt (never for `Ok(false)`/no-damage, whose
    /// pending input epochs must stay buffered for a later, real submit).
    ///
    /// `epochs` decides whether this presentation's pending input epochs are
    /// drained into the recorded snapshot ([`EpochDisposition::Drain`], the
    /// common case) or merely read from without consuming them
    /// ([`EpochDisposition::Retain`]) — see that type's own doc for why a
    /// submit failure that arms a retry must use `Retain`: draining on the
    /// failed attempt would leave the eventual retry's own real submit with
    /// nothing pending to attribute to the frame that actually reaches the
    /// screen.
    ///
    /// `frame_time` is the pump's frame time, recorded as the snapshot's
    /// `clock_timestamp` so a late segment start shows against it.
    pub(super) fn record_submit_telemetry(
        &self,
        frame_time: Instant,
        presentation: &PresentationState,
        submit_at: Instant,
        present_outcome: PresentOutcome,
        epochs: EpochDisposition,
    ) {
        if let Some((segment_start, segment_end, phases)) = presentation.take_last_segment_span() {
            let times = FrameTimes {
                clock_timestamp: frame_time,
                segment_start,
                segment_end,
                submit_at,
                phases,
            };
            let address = flui_foundation::PresentationAddress {
                ui_runtime_id: self.id,
                presentation_id: presentation.id(),
            };
            let snapshot = match epochs {
                EpochDisposition::Drain => {
                    presentation
                        .clock()
                        .record_frame(address, times, present_outcome)
                }
                EpochDisposition::Retain => presentation.clock().record_frame_retaining_epochs(
                    address,
                    times,
                    present_outcome,
                ),
            };
            Self::emit_frame_telemetry(presentation, &snapshot);
        }
    }

    fn emit_frame_telemetry(presentation: &PresentationState, snapshot: &FrameSnapshot) {
        if !tracing::enabled!(target: "flui.frame", tracing::Level::TRACE) {
            return;
        }

        let outcome = match snapshot.present_outcome {
            PresentOutcome::Presented => "presented",
            PresentOutcome::Errored => "errored",
            _ => "unknown",
        };
        let input_latency_max_us = snapshot
            .latencies()
            .map(|(_, latency)| Self::duration_micros_saturated(latency))
            .max()
            .unwrap_or(0);

        tracing::trace!(
            target: "flui.frame",
            event = "frame_telemetry",
            ui_runtime_id = snapshot.address.ui_runtime_id.as_u64(),
            presentation = %snapshot.address.presentation_id,
            frame_id = %snapshot.frame_id,
            present_outcome = outcome,
            segment_us = Self::duration_micros_saturated(snapshot.segment_span()),
            build_us = Self::duration_micros_saturated(snapshot.phase_duration(FramePhase::Build)),
            layout_us =
                Self::duration_micros_saturated(snapshot.phase_duration(FramePhase::Layout)),
            paint_us = Self::duration_micros_saturated(snapshot.phase_duration(FramePhase::Paint)),
            composite_us =
                Self::duration_micros_saturated(snapshot.phase_duration(FramePhase::Composite)),
            produce_to_present_us = Self::duration_micros_saturated(
                snapshot.submit_at.saturating_duration_since(snapshot.clock_timestamp)
            ),
            input_count = snapshot.input_epochs.len(),
            input_latency_max_us,
            input_epochs_overflowed = snapshot.input_epochs.overflowed(),
            produces_deferred = presentation.clock().produces_deferred(),
            frames_dropped = presentation.frames_dropped(),
            "frame telemetry"
        );
    }

    /// Takes `web_time::Duration` to match the telemetry API it consumes.
    /// That is the same type as `std::time::Duration` on every target —
    /// `web_time` re-exports `std::time::*` and overrides only `Instant` and
    /// `SystemTime` — so this is a readability choice, not a portability one.
    fn duration_micros_saturated(duration: web_time::Duration) -> u64 {
        u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
    }
}

/// Every presentation's text-store commits, closed for one frame drive.
///
/// Dropping it restores each presentation's effective barrier, retaining a
/// native preparation wait on normal return or unwind. Queued grants run at the
/// anchor after the drive, never from a destructor. A rejected nested drive
/// restores the enclosing transaction instead of reopening its commits.
struct TextCommitsClosed<'a> {
    first: Option<(&'a PresentationState, bool)>,
    remaining: Vec<(&'a PresentationState, bool)>,
}

impl<'a> TextCommitsClosed<'a> {
    fn close(ui_runtime: &'a UiRuntime) -> Self {
        let mut previous = ui_runtime.presentations.iter().map(|presentation| {
            // Admission precedes this guard. A preexisting native wait is not
            // an enclosing frame transaction: completing that wait must reopen
            // commits instead of restoring the old preparation barrier.
            let retained = presentation
                .text_preparation
                .borrow()
                .holds_document_barrier();
            (
                presentation,
                presentation.text_input().is_transaction_open() && !retained,
            )
        });
        let first = previous.next();
        let remaining: Vec<_> = previous.collect();
        for (presentation, _) in first.iter().chain(&remaining) {
            presentation.text_input().set_transaction_open(true);
        }
        Self { first, remaining }
    }
}

impl Drop for TextCommitsClosed<'_> {
    fn drop(&mut self) {
        for (presentation, open) in self.first.iter().chain(&self.remaining) {
            let retained = presentation
                .text_preparation
                .borrow()
                .holds_document_barrier();
            presentation
                .text_input()
                .set_transaction_open(*open || retained);
        }
    }
}

/// Splits one segment's time into build, layout and paint on the
/// presentation's frame clock, recording the split after every stage so a
/// segment that fails part-way still reports what it spent.
struct PhaseStopwatch<'a> {
    presentation: &'a PresentationState,
    last: Instant,
    phases: FramePhaseDurations,
}

impl<'a> PhaseStopwatch<'a> {
    fn start(presentation: &'a PresentationState) -> Self {
        let phases = FramePhaseDurations::default();
        presentation.record_segment_phases(phases);
        Self {
            presentation,
            last: presentation.clock().now(),
            phases,
        }
    }

    /// Charge the time since the previous lap to `phase`.
    fn lap(&mut self, phase: FramePhase) {
        let now = self.presentation.clock().now();
        let spent = now.saturating_duration_since(self.last);
        self.last = now;
        let slot = match phase {
            FramePhase::Build => &mut self.phases.build,
            FramePhase::Layout => &mut self.phases.layout,
            FramePhase::Paint => &mut self.phases.paint,
            _ => return,
        };
        *slot = slot.saturating_add(spent);
        self.presentation.record_segment_phases(self.phases);
    }
}
