//! Frame production: draw, paint, commit, render and frame telemetry.

use super::{EpochDisposition, FramePaintOutcome, MAX_NOT_SHOWN_RETRIES, UiRealm};
use crate::app::epoch::FrameCommitState;
use crate::app::frame_failure::{FrameFailureKind, SegmentPhase};
use crate::app::held_input::HeldPointerReplay;
use crate::app::presentation::PresentationState;
use flui_engine::RasterBackend;
use flui_foundation::PresentationId;
use flui_layer::Scene;
use flui_rendering::binding::RendererBinding as _;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_scheduler::{DemandKind, FrameSnapshot, Instant, PresentOutcome};
use flui_types::Size;
use flui_types::geometry::px;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

impl UiRealm {
    // ========================================================================
    // Frame production (moved from the retired `AppBinding`)
    // ========================================================================

    /// Draw a frame and return the produced `Scene`, if any. Test-only —
    /// production drives frames through [`Self::render_frame_entered`].
    #[cfg(test)]
    pub(crate) fn draw_frame(&self, constraints: BoxConstraints) -> Option<Scene> {
        match self.enter(|realm| realm.draw_frame_entered(constraints)).1 {
            // (the third tuple element, `any_failed`, is a retry-arming
            // concern for `render_frame_entered`; this test helper only
            // reports what was painted)
            FramePaintOutcome::Painted(scene) => Some(scene),
            FramePaintOutcome::Idle | FramePaintOutcome::Errored => None,
        }
    }

