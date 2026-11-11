//! The raster executor: one mutator that renders every presentation's slot,
//! fed by closed commands (ADR-0305 §3–§6, §10).
//!
//! The owner holds a [`RasterSender`]: it mints a [`SlotId`], attaches the
//! slot, sends frames and configuration changes, and drains the slot's
//! settlements. The [`RasterExecutor`] holds the backend (a
//! [`GpuContext`], or the testing stand-in) and applies
//! what was sent in [`turns`](RasterExecutor::turn): first the lifecycle
//! queue in order, then each slot's inbox, where a later frame supersedes a
//! pending one and each kind of change keeps its latest value.
//!
//! Each frame settles by its stamp into its slot's record: a ring of the
//! placement's depth the owner drains on its own thread, so a settlement
//! names the frame it answers, not whatever frame is current when the
//! owner reads it. Every accepted frame settles on every exit: rendered,
//! refused on a stale generation, not shown, failed, panicked, or still
//! pending when the executor stops. A frame superseded in the inbox is
//! settled by the frame that superseded it; a detached slot's pending frame
//! leaves with its presentation.
//!
//! The executor is inline: the owner runs its turn. One lock exists, each
//! slot's inbox's, held only to swap values: nothing that can run user code
//! (a destructor, the host's wake, a `tracing` event) runs under it.

mod backend;
mod command;
mod inbox;
mod sender;
mod slot;

use std::sync::Arc;

use crossbeam_channel::{Receiver, bounded};
use flui_foundation::{
    SurfaceGeneration,
    panic::{FirstPanic, discard_payload},
};
use flui_layer::{DamageRegion, FullReason, ResyncReason, SceneSnapshot};

pub use backend::SlotBackend;
pub use command::{
    ExecutorHealth, LIFECYCLE_CAPACITY, RasterCommand, RasterSendError, RasterVerdict, SendReceipt,
    SlotChange, SlotConfig, SlotSettlement, SlotTarget, SlotVisibility,
};
pub use inbox::{SlotFailure, SlotPhase, SlotRecord};
pub use sender::RasterSender;
pub(crate) use slot::SlotMint;
pub use slot::{SlotDomain, SlotId};

use self::{
    inbox::{INLINE_DEPTH, SlotInbox},
    sender::{Lifecycle, Shared},
};
use crate::{
    error::{EngineError, Recoverability},
    raster::PresentDisposition,
};

/// What one [`RasterExecutor::turn`] did.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurnOutcome {
    /// Lifecycle commands applied.
    pub commands: usize,
    /// Settlements written.
    pub settled: usize,
}

/// One attached slot, as the executor holds it.
struct ExecSlot {
    id: SlotId,
    inbox: Arc<SlotInbox>,
    /// The generation of the configuration the backend has; a frame renders
    /// only when its stamp equals it.
    applied: SurfaceGeneration,
    visibility: SlotVisibility,
    /// Why the next rendered frame repaints the whole target, if it does:
    /// the slot was just attached, resized or shown again, or a frame of it
    /// failed.
    full_redraw: Option<FullReason>,
}

/// The raster executor: owns the backend and every slot, and renders them on
/// one mutator (ADR-0305 §10).
///
/// Inline: the owner runs [`turn`](RasterExecutor::turn) after it sends. Dropping the
/// executor shuts it down.
pub struct RasterExecutor<B: SlotBackend = crate::GpuContext> {
    backend: B,
    shared: Arc<Shared>,
    lifecycle: Receiver<Lifecycle>,
    /// The host's wake: called once a turn wrote a settlement, with no lock
    /// held and contained. Owner-local, as the inline executor is; the
    /// threaded placement asks `Send` of it at its spawn (ADR-0305 §11).
    wake: Box<dyn Fn()>,
    slots: Vec<Option<ExecSlot>>,
}

impl<B: SlotBackend> std::fmt::Debug for RasterExecutor<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RasterExecutor")
            .field("health", &self.shared.health())
            .field("slots", &self.slots.iter().flatten().count())
            .finish_non_exhaustive()
    }
}

impl<B: SlotBackend> RasterExecutor<B> {
    /// An inline executor over `backend`, and the owner's sender for it.
    ///
    /// `wake` is the host's: the executor calls it once a turn wrote a
    /// settlement, with no lock held and contained.
    pub fn new(backend: B, wake: impl Fn() + 'static) -> (Self, RasterSender) {
        let (lifecycle_tx, lifecycle) = bounded(LIFECYCLE_CAPACITY);
        let shared = Arc::new(Shared::new(lifecycle_tx));
        let sender = RasterSender::new(Arc::clone(&shared));
        (
            Self {
                backend,
                shared,
                lifecycle,
                wake: Box::new(wake),
                slots: Vec::new(),
            },
            sender,
        )
    }

