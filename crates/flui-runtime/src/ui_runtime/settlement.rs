//! What a frame handed to a sink owes once it settles, and the settlement that pays it: a frame a
//! sink answers inline settles at once, a frame it queues settles later by its stamp
//! (ADR-0305 §4).

use std::rc::Rc;

use flui_foundation::FrameEpoch;
use flui_foundation::panic::FirstPanic;
use flui_interaction::HapticCut;
use flui_layer::SceneVersionId;
use flui_platform::SystemUiOverlayStyle;
use flui_platform::input_regions::InputRegion;
use flui_scheduler::{FramePhaseDurations, Instant, PresentOutcome};
use std::panic::resume_unwind;

use super::{EpochDisposition, MAX_NOT_SHOWN_RETRIES, StagedLayerRead, UiRuntime};
use crate::epoch::{FrameCommitState, TreeRevision};
use crate::frame_failure::FrameFailureKind;
use crate::presentation::PresentationState;
use crate::sink::{FrameSettlement, Settled, SubmitVerdict};

/// The most frames one presentation keeps pending: the deepest placement's settlement ring
/// (ADR-0305 §5). A sink that queues one more has let a settlement go, and the oldest record is
/// folded into the next, as a superseded frame's is.
const MAX_PENDING_FRAMES: usize = 2;

/// What a frame commits once its sink's verdict is known, captured when the frame is handed to
/// the sink, so a settlement that arrives later commits the frame it names and not the tree as it
/// is by then.
pub(crate) struct PendingFrame {
    /// The stamp the frame's settlement names.
    epoch: FrameEpoch,
    /// The frame time of the pump that painted the frame.
    frame_time: Instant,
    /// The span and phases of the segment that painted the frame, for its telemetry.
    segment: Option<(Instant, Instant, FramePhaseDurations)>,
    /// The scene version the sink was handed, acknowledged once it is shown.
    shown: SceneVersionId,
    /// The tree revision the frame was painted from.
    tree_revision: TreeRevision,
    /// The input regions of the frame, which the window gets once the frame is shown.
    input_regions: Rc<[InputRegion]>,
    /// The layer reads parked for the frame's scene, read from it.
    layer_reads: Vec<StagedLayerRead>,
    /// The frame-aligned haptic requests the frame confirms.
    haptics: HapticCut,
    /// The system-bar style the frame's scene asks for, delivered once the frame is shown.
    overlay_style: Option<SystemUiOverlayStyle>,
}

impl PendingFrame {
    /// How many layer reads the frame holds.
    pub(crate) fn staged_layer_reads(&self) -> usize {
        self.layer_reads.len()
    }

    /// The layer reads the frame holds, which still owe a reply.
    pub(crate) fn into_layer_reads(self) -> Vec<StagedLayerRead> {
        self.layer_reads
    }
}

/// What a frame's verdict leaves for the rest of the frame's tail, and the first panic held on
/// the way.
pub(super) struct VerdictTail {
    pub(super) presented: bool,
    pub(super) retry_needed: bool,
    pub(super) retry_needs_repaint: bool,
    pub(super) replay_committed_input: bool,
    pub(super) overlay_style: Option<SystemUiOverlayStyle>,
    pub(super) held: FirstPanic,
}

impl VerdictTail {
    fn new(held: FirstPanic) -> Self {
        Self {
            presented: false,
            retry_needed: false,
            retry_needs_repaint: false,
            replay_committed_input: false,
            overlay_style: None,
            held,
        }
    }
}

impl UiRuntime {
    /// Captures what `producer`'s frame of `shown` owes once its verdict is known, before the
    /// frame is handed to its sink: the layer reads parked for the scene are read from it here,
    /// and count against the parking bound until [`Self::land_pending_frame`].
    pub(super) fn capture_pending_frame(
        producer: &PresentationState,
        frame_time: Instant,
        scene: &flui_layer::SceneVersion,
    ) -> PendingFrame {
        let layer_reads = producer.stage_agent_layer_reads(scene);
        producer.layer_reads_in_flight.set(layer_reads.len());
        PendingFrame {
            epoch: FrameEpoch::ZERO,
            frame_time,
            segment: producer.take_last_segment_span(),
            shown: scene.id(),
            tree_revision: producer.tree_revision(),
            input_regions: producer.staged_input_regions(),
            layer_reads,
            haptics: producer.haptics().frame_cut(),
            overlay_style: producer.overlay_style.take_staged(),
        }
    }