    /// The complete build+layout+paint pipeline for one frame.
    ///
    /// **Frame-phase parity (critical):** the realm-level pre-phase (vsync
    /// tick, then gesture-deadline tick) MUST precede every presentation's
    /// own segment, for the identical ordering argument the retired
    /// `AppBinding::draw_frame_entered` depended on: both tick calls can
    /// dirty render/build state a segment's dirty sample needs to observe.
    /// Reordering the pre-phase is out of scope for the change that
    /// introduced the per-presentation loop below; the frame-loop tests are
    /// the parity oracle.
    ///
    /// **Per-presentation segment loop (ADR-0043 §3; issue #556):**
    /// presentations are processed once each, in mount order. Each
    /// presentation's own [`FrameClock`](flui_scheduler::FrameClock) —
    /// replacing the old `take_redraw_pending() || has_pending_work()`
    /// predicate read directly — decides whether its segment runs this pump.
    /// The clock never inspects the tree itself (ownership rule: it never
    /// knows phases, callbacks, or element trees), so the exact union the
    /// old predicate read is marked as `Dirty` demand and the clock's
    /// `poll` makes the call: `PollDecision::should_run_segment` is
    /// EXACTLY that same `woken || has_pending_work()` union (hidden/
    /// backpressure aside — unwired in production today), independent of
    /// first-frame deferral, which never gates the segment (see
    /// `FrameClock`'s own module doc's `.flutter/` citation — deferral
    /// withholds only the submit). Production can host multiple
    /// presentations, but secondary windows carry no widget content today,
    /// so only the primary can produce painted output; see
    /// [`Self::draw_frame_for_presentation`]'s doc for the proof that the
    /// gate cannot skip a segment the old, ungated code would have run.
    /// Returns the last presentation whose segment ran, paired with its
    /// outcome. This aggregate is not a multi-surface submit contract: it
    /// retains only one scene and receives one constraints set. Production
    /// secondary windows are currently contentless, so at most one
    /// presentation can paint in a pump. Supporting simultaneous paintable
    /// presentations requires per-presentation constraints, sinks, and
    /// submit routing under issue #559; callers must not treat the current
    /// last-outcome tuple as last-scene-wins behavior.
    pub(super) fn draw_frame_entered(
        &self,
        constraints: BoxConstraints,
    ) -> (PresentationId, FramePaintOutcome, bool) {
        // Vsync tick + gesture-deadline tick — MUST precede every
        // presentation's build phase. See the retired `AppBinding::
        // draw_frame_entered`'s doc for the full disjoint-controller-set
        // argument this ordering depends on. Each presentation ticks its OWN
        // registry now (the registry moved off this realm's former
        // `vsync_slot`, one per surface), off the SAME realm-relative `now`
        // every presentation observes, so there is no clock drift between
        // siblings sharing this one pump.
        let now = self.now_secs();
        for presentation in self.presentations.iter() {
            let vsync = presentation.vsync();
            // Sampled BEFORE `tick_all`, not after: the tick that completes
            // a controller still delivers that controller's final value and
            // status change (`.flutter/packages/flutter/lib/src/scheduler/
            // ticker.dart:272-285`'s `_tick` calls `_onTick` unconditionally,
            // THEN checks whether to schedule another one), so a controller
            // that WAS running when this tick started must still mark
            // demand for THIS exact pump even though it may have just
            // settled -- the identical before/after-tick question
            // `flui-testing`'s own `pump_presentation` had to fix (see its
            // doc), just never wired at this layer before this slice.
            //
            // The "does the NEXT pump need to be scheduled" question
            // (Flutter's own `shouldScheduleTick`, sampled AFTER the tick)
            // is answered separately, in `render_frame_entered`, AFTER the
            // segment this demand mark feeds has actually rendered and
            // `mark_rendered()`/the retry check has run -- NOT here. See
            // that method's own comment for why: raising the continuation
            // wake here, before this same pump's `mark_rendered()` clears
            // `needs_redraw`, silently clobbers it within the SAME
            // callback -- a real, probe-confirmed stall (not hypothetical)
            // for a controller with no other tree-visible effect, since
            // nothing else keeps `needs_redraw`/`has_pending_work()` true
            // once that clobber happens.
            let was_running = vsync.has_running();
            vsync.tick_all(now);

            if was_running {
                // A running controller with no OTHER tree-visible effect
                // now genuinely marks demand -- and therefore flushes --
                // every tick, matching Flutter's own `Ticker`-driven
                // `scheduleFrame` (the running ticker alone is sufficient),
                // rather than silently relying on some unrelated dirty
                // state to also be present. This closes a gap the previous
                // slice on this issue explicitly named and deferred.
                presentation.clock().mark_demand(DemandKind::Animation);
            }

            presentation.gestures().tick_deadlines();
            if presentation.gestures().has_pending_deadlines() {
                self.wake_frame();
            }
        }

        // The async-driver step lives in `UpdateScheduler::handle_begin_frame`'s
        // mid-frame slot, not here: this pipeline runs in
        // `PersistentCallbacks`, where `drive_async_tasks` debug-asserts it
        // must never poll (polling here could re-enter a frame-phase-only
        // capability from inside a woken future). One mid-frame poll per
        // frame, on the right `UpdateScheduler` instance, is enforced by the
        // scheduler itself.

        let mut last_outcome = FramePaintOutcome::Idle;
        let mut producer = self.presentations.primary().id();
        // Whether ANY presentation's segment failed THIS pump (a pipeline
        // error or a boundary-caught panic) — returned separately from
        // `last_outcome`, which only ever describes the LAST segment that
        // ran: with more than one presentation mounted, an earlier
        // presentation's failure followed by a sibling's clean `Painted`
        // must still arm the caller's retry instead of letting the pump
        // settle as rendered (`mark_rendered()` would clear the wake the
        // failure needs).
        let mut any_failed = false;
        for presentation in self.presentations.iter() {
            // Platform-driven semantics enablement lands here, BEFORE dirty
            // sampling: the activation listener could only flip the host
            // flag and wake the loop (it runs on the adapter's thread), and
            // enabling seeds the root as needing semantics — exactly the
            // pending work the sampling below should then observe.
            presentation.reconcile_semantics_enablement();
            // Segment start: clear this presentation's wake bit BEFORE
            // sampling dirty, so a mark arriving WHILE this segment runs
            // sets the bit again and lands next pump instead of being lost.
            // The union is fed into the clock as `Dirty` demand rather than
            // gating directly — the clock decides, the realm only reports.
            let woken = presentation.take_redraw_pending();
            if woken || presentation.has_pending_work() {
                presentation.clock().mark_demand(DemandKind::Dirty);
            }

            let segment_start = presentation.clock().now();
            let decision = presentation.clock().poll(segment_start);
            // `flui.pace`: the per-presentation produce decision, the one
            // fact a pacing investigation needs next to the present trace
            // (a wake that ran no segment must never be paced as if it had
            // rendered and failed to present — ADR-0058).
            tracing::trace!(
                target: "flui.pace",
                event = "segment_poll",
                presentation = ?presentation.id(),
                decision = ?decision,
                "presentation segment poll"
            );
            if !decision.should_run_segment() {
                // Nothing to flush and nobody asked, or a gate (hidden /
                // backpressure) is closed: skip this presentation's segment
                // entirely. Bound: each presentation builds and flushes at
                // most once per pump; this `continue` is what makes that
                // true rather than merely documented. First-frame deferral
                // is NOT one of these reasons — see `FrameClock`'s module
                // doc: deferral withholds only the submit, never the
                // segment (`.flutter/packages/flutter/lib/src/rendering/
                // binding.dart:582-599`), so a deferred-but-demanded
                // presentation always reaches `should_run_segment() ==
                // true` (`PollDecision::ProduceWithheld`) below.
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
                Self::draw_frame_for_presentation(presentation, constraints)
            }));

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
                    // A terminal clean result resets only after every
                    // contained report from this attempt was delivered.
                    presentation.reset_frame_failure_streak();
                    outcome
                }
                Ok(Err(error)) => {
                    self.report_frame_failure(presentation, FrameFailureKind::Pipeline { error });
                    FramePaintOutcome::Errored
                }
                Err(payload) => {
                    let failed_phase = presentation.segment_phase();
                    if matches!(failed_phase, SegmentPhase::Tail | SegmentPhase::Scene) {
                        // The pipeline already consumed this presentation's
                        // paint dirtiness before either post-pipeline segment
                        // began. Re-dirty the exact failed presentation here,
                        // while its identity is still local to this catch;
                        // a later clean sibling must not steal attribution.
                        self.mark_needs_full_repaint_for(presentation);
                    }
                    // Classify against the borrowed raw payload before the
                    // configured privacy policy decides whether any source
                    // text may be retained.
                    let (message, internal_invariant) =
                        self.frame_failure_detail.get().panic_text(&*payload);
                    self.report_frame_failure(
                        presentation,
                        FrameFailureKind::SegmentPanic {
                            message,
                            phase: failed_phase,
                            internal_invariant,
                        },
                    );
                    FramePaintOutcome::Errored
                }
            };
            if matches!(
                &result,
                FramePaintOutcome::Painted(_) | FramePaintOutcome::Errored
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
            // Telemetry: remember this segment's span so `render_frame_entered`
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
            // `RenderingFlutterBinding::mark_first_frame_sent`'s `!errored`
            // guard, now also excluding `ProduceWithheld`: a withheld
            // result was never sent, by construction, so confirming it as
            // sent would be self-contradictory and would wrongly disarm a
            // later `defer_first_frame` call. `render_frame_entered` below
            // separately re-checks `is_deferred()` at its own submit point
            // before honoring whatever this segment produced.
            if decision.is_produce() && !matches!(result, FramePaintOutcome::Errored) {
                presentation.clock().mark_first_frame_sent();
            }
            if matches!(result, FramePaintOutcome::Errored) {
                any_failed = true;
            }
            last_outcome = result;
            producer = presentation.id();
        }
        (producer, last_outcome, any_failed)
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
    pub(super) fn draw_frame_for_presentation(
        presentation: &PresentationState,
        constraints: BoxConstraints,
    ) -> Result<FramePaintOutcome, flui_rendering::RenderError> {
        presentation.enter_segment_phase(SegmentPhase::Build);

        #[cfg(test)]
        presentation.record_flush();

        // Phase 1: enter the widget frame unconditionally. `draw_frame`
        // already skips `build_scope` when nothing is dirty, but its frame
        // entry also discards any undrained lifecycle-panic records from the
        // preceding frame. A pipeline-only segment must still perform that
        // cleanup: lazy child service below can produce records after the
        // preceding frame's build phase, leaving no pending build to make a
        // conditional call here run on the next frame.
        presentation.widgets().draw_frame_with_phase_marker(
            presentation.segment_phase_marker(),
            SegmentPhase::Finalize,
        );

        // Phase 2 & 3: Layout, Compositing, Paint, Semantics through the
        // typestate-driven orchestrator.
        presentation.enter_segment_phase(SegmentPhase::Pipeline);
        presentation
            .renderer()
            .root_pipeline_owner()
            .with_mut(|owner| owner.set_root_constraints(Some(constraints)));
        let layer_tree = match presentation
            .widgets()
            .run_frame_with_layout_builders(presentation.pipeline())
        {
            Ok(layer_tree) => layer_tree,
            Err(error) => {
                // Paint commits the layer tree (which carries its own
                // leader index) before semantics runs; a semantics error
                // retains it for the retry.
                return Err(error);
            }
        };

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
            presentation.attach_performance_overlay(&mut layer_tree);

            // By value, not `Arc<Scene>` — see `FramePaintOutcome::Painted`'s
            // own doc for why.
            Ok(FramePaintOutcome::Painted(Scene::new(layer_tree)))
        } else {
            Ok(FramePaintOutcome::Idle)
        }
    }

    /// Commit a painted presentation after the sink accepts its scene.
    fn commit_painted_frame(&self, presentation: &PresentationState) {
        debug_assert!(
            self.presentations.get(presentation.id()).is_some(),
            "commit target must belong to this realm"
        );
        let committed_revision = presentation.commit_tree_revision();
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
        for pointer_event in replay.by_ref() {
            let dispatch = Self::dispatch_pointer_event_entered(presentation, &pointer_event);
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

    /// Render while the platform dispatcher already owns the realm entry.
    /// This keeps scheduler callbacks and the full build/layout/paint/raster
    /// transaction under one activation instead of creating a nested scope.
    ///
    /// Returns whether the frame reached `present()` — needed for the
    /// runner's no-present fallback throttle: `Fifo` present blocks every
    /// PRESENTED frame at display cadence, but a frame that never presents
    /// (nothing dirty, no damage, occluded surface, surface lost) carries no
    /// such pacing signal.
    ///
    /// Step by step: settle any lone arena member queued by an earlier event
    /// whose owner boundary could not finish (e.g. after a panic); flush
    /// coalesced pointer moves; draw the frame ([`Self::draw_frame_entered`]);
    /// classify its result and, when the actual producer is not deferred,
    /// submit a non-empty painted scene and commit an accepted verdict;
    /// re-hit-test stationary pointing devices against the primary tree only
    /// when that presentation is committed; then arm retry work or mark the
    /// pump rendered as applicable. The producer's `FrameClock::is_deferred`
    /// submit gate is DELIBERATELY
    /// separate from `draw_frame_entered`'s own segment gate: first-frame
    /// deferral withholds only the submit, never the build/layout/paint
    /// work (`.flutter/packages/flutter/lib/src/rendering/binding.dart:582-599`
    /// — `RendererBinding.deferFirstFrame`'s own doc: "the framework will
    /// still do all the work to produce frames, but those frames are never
    /// sent to the engine"), so a deferred presentation's segment can very
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
    #[cfg_attr(
        all(not(target_arch = "wasm32"), not(test)),
        expect(
            dead_code,
            reason = "the direct-sink entry point is the web runner's production frame path \
                      (wasm32) and the scripted-backend test seam; native production drives \
                      render_frame_on_lane instead -- see FrameSink's own doc"
        )
    )]
    pub(crate) fn render_frame_entered<R: RasterBackend>(&self, renderer: &mut R) -> bool {
        self.render_frame_with_sink(&mut crate::app::raster_lane::DirectSink::new(renderer))
    }

    /// [`Self::render_frame_entered`], driven through the raster mailbox:
    /// the desktop/Android runners' canonical frame path (ADR-0045's inline
    /// lane). The scene crosses the raster boundary as an owned, stamped
    /// `SceneSnapshot` and is rendered by the lane's own pump; the direct
    /// entry point above remains for the web runner and for tests that pin
    /// this transaction against scripted backends — both feed the same
    /// classification arms below, so the retry/telemetry semantics cannot
    /// drift between the two.
    #[cfg(not(target_arch = "wasm32"))]
    #[tracing::instrument(level = "debug", skip_all)]
    pub(crate) fn render_frame_on_lane<B: RasterBackend>(
        &self,
        lane: &mut crate::app::raster_lane::RasterLane<B>,
    ) -> bool {
        self.render_frame_with_sink(lane)
    }

    /// The shared frame transaction behind both entry points above. See
    /// [`Self::render_frame_entered`]'s doc for the step-by-step contract.
    fn render_frame_with_sink<S: crate::app::raster_lane::FrameSink>(&self, sink: &mut S) -> bool {
        self.gestures().drain_deferred_arena_resolutions();
        self.gestures().flush_pending_moves();

        let (width, height) = sink.surface_size();
        let dpr = self
            .presentations
            .primary()
            .renderer()
            .root_pipeline_owner()
            .with(PipelineOwner::device_pixel_ratio);
        let constraints =
            BoxConstraints::tight(Size::new(px(width as f32 / dpr), px(height as f32 / dpr)));
        let (producer_id, outcome, any_failed) = self.draw_frame_entered(constraints);
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
        // `any_failed`, not "the producer's outcome was Errored": with more
        // than one presentation mounted, an earlier presentation's failed
        // segment followed by a clean sibling `Painted` must still arm the
        // retry — keying this off the LAST outcome alone would end such a
        // pump in `mark_rendered()`, clearing the very wake the failed
        // presentation's retry needs (and on a pump whose failure consumed
        // its build-dirty state, nothing else would ever reopen the gate).
        let mut retry_needed = any_failed;
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
        use crate::app::raster_lane::SubmitVerdict;
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
                    presented = true;
                    producer.record_frame_rendered();
                    // The surface is demonstrably available: any withheld
                    // streak ends here, so a later withdrawal gets the full
                    // retry budget again.
                    producer.clear_not_shown_streak();
                    Self::record_submit_telemetry(
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
                SubmitVerdict::SurfaceStale => {
                    producer.record_frame_dropped();
                    // `Retain`, not `Drain`: this arm arms a retry
                    // (`retry_needed = true` below) via `wake_frame()`, and
                    // the eventual real submit that retry produces must
                    // still find the epochs that arrived before this
                    // failure -- see `record_submit_telemetry`'s own doc.
                    Self::record_submit_telemetry(
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Retain,
                    );
                    retry_needed = true;
                    // See `retry_needs_repaint`'s own binding above for why
                    // this arm (a genuine submit failure, not a pipeline
                    // error) is one of the arms that also re-dirties
                    // `producer`'s pipeline via
                    // `mark_needs_full_repaint_for` below.
                    //
                    // Covers a lost surface, a validation failure, and (on
                    // the raster-lane path) a stale surface-generation
                    // stamp — the lane has already restamped itself from
                    // the mailbox's required generation, so the retry armed
                    // here submits against the reconfigured surface. NOTE:
                    // the wgpu backend does NOT yet reconfigure the surface
                    // before a validation-failure retry's own acquire
                    // attempt (`Renderer::acquire_surface_texture`'s
                    // `Validation` arm gaining reconfigure-and-retry-once
                    // ships separately, issue #626) — until it lands, that
                    // flavor of armed retry re-attempts the identical
                    // acquire and may keep failing, at the runner's
                    // no-present throttle (~62 Hz), indefinitely; arming it
                    // anyway is still strictly better than a permanently
                    // dropped frame nothing ever retries. Whether that
                    // steady state deserves its own backoff (like
                    // `DeviceRecoveryBackoff` gates device recovery) is a
                    // real question this arm does not answer.
                    retry_needs_repaint = true;
                    tracing::debug!(
                        "Surface stale/lost; frame dropped — retry armed via wake_frame()"
                    );
                }
                SubmitVerdict::DeviceLost => {
                    producer.record_frame_dropped();
                    // `Retain`, not `Drain`: this arm now arms a retry
                    // (`retry_needed = true` below), and the eventual real
                    // submit that retry produces must still find the epochs
                    // that arrived before this failure — see
                    // `record_submit_telemetry`'s own doc.
                    Self::record_submit_telemetry(
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Retain,
                    );
                    // Division of labor with the platform runner's own
                    // recovery wake (`render_frame_with_device_recovery` in
                    // `runner.rs`): this arm only ever runs once
                    // `render_scene` was actually called, i.e. there was
                    // dirty content to submit — it owns the retry for a
                    // loss OBSERVED MID-FRAME, for every `RasterBackend`
                    // consumer (headless included), not just the desktop/
                    // Android runners. A loss discovered on a quiescent
                    // loop, before anything is dirty enough to reach
                    // `render_scene` at all, never reaches this arm
                    // (`draw_frame_entered` produces `Idle`, nothing to
                    // submit) — that gap is what the runner's own wake on a
                    // FAILED pre-frame recovery attempt covers instead. The
                    // two are non-overlapping, not redundant.
                    retry_needed = true;
                    // See `retry_needs_repaint`'s own binding above.
                    retry_needs_repaint = true;
                    tracing::warn!(
                        "GPU device lost; frame dropped — retry armed, recovery is \
                         attempted by the renderer owner"
                    );
                }
                SubmitVerdict::Failed => {
                    producer.record_frame_dropped();
                    Self::record_submit_telemetry(
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

        // Ambient hover derivations belong to the primary presentation and
        // may be refreshed only from a tree whose latest terminal revision
        // has been acknowledged. This runs after submit classification so a
        // just-painted primary can become committed in this same pump. A
        // secondary failure does not suppress a still-committed primary;
        // conversely, a primary failure holds its prior hover derivation.
        if self.presentations.primary().frame_commit_state() == FrameCommitState::Committed {
            self.gestures()
                .mouse_tracker()
                .update_all_devices(|position| {
                    let mut result = flui_interaction::routing::HitTestResult::new();
                    self.presentations.primary().renderer().hit_test_in_view(
                        &mut result,
                        position,
                        0,
                    );
                    result
                });
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
            any_failed,
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
            // pipeline `Errored` included: unlike the pre-frame success arm
            // (which runs synchronously right before this same call's own
            // `render_frame_entered`, so nothing external needs poking),
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
        // the SAME post-tick state Flutter's own `shouldScheduleTick`
        // reads — but `mark_rendered()` unconditionally clears
        // `needs_redraw` to `false`, and a `wake_frame()` call made BEFORE
        // that point (inside `draw_frame_entered`) gets silently clobbered
        // within this SAME callback. On the real desktop path, where the
        // next platform callback's own dirty gate (`wake_action`, in
        // `runner.rs`) reads `needs_redraw()`/`has_pending_work()` fresh —
        // neither of which a bare running controller with no other
        // tree-visible effect keeps true — that clobber is a genuine,
        // silent stall: the controller never receives another tick after
        // its anchor frame. Confirmed red before this fix, green after, by
        // `runner.rs`'s
        // `a_running_controller_with_no_other_dirty_state_keeps_producing_across_the_real_wake_action_gate`,
        // which drives the exact `record_compositor_tick` -> dirty-gate ->
        // `render_frame_entered` sequence `bootstrap_desktop`'s closure
        // uses, not `draw_frame_entered` called directly (which never
        // exercises `wake_action` at all and could not have caught this).
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
            if presentation.vsync().has_running() {
                presentation.clock().mark_demand(DemandKind::Animation);
                if presentation.clock().try_arm_redraw_request() {
                    self.wake_frame();
                }
            }
        }

        presented
    }

    /// Finalize and record this pump's [`flui_scheduler::FrameSnapshot`] for
    /// `presentation`, IF a segment actually ran THIS PUMP for
    /// `presentation` specifically (`PresentationState::
    /// take_last_segment_span` returns `Some`) — a presentation whose
    /// segment was skipped entirely this pump (`draw_frame_entered`'s own
    /// gate) has nothing to record.
    ///
    /// `presentation` must be the exact presentation whose segment produced
    /// the outcome being submitted this call — [`Self::render_frame_entered`]
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
    /// Called only from the branches of [`Self::render_frame_entered`] that
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
    pub(super) fn record_submit_telemetry(
        presentation: &PresentationState,
        submit_at: Instant,
        present_outcome: PresentOutcome,
        epochs: EpochDisposition,
    ) {
        if let Some((segment_start, segment_end)) = presentation.take_last_segment_span() {
            let snapshot = match epochs {
                EpochDisposition::Drain => presentation.clock().record_frame(
                    presentation.id(),
                    segment_start,
                    segment_start,
                    segment_end,
                    submit_at,
                    present_outcome,
                ),
                EpochDisposition::Retain => presentation.clock().record_frame_retaining_epochs(
                    presentation.id(),
                    segment_start,
                    segment_start,
                    segment_end,
                    submit_at,
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
            presentation = %snapshot.presentation,
            frame_id = %snapshot.frame_id,
            present_outcome = outcome,
            segment_us = Self::duration_micros_saturated(snapshot.segment_span()),
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