    /// Whether the executor still takes commands.
    #[must_use]
    pub fn health(&self) -> ExecutorHealth {
        self.shared.health()
    }

    /// The backend, for a read that is not a command (an offscreen slot's
    /// readback).
    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Apply everything sent since the last turn: the trim, then the
    /// lifecycle queue in order, then each slot's inbox.
    ///
    /// Every command runs contained. A panic in a slot's command settles its
    /// frame [`RasterVerdict::Retry`] and goes into the slot's record; a
    /// panic in the trim, a destructor or the host's wake is resumed once the
    /// turn's state is committed.
    pub fn turn(&mut self) -> TurnOutcome {
        let mut outcome = TurnOutcome::default();
        if self.shared.health() != ExecutorHealth::Running {
            return outcome;
        }
        let mut first = FirstPanic::new();
        if let Some(level) = self.shared.take_trim() {
            first.contain(|| self.backend.reclaim(level));
        }
        while let Ok(message) = self.lifecycle.try_recv() {
            outcome.commands += 1;
            outcome.settled += match message {
                Lifecycle::Attach {
                    slot,
                    target,
                    config,
                    generation,
                    inbox,
                } => self.attach(slot, target, config, generation, inbox, &mut first),
                Lifecycle::Detach { slot, inbox } => {
                    self.detach(slot, &inbox, &mut first);
                    0
                }
            };
        }
        for index in 0..self.slots.len() {
            outcome.settled += self.service(index, &mut first);
        }
        if outcome.settled > 0 {
            first.contain(|| (self.wake)());
        }
        first.resume();
        outcome
    }

    /// Stop: every send answers [`RasterSendError::Gone`] from now on, every
    /// slot is taken down, and every frame still pending settles
    /// [`RasterVerdict::Failed`], those of slots whose attach was still
    /// queued included. Called by `Drop`; a second call does nothing.
    pub fn shut_down(&mut self) {
        if self.shared.health() != ExecutorHealth::Running {
            return;
        }
        self.shared.shut_down();
        let mut first = FirstPanic::new();
        let mut settled = 0;
        while let Ok(message) = self.lifecycle.try_recv() {
            let inbox = match message {
                Lifecycle::Attach { inbox, .. } => inbox,
                Lifecycle::Detach { slot, inbox } => {
                    self.detach(slot, &inbox, &mut first);
                    inbox
                }
            };
            settled += close(&inbox, &mut first);
        }
        for entry in &mut self.slots {
            if let Some(slot) = entry.take() {
                first.contain(|| self.backend.detach(slot.id));
                settled += close(&slot.inbox, &mut first);
            }
        }
        if settled > 0 {
            first.contain(|| (self.wake)());
        }
        first.resume();
    }

    fn attach(
        &mut self,
        slot: SlotId,
        target: SlotTarget,
        config: SlotConfig,
        generation: SurfaceGeneration,
        inbox: Arc<SlotInbox>,
        first: &mut FirstPanic,
    ) -> usize {
        let mut caught = FirstPanic::new();
        let failure = match caught.contain(|| self.backend.attach(slot, target)) {
            Some(Ok(())) => None,
            Some(Err(error)) => Some(SlotFailure::Refused(error)),
            None => {
                Some(SlotFailure::Panicked(caught.into_payload().expect(
                    "BUG: a contained call that did not return holds its panic",
                )))
            }
        };
        let Some(failure) = failure else {
            inbox.lock().phase = SlotPhase::Attached;
            let index = slot.index() as usize;
            if self.slots.len() <= index {
                self.slots.resize_with(index + 1, || None);
            }
            self.slots[index] = Some(ExecSlot {
                id: slot,
                inbox,
                applied: generation,
                visibility: config.visibility,
                full_redraw: Some(FullReason::FirstFrame),
            });
            return 0;
        };
        let cause = cause(&failure);
        // A frame sent before the refusal was visible settles here; later
        // ones are refused at send.
        let (frame, extra) = {
            let mut state = inbox.lock();
            state.phase = SlotPhase::Refused;
            let extra = state.hold_failure(failure);
            let frame = state.frame.take();
            if let Some(frame) = frame.as_ref() {
                state.settle(frame, RasterVerdict::Failed);
            }
            (frame, extra)
        };
        let settled = usize::from(frame.is_some());
        retire(frame, extra, first);
        report(Report::Refused { slot, cause }, first);
        settled
    }

