//! What a frame handed to a sink owes once it settles, and the settlement that pays it: a frame a
//! sink answers inline settles at once, a frame it queues settles later by its stamp
//! (ADR-0305 §4).

use std::rc::Rc;

use flui_foundation::FrameEpoch;
use flui_foundation::panic::FirstPanic;
use flui_interaction::haptics::HapticCut;
use flui_layer::SceneVersionId;
use flui_platform::SystemUiOverlayStyle;
use flui_platform::input_regions::InputRegion;
use flui_scheduler::{FramePhaseDurations, FrameTelemetryCut, Instant, PresentOutcome};
use std::panic::resume_unwind;

use super::{EpochDisposition, MAX_NOT_SHOWN_RETRIES, StagedLayerRead, UiRuntime};
use crate::epoch::{FrameCommitState, TreeRevision};
use crate::frame_failure::FrameFailureKind;
use crate::presentation::PresentationState;
use crate::sink::{FrameSettlement, Settled, SubmitVerdict};

/// The most frames one presentation keeps pending. A record leaves when a settlement names it, a
/// later frame's settlement folds it, or the sink reports that a later frame replaced it in its
/// inbox ([`SubmitVerdict::QueuedReplacing`]), which folds it by its stamp (ADR-0305 §4). So a
/// sink's own depth bounds the ring: the engine's raster executor holds at most its depth of
/// frames rendering or settled and not yet delivered, and one in its inbox. A sink that answers
/// `Queued` past this bound broke that contract; the newest record, the frame it most likely
/// replaced, folds so the ring stays bounded.
pub(super) const MAX_PENDING_FRAMES: usize = 8;

/// How long the owner sleeps at most while a presentation has frames pending, so a settlement
/// whose wake was lost is drained within this interval instead of never (ADR-0305 §5).
pub(crate) const IN_FLIGHT_FLOOR: std::time::Duration = std::time::Duration::from_millis(50);

/// What a frame commits once its sink's verdict is known, captured when the frame is handed to
/// the sink, so a settlement that arrives later commits the frame it names and not the tree as it
/// is by then.
pub(crate) struct PendingFrame {
    /// The stamp the frame's settlement names.
    epoch: FrameEpoch,
    /// When the frame was handed to its sink, on its presentation's clock: what the in-flight
    /// floor counts from.
    submitted_at: Instant,
    /// What the frame's telemetry is recorded from.
    telemetry: SubmittedTelemetry,
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
    /// The frame was drawn while its window could not be seen: it is the frame the window owed
    /// its surface, which a refusal of this frame, and of no other, ends (ADR-0357).
    drawn_unseen: bool,
}

impl PendingFrame {
    /// The frame, given the stamp `epoch` the sink was handed it under.
    pub(super) fn stamped(self, epoch: FrameEpoch) -> Self {
        Self { epoch, ..self }
    }

    /// When the frame was handed to its sink.
    pub(crate) fn submitted_at(&self) -> Instant {
        self.submitted_at
    }

    /// How many layer reads the frame holds.
    pub(crate) fn staged_layer_reads(&self) -> usize {
        self.layer_reads.len()
    }

    /// The layer reads the frame holds, which still owe a reply.
    pub(crate) fn into_layer_reads(self) -> Vec<StagedLayerRead> {
        self.layer_reads
    }
}

/// What a frame's telemetry is recorded from, taken when the frame was handed to its sink: the
/// frame time of the pump that painted it, its segment's span and phases, and its cut of the
/// presentation's frame ids and input epochs.
#[derive(Debug, Clone, Copy)]
pub(super) struct SubmittedTelemetry {
    pub(super) frame_time: Instant,
    pub(super) segment: Option<(Instant, Instant, FramePhaseDurations)>,
    pub(super) cut: FrameTelemetryCut,
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
    /// frame is handed to its sink: the layer reads parked for the scene are read from it here.
    pub(super) fn capture_pending_frame(
        producer: &PresentationState,
        frame_time: Instant,
        scene: &flui_layer::SceneVersion,
    ) -> PendingFrame {
        let layer_reads = producer.stage_agent_layer_reads(scene);
        PendingFrame {
            epoch: FrameEpoch::ZERO,
            submitted_at: producer.clock().now(),
            telemetry: SubmittedTelemetry {
                frame_time,
                segment: producer.take_last_segment_span(),
                cut: producer.clock().telemetry_cut(),
            },
            shown: scene.id(),
            tree_revision: producer.tree_revision(),
            input_regions: producer.staged_input_regions(),
            layer_reads,
            haptics: producer.haptics().frame_cut(),
            overlay_style: producer.overlay_style.take_staged(),
            drawn_unseen: producer.clock().is_hidden(),
        }
    }