    /// The frame being handed to `producer`'s sink came back from the call: its layer reads are
    /// counted where it goes next (its record, the parked list, or their replies).
    pub(super) fn land_pending_frame(producer: &PresentationState) {
        producer.layer_reads_in_flight.set(0);
    }

    /// The sink never answered (it panicked, or a subscriber stopped the submit before it was
    /// reached): the layer reads the frame holds wait for the next scene.
    pub(super) fn abandon_pending_frame(producer: &PresentationState, frame: PendingFrame) {
        producer.restore_staged_agent_layer_reads(frame.layer_reads);
    }

    /// The sink queued `frame`, stamped `epoch`: its record waits for the settlement that names
    /// it. A presentation already holding [`MAX_PENDING_FRAMES`] folds its oldest record into the
    /// next, so the reads it holds wait for a later scene.
    pub(super) fn queue_pending_frame(
        producer: &PresentationState,
        epoch: FrameEpoch,
        mut frame: PendingFrame,
    ) {
        frame.epoch = epoch;
        let folded = {
            let mut pending = producer.pending_frames.borrow_mut();
            let folded = (pending.len() >= MAX_PENDING_FRAMES)
                .then(|| pending.pop_front())
                .flatten();
            pending.push_back(frame);
            folded
        };
        if let Some(folded) = folded {
            tracing::warn!(
                target: "flui.frame",
                epoch = folded.epoch.get(),
                "BUG: a sink queued more frames than its settlement ring holds; the oldest is folded"
            );
            Self::abandon_pending_frame(producer, folded);
        }
    }

    /// Settles one frame a sink queued (ADR-0305 §4): the frame whose record `settlement`'s
    /// address and epoch name commits what it captured when it was submitted, by the same rules
    /// an inline verdict follows. Frames of the same presentation queued before it and not
    /// settled were superseded on the raster side, so their records fold into this one. A
    /// settlement no pending record matches (another runtime's, an earlier life's, one already
    /// settled, or `Queued`, which no settlement carries) is [`Settled::Stale`], traced and
    /// ignored.
    ///
    /// The host calls it on the owner thread, with no borrow of its own held: the settlement
    /// runs code the runtime does not own (layer read replies, the window's input regions, a
    /// haptic actuator, a `tracing` subscriber). The first panic of that code resumes after the
    /// settlement's own obligations (the retry, the hover refresh) have run.
    pub fn settle_frame(&mut self, settlement: FrameSettlement) -> Settled {
        self.enter(|ui_runtime| ui_runtime.settle_frame_entered(settlement))
    }