    fn detach(&mut self, slot: SlotId, inbox: &SlotInbox, first: &mut FirstPanic) {
        let index = slot.index() as usize;
        if self
            .slots
            .get(index)
            .and_then(Option::as_ref)
            .is_some_and(|held| held.id == slot)
        {
            self.slots[index] = None;
            first.contain(|| self.backend.detach(slot));
        }
        // The pending frame leaves with its presentation, unsettled.
        let frame = {
            let mut state = inbox.lock();
            state.phase = SlotPhase::Detached;
            state.in_flight = 0;
            state.frame.take()
        };
        retire(frame, None, first);
    }

    /// Apply slot `index`'s pending changes and render its pending frame;
    /// answers how many settlements it wrote.
    fn service(&mut self, index: usize, first: &mut FirstPanic) -> usize {
        let Self { backend, slots, .. } = self;
        let Some(slot) = slots.get_mut(index).and_then(Option::as_mut) else {
            return 0;
        };
        let (size, generation, visibility, frame) = {
            let mut state = slot.inbox.lock();
            // A full ring holds the next frame where it is.
            let frame = if state.settlements.len() < INLINE_DEPTH {
                state.frame.take()
            } else {
                None
            };
            (
                state.size.take(),
                state.generation,
                state.visibility.take(),
                frame,
            )
        };
        let mut failure = None;
        // A resize that failed stays pending, so the next turn retries it:
        // the owner already stamps frames with the generation it minted.
        let mut retry = None;
        let mut resize_report = None;
        if let Some(visibility) = visibility {
            if !slot.visibility.is_shown() && visibility.is_shown() {
                slot.owe_full(RESET);
            }
            slot.visibility = visibility;
        }
        if let Some(size) = size {
            let mut caught = FirstPanic::new();
            match caught.contain(|| backend.resize(slot.id, size)) {
                Some(Ok(())) => {
                    slot.applied = generation;
                    slot.owe_full(FullReason::SurfaceChanged);
                }
                Some(Err(error)) => {
                    retry = Some(size);
                    resize_report = Some(Report::ResizeRefused { slot: slot.id });
                    failure = Some(SlotFailure::Refused(error));
                }
                None => {
                    retry = Some(size);
                    failure = caught.into_payload().map(SlotFailure::Panicked);
                }
            }
        }
        let Some(frame) = frame else {
            let extra = {
                let mut state = slot.inbox.lock();
                if let Some(size) = retry {
                    state.size.get_or_insert(size);
                }
                failure.and_then(|failure| state.hold_failure(failure))
            };
            retire(None, extra, first);
            report_each([resize_report, None], first);
            return 0;
        };
        let (verdict, render_failure, surface_lost, render_report) = render(backend, slot, &frame);
        let discarded = if failure.is_none() {
            failure = render_failure;
            None
        } else {
            render_failure
        };
        let extra = {
            let mut state = slot.inbox.lock();
            if let Some(size) = retry {
                state.size.get_or_insert(size);
            }
            if surface_lost {
                state.generation = state.generation.next();
                slot.applied = state.generation;
            }
            state.settle(&frame, verdict);
            failure.and_then(|failure| state.hold_failure(failure))
        };
        retire(Some(frame), extra, first);
        retire(None, discarded, first);
        report_each([resize_report, render_report], first);
        1
    }
}

/// The full repaint a slot owes after the raster side lost what it showed.
const RESET: FullReason = FullReason::Resync(ResyncReason::RasterReset);

impl ExecSlot {
    /// Owe a full repaint for `reason`, unless one is owed already.
    fn owe_full(&mut self, reason: FullReason) {
        let _owed = self.full_redraw.get_or_insert(reason);
    }
}

impl<B: SlotBackend> Drop for RasterExecutor<B> {
    fn drop(&mut self) {
        self.shut_down();
    }
}