    /// The sink never answered (it panicked, or a subscriber stopped the submit before it was
    /// reached): the layer reads the frame holds wait for the next scene.
    pub(super) fn abandon_pending_frame(producer: &PresentationState, frame: PendingFrame) {
        let reads = frame.layer_reads.len();
        producer.restore_staged_agent_layer_reads(frame.layer_reads);
        tracing::trace!(
            target: "flui.frame",
            epoch = frame.epoch.get(),
            reads,
            "a frame was abandoned before it was shown; its layer reads wait for the next scene"
        );
    }

    /// What a frame whose verdict commits it owes the current tree: a frame that shows it replays
    /// the input held for its commit. A frame painted before a later one commits an older tree,
    /// and the input waits for the later frame's commit; when no pending frame was painted from
    /// the current tree (the later one was answered without a commit), the frame asks for a
    /// repaint so the current tree is shown, input held or not.
    fn owe_current_tree(
        producer: &PresentationState,
        shows_current_tree: bool,
        tail: &mut VerdictTail,
    ) {
        if shows_current_tree {
            tail.replay_committed_input = true;
        } else {
            let current = producer.tree_revision();
            let shown_by_later = producer
                .pending_frames
                .borrow()
                .iter()
                .any(|pending| pending.tree_revision >= current);
            if !shown_by_later && !producer.device_abandoned() {
                tail.retry_needed = true;
                tail.retry_needs_repaint = true;
            }
        }
    }

    /// The sink queued `frame`, under its stamp: its record waits for the settlement that names
    /// it. A frame queued in place of the frame stamped `replaces` folds that frame's record,
    /// found by its stamp, so the reads it holds wait for a later scene; a stamp no record has
    /// (an inline verdict or a failure folded it first) folds nothing. Into a presentation that
    /// still holds [`MAX_PENDING_FRAMES`], the sink broke its bound and the newest record folds.
    /// Returns the first panic of a fold, for the caller to resume after the frame's tail.
    pub(super) fn queue_pending_frame(
        producer: &PresentationState,
        frame: PendingFrame,
        replaces: Option<FrameEpoch>,
    ) -> FirstPanic {
        let (replaced, overflow) = {
            let mut pending = producer.pending_frames.borrow_mut();
            let replaced = replaces
                .and_then(|replaced| pending.iter().rposition(|record| record.epoch == replaced))
                .and_then(|index| pending.remove(index));
            let overflow = (pending.len() >= MAX_PENDING_FRAMES)
                .then(|| pending.pop_back())
                .flatten();
            pending.push_back(frame);
            (replaced, overflow)
        };
        // The reads wait again before a subscriber hears of it, and each step runs contained.
        let mut held = FirstPanic::new();
        if let Some(replaced) = replaced {
            held.contain(|| Self::abandon_pending_frame(producer, replaced));
        }
        if let Some(overflow) = overflow {
            let epoch = overflow.epoch.get();
            held.contain(|| Self::abandon_pending_frame(producer, overflow));
            held.contain(|| {
                tracing::error!(
                    target: "flui.frame",
                    epoch,
                    limit = MAX_PENDING_FRAMES,
                    "a presentation holds more frames pending than its sink keeps unsettled; \
                     the newest record folds"
                );
            });
        }
        held
    }