    fn settle_frame_entered(&self, settlement: FrameSettlement) -> Settled {
        let FrameSettlement {
            address,
            epoch,
            verdict,
        } = settlement;
        let stale = |why: &'static str| {
            tracing::trace!(
                target: "flui.frame",
                epoch = epoch.get(),
                why,
                "stale frame settlement ignored"
            );
            Settled::Stale
        };
        if verdict == SubmitVerdict::Queued {
            return stale("a settlement never carries Queued");
        }
        if address.ui_runtime_id != self.id {
            return stale("another runtime's frame");
        }
        let Some(producer) = self.presentations.get(address.presentation_id) else {
            return stale("its presentation is gone");
        };
        let found = {
            let mut pending = producer.pending_frames.borrow_mut();
            pending
                .iter()
                .position(|frame| frame.epoch == epoch)
                .map(|index| {
                    let superseded = pending.drain(..index).collect::<Vec<PendingFrame>>();
                    let frame = pending
                        .pop_front()
                        .expect("BUG: the matching record was just found");
                    (superseded, frame)
                })
        };
        let Some((superseded, frame)) = found else {
            return stale("no pending frame has its epoch");
        };
        // A superseded frame's scene never reaches the screen: what it read waits for the next.
        for superseded in superseded {
            Self::abandon_pending_frame(producer, superseded);
        }
        let tail = self.settle_pending_frame(producer, frame, verdict, FirstPanic::new());
        self.finish_settlement(producer, tail);
        Settled::Applied
    }

    /// The tail of a frame settled after its pump: the retry its verdict arms, the hover refresh
    /// over the tree it committed, the input held for that commit, and the system bars' style.
    fn finish_settlement(&self, producer: &PresentationState, tail: VerdictTail) {
        let VerdictTail {
            retry_needed,
            retry_needs_repaint,
            replay_committed_input,
            overlay_style,
            mut held,
            ..
        } = tail;
        if producer.frame_commit_state() == FrameCommitState::Committed
            && !producer.gestures().is_hover_frozen()
        {
            held.absorb(Self::refresh_hover_of(producer));
        }
        held.contain(|| {
            if retry_needed {
                if retry_needs_repaint {
                    self.mark_needs_full_repaint_for(producer);
                }
                self.wake_frame();
            }
        });
        if let Some(payload) = held.into_payload() {
            resume_unwind(payload);
        }
        if replay_committed_input {
            self.replay_committed_held_pointer_input(producer);
        }
        if let Some(style) = overlay_style {
            producer.with_window(|window| producer.overlay_style.deliver(window, style));
        }
    }

    /// Applies `verdict` to `frame`, the frame `producer` handed its sink: the commit, the
    /// replies, the telemetry and the retry each verdict owes. `held` carries a panic held
    /// before the verdict was known (a subscriber that failed as the `submit` span closed); the
    /// first panic of the work after the commit joins it in the returned tail, for the caller to
    /// resume once the retry the verdict armed is in place.
    pub(super) fn settle_pending_frame(
        &self,
        producer: &PresentationState,
        frame: PendingFrame,
        render_verdict: SubmitVerdict,
        mut held: FirstPanic,
    ) -> VerdictTail {
        let PendingFrame {
            epoch: _,
            frame_time,
            segment,
            shown,
            tree_revision,
            input_regions,
            layer_reads,
            haptics,
            overlay_style,
        } = frame;
        // The frame this scene becomes once presented.
        let frame_number = producer.frames_rendered() + 1;
        let shown_on_screen = matches!(
            render_verdict,
            SubmitVerdict::Presented | SubmitVerdict::NoPresent
        );
        // A shown version is the raster side's new base: the log before it is let go.
        // What the log held drops at the end of this function, after the replies and the
        // commit, with no borrow held: an annotation's `Drop` is user code.
        let released = shown_on_screen
            // A scene lent to a frame still producing it misses this acknowledgement:
            // the next publication then over-reports damage, never under-reports it.
            .then(|| producer.scene.completed(shown).ok());
        // Before anything else that can panic: the frame's layer reads are answered if the
        // window showed the scene, and parked again if it did not.
        held.absorb(producer.settle_agent_layer_reads(layer_reads, shown_on_screen));
        let mut tail = VerdictTail::new(FirstPanic::new());
        // The first panic of the work that follows the submit is kept beside a layer read's
        // reply panic (`held`), so neither replaces the other.
        let mut post_submit = FirstPanic::new();
        post_submit.contain(|| {
            // Telemetry: sampled AFTER the submit returns, never before
            // it -- a `Presented` verdict means the backend called
            // `output.present()`, and whatever pacing block that call carries
            // happens inside it rather than after it (under the default
            // `Fifo` mode the block is a wait for the next vsync on the
            // Vulkan/Wayland path; the native AppKit backend returns from the
            // present in ~42 µs — ADR-0058's per-backend facts. On the inline
            // executor its turn runs synchronously inside `submit`, so the
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
                    held.absorb(self.commit_painted_frame(producer, tree_revision, &input_regions));
                    // What the tail owes is recorded before the code below that the runtime does
                    // not own (a haptic backend, telemetry, a subscriber) can panic.
                    tail.overlay_style = overlay_style;
                    tail.presented = true;
                    tail.replay_committed_input = true;
                    producer.record_frame_rendered();
                    // The surface is demonstrably available: any withheld
                    // streak ends here, so a later withdrawal gets the full
                    // retry budget again.
                    producer.clear_not_shown_streak();
                    // The frame reached `present()`: frame-aligned haptics
                    // play with it (ADR-0210 §4). Only this arm plays them;
                    // the others leave the queue to its deadline.
                    producer.haptics().frame_presented(haptics);
                    self.record_submit_telemetry(
                        frame_time,
                        segment,
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
                }
                SubmitVerdict::NoPresent => {
                    held.absorb(self.commit_painted_frame(producer, tree_revision, &input_regions));
                    // No frame-aligned haptics: the backend found no damage,
                    // so the screen kept the previous frame and the change a
                    // queued request confirms never showed. A request queued
                    // before that previous frame already resolved with it; the
                    // rest resolve `Suppressed` at their deadline.
                    tail.overlay_style = overlay_style;
                    tail.replay_committed_input = true;
                    // Nothing was owed and nothing was lost — the surface was
                    // never asked to show anything, so a withheld streak is
                    // over just as it is on a present.
                    producer.clear_not_shown_streak();
                    tracing::trace!(
                        frame = frame_number,
                        "Frame skipped: no damage (no present)"
                    );
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
                    held.absorb(self.commit_painted_frame(producer, tree_revision, &input_regions));
                    producer.record_frame_dropped();
                    let streak = producer.record_frame_withheld();
                    tail.replay_committed_input = true;
                    if streak <= MAX_NOT_SHOWN_RETRIES {
                        // Armed before the trace: a subscriber's panic there keeps the retry.
                        tail.retry_needed = true;
                        tail.retry_needs_repaint = true;
                        tracing::debug!(
                            frame = frame_number,
                            streak,
                            "Frame rendered but never shown (surface unavailable); \
                         retaining it — retry armed via wake_frame()"
                        );
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
                    // The failed submit consumed painting work. A wake alone
                    // cannot repaint a clean static tree, so retain its epochs
                    // and dirty the producer for the ordinary paced retry,
                    // armed before the telemetry's subscriber can panic.
                    tail.retry_needed = true;
                    tail.retry_needs_repaint = true;
                    self.record_submit_telemetry(
                        frame_time,
                        segment,
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Retain,
                    );
                    tracing::debug!(
                        ?render_verdict,
                        "Transient render failure; paced retry armed"
                    );
                }
                SubmitVerdict::DeviceLost => {
                    producer.record_frame_dropped();
                    // The submit consumed the paint demand, so the frame the
                    // rebuilt device shows needs a full repaint whenever it
                    // comes; marked before the telemetry's subscriber can panic.
                    self.mark_needs_full_repaint_for(producer);
                    // `Retain`, not `Drain`: the frame is tried again, and the
                    // eventual real submit must still find the epochs that
                    // arrived before this failure — see
                    // `record_submit_telemetry`'s own doc.
                    self.record_submit_telemetry(
                        frame_time,
                        segment,
                        producer,
                        submit_at,
                        PresentOutcome::Errored,
                        EpochDisposition::Retain,
                    );
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
                        segment,
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
                SubmitVerdict::Queued => {
                    unreachable!("BUG: a queued frame is applied only by its settlement")
                }
            }
        });
        // What the log held drops here, after the replies and the commit, with no borrow
        // held, beside the first panic of the work above; what nobody else holds goes back
        // to the scene's owner.
        if let Some(Some(released)) = released {
            post_submit.contain(|| producer.scene.restock(released));
        }
        // A panic of the work after the commit is held behind the first one, a layer read's reply
        // included: the caller resumes it once the retry the verdict armed is in place.
        held.absorb(post_submit);
        tail.held = held;
        tail
    }
}