/// Render `frame` into `slot`: its verdict, the slot failure it raised,
/// whether the surface was lost, and what to report once the frame settled.
fn render<B: SlotBackend>(
    backend: &mut B,
    slot: &mut ExecSlot,
    frame: &SceneSnapshot,
) -> (RasterVerdict, Option<SlotFailure>, bool, Option<Report>) {
    let stamped = frame.stamp.surface_generation;
    if !slot.visibility.is_shown() {
        slot.owe_full(RESET);
        return (RasterVerdict::NotShown, None, false, None);
    }
    if stamped == SurfaceGeneration::ZERO || stamped != slot.applied {
        slot.owe_full(FullReason::SurfaceChanged);
        let report = Report::Stale {
            slot: slot.id,
            stamped,
            applied: slot.applied,
        };
        return (RasterVerdict::SurfaceStale, None, false, Some(report));
    }
    let damage = slot.full_redraw.map_or(frame.damage, DamageRegion::Full);
    let mut caught = FirstPanic::new();
    let rendered = caught.contain(|| backend.render(slot.id, &frame.scene, damage));
    let Some(rendered) = rendered else {
        slot.owe_full(RESET);
        let payload = caught
            .into_payload()
            .expect("BUG: a contained call that did not return holds its panic");
        return (
            RasterVerdict::Retry,
            Some(SlotFailure::Panicked(payload)),
            false,
            None,
        );
    };
    match rendered {
        Ok(PresentDisposition::Presented) => {
            slot.full_redraw = None;
            (RasterVerdict::Presented, None, false, None)
        }
        Ok(PresentDisposition::NoDamage) => (RasterVerdict::NoPresent, None, false, None),
        Ok(PresentDisposition::NotShown) => {
            slot.owe_full(RESET);
            (RasterVerdict::NotShown, None, false, None)
        }
        Err(error) => {
            slot.owe_full(RESET);
            let (verdict, surface_lost) = classify(&error);
            let report = Report::FrameFailed {
                slot: slot.id,
                error,
                verdict,
            };
            (verdict, None, surface_lost, Some(report))
        }
    }
}

/// A tracing event about a command, emitted once the command committed:
/// a subscriber is user code, and one that panics must not strand a frame.
enum Report {
    Refused {
        slot: SlotId,
        cause: &'static str,
    },
    ResizeRefused {
        slot: SlotId,
    },
    Stale {
        slot: SlotId,
        stamped: SurfaceGeneration,
        applied: SurfaceGeneration,
    },
    FrameFailed {
        slot: SlotId,
        error: EngineError,
        verdict: RasterVerdict,
    },
}

/// What kind of failure a slot's record holds, for a report that must not
/// format the failure itself: an error's `Debug` or `Display` can be user
/// code, and the owner reads the failure from the record.
fn cause(failure: &SlotFailure) -> &'static str {
    match failure {
        SlotFailure::Refused(_) => "refused",
        SlotFailure::Panicked(_) => "panicked",
    }
}

fn report(report: Report, first: &mut FirstPanic) {
    first.contain(move || match report {
        Report::Refused { slot, cause } => {
            tracing::warn!(
                ?slot,
                cause,
                "raster executor: slot refused; the record holds why"
            );
        }
        Report::ResizeRefused { slot } => {
            tracing::warn!(
                ?slot,
                "raster executor: resize refused; the record holds why, retried next turn"
            );
        }
        Report::Stale {
            slot,
            stamped,
            applied,
        } => tracing::debug!(
            ?slot,
            ?stamped,
            ?applied,
            "raster executor: frame stamped against another surface generation"
        ),
        Report::FrameFailed {
            slot,
            error,
            verdict,
        } => tracing::warn!(?slot, %error, ?verdict, "raster executor: frame failed"),
    });
}

fn report_each<const N: usize>(reports: [Option<Report>; N], first: &mut FirstPanic) {
    for each in reports.into_iter().flatten() {
        report(each, first);
    }
}

/// A render error's verdict, and whether it lost the surface (which mints the
/// slot's next generation).
fn classify(error: &EngineError) -> (RasterVerdict, bool) {
    match error {
        EngineError::DeviceLost | EngineError::GpuUnavailable | EngineError::GpuProgress { .. } => {
            (RasterVerdict::DeviceLost, false)
        }
        EngineError::SurfaceLost | EngineError::SurfaceValidation => {
            (RasterVerdict::SurfaceStale, true)
        }
        other if other.recoverability() == Recoverability::Recoverable => {
            (RasterVerdict::Retry, false)
        }
        _ => (RasterVerdict::Failed, false),
    }
}

/// Take `inbox` down for good, settling its pending frame `Failed`; answers
/// how many settlements it wrote.
fn close(inbox: &SlotInbox, first: &mut FirstPanic) -> usize {
    let frame = {
        let mut state = inbox.lock();
        state.phase = SlotPhase::Detached;
        let frame = state.frame.take();
        if let Some(frame) = frame.as_ref() {
            state.settle(frame, RasterVerdict::Failed);
        }
        state.in_flight = 0;
        frame
    };
    let settled = usize::from(frame.is_some());
    retire(frame, None, first);
    settled
}

/// Drop what a command took out of an inbox, after its lock is released: a
/// scene's destructor and a panic payload's can be user code.
fn retire(frame: Option<SceneSnapshot>, extra: Option<SlotFailure>, first: &mut FirstPanic) {
    if let Some(frame) = frame {
        first.contain(move || drop(frame));
    }
    match extra {
        Some(SlotFailure::Panicked(payload)) => discard_payload(payload),
        Some(SlotFailure::Refused(error)) => {
            let _dropped = first.contain(move || drop(error));
        }
        None => {}
    }
}