    /// Settles one frame a sink queued (ADR-0305 §4): the frame whose record `settlement`'s
    /// address and epoch name commits what it captured when it was submitted, by the same rules
    /// an inline verdict follows. The raster side settles a slot's frames in order, so frames of
    /// the same presentation queued before it and still pending get no settlement of their own,
    /// and their records fold into this one; an executor that dies settles a frame queued behind a
    /// render it still holds only after that render, so even then a later settlement never folds
    /// a frame the raster side may still show. A settlement no pending record matches (another
    /// runtime's, an earlier life's, one already settled or folded, or a queued verdict, which no
    /// settlement carries) is [`Settled::Stale`], traced and ignored.
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
        if matches!(
            verdict,
            SubmitVerdict::Queued | SubmitVerdict::QueuedReplacing(_)
        ) {
            return stale("a settlement never carries a queued verdict");
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
        // Each runs contained, so a visual-update callback that panics while one of them asks for
        // a repaint still lets the settled frame commit; the first failure resumes after.
        let mut held = FirstPanic::new();
        for superseded in superseded {
            held.contain(|| Self::abandon_pending_frame(producer, superseded));
        }
        let tail = self.settle_pending_frame(producer, frame, verdict, held);
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
                // An unseen presentation retries once it is seen, which wakes it.
                if !producer.clock().is_hidden() {
                    self.wake_frame();
                }
            } else if !producer.failure_backoff_holds(producer.clock().now())
                && !producer.device_abandoned()
                && !producer.frames_in_flight().is_saturated()
                && producer.has_frame_work()
            {
                // Work the in-flight gate kept while the sink held as many frames as it may:
                // this settlement freed a place, and nothing else wakes the loop for it
                // (ADR-0305 §5).
                self.wake_frame();
            }
        });
        // Held input replays once the retry is in place, before the first panic resumes: a panic
        // that came first does not strand the input the commit released. Which settlement
        // replays it is decided with the verdict (`owe_current_tree`), and the replay runs only
        // while the presentation is committed, the rule that held it.
        if replay_committed_input {
            held.contain(|| self.replay_committed_held_input(producer));
        }
        if let Some(style) = overlay_style {
            held.contain(|| {
                producer.with_window(|window| producer.overlay_style.deliver(window, style));
            });
        }
        if let Some(payload) = held.into_payload() {
            resume_unwind(payload);
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
            epoch,
            submitted_at: _,
            telemetry,
            shown,
            tree_revision,
            input_regions,
            layer_reads,
            haptics,
            overlay_style,
            drawn_unseen,
        } = frame;
        // Input held for a commit replays against the tree this frame shows only if no later
        // frame was painted since; a settlement of an older frame leaves it for the newer one.
        let shows_current_tree = tree_revision >= producer.tree_revision();
        // The frame this scene becomes once presented.
        let frame_number = producer.frames_rendered() + 1;
        let shown_on_screen = matches!(
            render_verdict,
            SubmitVerdict::Presented | SubmitVerdict::NoPresent
        );
        // The owed frame of a covered window that its surface did not take ends the owing; a
        // frame drawn while the window was seen and settled after the covering does not.
        if drawn_unseen && !shown_on_screen {
            producer.refuse_occluded_frame();
        }
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
            // present in ~42 µs — ADR-0353's per-backend facts. On the inline
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
            // A frame that commits the current tree is through: the next dropped frame starts a
            // fresh streak. An older frame's commit leaves the streak and the paced retry a newer
            // failed frame armed, which still owes the current tree.
            if shows_current_tree
                && matches!(
                    render_verdict,
                    SubmitVerdict::Presented | SubmitVerdict::NoPresent
                )
            {
                producer.reset_frame_failure_streak();
            }
            match render_verdict {
                SubmitVerdict::Presented => {
                    held.absorb(self.commit_painted_frame(producer, tree_revision, &input_regions));
                    // What the tail owes is recorded before the code below that the runtime does
                    // not own (a haptic backend, telemetry, a subscriber) can panic.
                    tail.overlay_style = overlay_style;
                    tail.presented = true;
                    Self::owe_current_tree(producer, shows_current_tree, &mut tail);
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
                        epoch,
                        telemetry,
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
                    Self::owe_current_tree(producer, shows_current_tree, &mut tail);
                    // Nothing was owed and nothing was lost — the surface was
                    // never asked to show anything, so a withheld streak is
                    // over just as it is on a present.
                    producer.clear_not_shown_streak();
                    tracing::trace!(
                        frame = frame_number,
                        "Frame skipped: no damage (no present)"
                    );
                }
                // The backend had content in hand and could not put it on screen (ADR-0335). The
                // frame commits nothing: the tree revision stays uncommitted, the input regions of
                // the last presented frame stay in force, and pointer input waits for the frame
                // that shows the tree (ADR-0305 §4). The work that produced the scene was consumed
                // out of the pipeline, so the retry repaints in full, until ADR-0304 §7 retries
                // from the held version: a wake alone would find nothing dirty and park with the
                // tree never shown.
                //
                // The first `MAX_NOT_SHOWN_RETRIES` consecutive attempts retry on the next display
                // period, which rides out a drawable that is withdrawn for a few frames (AppKit's
                // cold start withholds about 12 while `occlusionState` reports the window visible,
                // so the lifecycle gate does not engage). Past that budget the withdrawal is not
                // a transient (a window on an inactive Space, a display asleep): each further
                // attempt waits out the dropped-frame backoff, which doubles to one second, so
                // the presentation shows its tree within a second of the surface's return and
                // costs at most one paint per second until then. A key or button press ends the
                // wait early, as for any dropped frame.
                SubmitVerdict::NotShown => {
                    // The frame never reached the screen, so frame-aligned
                    // haptics wait: a retry that presents before their
                    // deadline plays them, otherwise they resolve `Suppressed`.
                    producer.record_frame_dropped();
                    let streak = producer.record_frame_withheld();
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
                        // The streak stays above the budget until a frame presents or finds no
                        // damage, or the window is shown again after it was hidden (a new
                        // withdrawal), so the next attempt waits out the backoff too instead of
                        // opening another burst. A settlement of an older tree and a given-up
                        // device (ADR-0267) arm nothing: the current tree is owed by its own frame,
                        // and a given-up device holds its frames. A frame that shows the current
                        // tree has no pending frame painted from it or a later one, since each
                        // painted segment advances the tree revision (ADR-0048), so the failed
                        // frame's guard for one has nothing to guard here.
                        let mut held = FirstPanic::new();
                        if shows_current_tree && !producer.device_abandoned() {
                            producer.note_frame_failure();
                            Self::arm_failure_retry(producer);
                            // The mark's platform wake is user code and can unwind: the wait is
                            // armed first, and the panic is held until the trace below ran.
                            held.contain(|| self.mark_needs_full_repaint_for(producer));
                        }
                        let first_report = producer.first_withdrawal_past_budget();
                        held.contain(|| {
                            // Whether the window's host tells a covering: one that cannot leaves
                            // a covered window reading seen, so its frames run into a surface
                            // that refuses them, and the report says which case it is. Read
                            // inside the containment: the window is not ours. `None` once the
                            // window is gone.
                            if first_report {
                                let host_reports_occlusion: Option<bool> =
                                    producer.with_window(|window| {
                                        window
                                            .state()
                                            .capabilities
                                            .reports_occlusion()
                                            .is_available()
                                    });
                                tracing::warn!(
                                    target: "flui.pace",
                                    event = "surface_withholds_frames",
                                    frame = frame_number,
                                    streak,
                                    host_reports_occlusion = ?host_reports_occlusion,
                                    "the window's surface withheld {} frames in a row; \
                                     retrying after the dropped-frame backoff. With \
                                     `host_reports_occlusion` false the window is likely covered \
                                     by another window its host cannot tell of; with it true the \
                                     host reads the window seen while its surface shows nothing",
                                    MAX_NOT_SHOWN_RETRIES
                                );
                            } else {
                                tracing::debug!(
                                    frame = frame_number,
                                    streak,
                                    "Frame withheld by the surface past {} consecutive attempts; \
                                     retrying after the dropped-frame backoff",
                                    MAX_NOT_SHOWN_RETRIES
                                );
                            }
                        });
                        if let Some(payload) = held.into_payload() {
                            resume_unwind(payload);
                        }
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
                        epoch,
                        telemetry,
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
                    // Paced like any dropped frame (ADR-0267): a frame asked
                    // for at once would run again before the renderer's owner
                    // next rebuilds the device, and fail the same way. A
                    // given-up rebuild was reported when it was given up and
                    // holds the frames until a rebuild is asked for again.
                    // The report counts the failure into the streak the retry
                    // is paced by, and the retry is armed before the failure
                    // handler or the telemetry's subscriber, code the runtime
                    // does not own, can panic.
                    let report = (!producer.device_abandoned()).then(|| {
                        let recovery = producer.device_recovery().unwrap_or_default();
                        let report = self.frame_failure_report(
                            producer,
                            FrameFailureKind::DeviceLost { recovery },
                        );
                        Self::arm_failure_retry(producer);
                        report
                    });
                    // `Retain`, not `Drain`: the frame is tried again, and the
                    // eventual real submit must still find the epochs that
                    // arrived before this failure — see
                    // `record_submit_telemetry`'s own doc. A subscriber's panic
                    // there is held until the failure is reported.
                    let mut reported = FirstPanic::new();
                    // The submit consumed the paint demand, so the frame the
                    // rebuilt device shows needs a full repaint whenever it
                    // comes. Marked after the retry is armed, since its
                    // platform wake can unwind, and held like the telemetry's.
                    reported.contain(|| self.mark_needs_full_repaint_for(producer));
                    reported.contain(|| {
                        self.record_submit_telemetry(
                            epoch,
                            telemetry,
                            producer,
                            submit_at,
                            PresentOutcome::Errored,
                            EpochDisposition::Retain,
                        );
                    });
                    if let Some(report) = report {
                        reported.contain(|| self.deliver_failure_report(report));
                    }
                    if let Some(payload) = reported.into_payload() {
                        resume_unwind(payload);
                    }
                }
                SubmitVerdict::Failed | SubmitVerdict::Refused => {
                    producer.record_frame_dropped();
                    // No retry fixes this frame, but its tree is still not shown and nothing else
                    // may ask for a frame: unless a pending frame painted from this tree or a
                    // later one will settle, the frame is tried again after the dropped-frame
                    // backoff (a key or button press ends the wait early), with a full repaint
                    // since the submit consumed the paint demand. The streak and the wait are armed
                    // before the mark, whose platform wake is user code and can unwind; that panic
                    // is held until the telemetry is drained. A given-up device holds the frames
                    // (ADR-0267). A refused frame waits for its window's next change, which asks
                    // for a frame.
                    let shown_by_later = producer
                        .pending_frames
                        .borrow()
                        .iter()
                        .any(|pending| pending.tree_revision >= tree_revision);
                    let mut held = FirstPanic::new();
                    if render_verdict == SubmitVerdict::Failed
                        && !shown_by_later
                        && !producer.device_abandoned()
                    {
                        producer.note_frame_failure();
                        Self::arm_failure_retry(producer);
                        held.contain(|| self.mark_needs_full_repaint_for(producer));
                    }
                    // A failed frame takes the inputs stamped since the frames still waiting for
                    // their settlement were cut: the earlier ones are theirs to report.
                    let mut telemetry = telemetry;
                    telemetry.cut = producer
                        .pending_frames
                        .borrow()
                        .iter()
                        .fold(telemetry.cut, |cut, pending| {
                            cut.after(pending.telemetry.cut)
                        });
                    held.contain(|| {
                        self.record_submit_telemetry(
                            epoch,
                            telemetry,
                            producer,
                            submit_at,
                            PresentOutcome::Errored,
                            EpochDisposition::Drain,
                        );
                        // The sink already logged the backend-specific error at
                        // classification time; this records the transaction-
                        // level consequence.
                        tracing::error!("Render error (non-recoverable this frame)");
                    });
                    if let Some(payload) = held.into_payload() {
                        resume_unwind(payload);
                    }
                }
                SubmitVerdict::Queued | SubmitVerdict::QueuedReplacing(_) => {
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
