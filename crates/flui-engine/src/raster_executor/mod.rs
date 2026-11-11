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
use flui_layer::{DamageRegion, SceneSnapshot};

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
    /// The next rendered frame repaints the whole target: after the slot is
    /// shown again, resized, or a frame of it failed.
    full_redraw: bool,
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
    pub fn new(backend: B, wake: impl Fn() + Send + Sync + 'static) -> (Self, RasterSender) {
        let (lifecycle_tx, lifecycle) = bounded(LIFECYCLE_CAPACITY);
        let shared = Arc::new(Shared::new(lifecycle_tx, Box::new(wake)));
        let sender = RasterSender::new(Arc::clone(&shared));
        (
            Self {
                backend,
                shared,
                lifecycle,
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
            first.contain(|| (self.shared.wake)());
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
            first.contain(|| (self.shared.wake)());
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
                full_redraw: true,
            });
            return 0;
        };
        tracing::warn!(?slot, ?failure, "raster executor: slot refused");
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
        if let Some(visibility) = visibility {
            slot.full_redraw |= !slot.visibility.is_shown() && visibility.is_shown();
            slot.visibility = visibility;
        }
        if let Some(size) = size {
            let mut caught = FirstPanic::new();
            match caught.contain(|| backend.resize(slot.id, size)) {
                Some(Ok(())) => {
                    slot.applied = generation;
                    slot.full_redraw = true;
                }
                Some(Err(error)) => {
                    tracing::warn!(slot = ?slot.id, %error, "raster executor: resize refused");
                }
                None => failure = caught.into_payload().map(SlotFailure::Panicked),
            }
        }
        let Some(frame) = frame else {
            settle_failure(&slot.inbox, failure, first);
            return 0;
        };
        let (verdict, render_failure, surface_lost) = render(backend, slot, &frame);
        if failure.is_none() {
            failure = render_failure;
        } else if let Some(SlotFailure::Panicked(later)) = render_failure {
            discard_payload(later);
        }
        let extra = {
            let mut state = slot.inbox.lock();
            if surface_lost {
                state.generation = state.generation.next();
                slot.applied = state.generation;
            }
            state.settle(&frame, verdict);
            failure.and_then(|failure| state.hold_failure(failure))
        };
        retire(Some(frame), extra, first);
        1
    }
}

impl<B: SlotBackend> Drop for RasterExecutor<B> {
    fn drop(&mut self) {
        self.shut_down();
    }
}

/// Render `frame` into `slot`: its verdict, the slot failure it raised, and
/// whether the surface was lost.
fn render<B: SlotBackend>(
    backend: &mut B,
    slot: &mut ExecSlot,
    frame: &SceneSnapshot,
) -> (RasterVerdict, Option<SlotFailure>, bool) {
    let stamped = frame.stamp.surface_generation;
    if !slot.visibility.is_shown() {
        slot.full_redraw = true;
        return (RasterVerdict::NotShown, None, false);
    }
    if stamped == SurfaceGeneration::ZERO || stamped != slot.applied {
        tracing::debug!(
            slot = ?slot.id,
            ?stamped,
            applied = ?slot.applied,
            "raster executor: frame stamped against another surface generation"
        );
        slot.full_redraw = true;
        return (RasterVerdict::SurfaceStale, None, false);
    }
    let damage = if slot.full_redraw {
        DamageRegion::Full
    } else {
        frame.damage
    };
    let mut caught = FirstPanic::new();
    let rendered = caught.contain(|| backend.render(slot.id, &frame.scene, damage));
    let Some(rendered) = rendered else {
        slot.full_redraw = true;
        let payload = caught
            .into_payload()
            .expect("BUG: a contained call that did not return holds its panic");
        return (
            RasterVerdict::Retry,
            Some(SlotFailure::Panicked(payload)),
            false,
        );
    };
    match rendered {
        Ok(PresentDisposition::Presented) => {
            slot.full_redraw = false;
            (RasterVerdict::Presented, None, false)
        }
        Ok(PresentDisposition::NoDamage) => (RasterVerdict::NoPresent, None, false),
        Ok(PresentDisposition::NotShown) => {
            slot.full_redraw = true;
            (RasterVerdict::NotShown, None, false)
        }
        Err(error) => {
            slot.full_redraw = true;
            let (verdict, surface_lost) = classify(&error);
            tracing::warn!(slot = ?slot.id, %error, ?verdict, "raster executor: frame failed");
            (verdict, None, surface_lost)
        }
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

/// Hold `failure` in `inbox`'s record, with the lock released before a
/// failure it already holds is discarded.
fn settle_failure(inbox: &SlotInbox, failure: Option<SlotFailure>, first: &mut FirstPanic) {
    let Some(failure) = failure else {
        return;
    };
    let extra = inbox.lock().hold_failure(failure);
    retire(None, extra, first);
}

/// Drop what a command took out of an inbox, after its lock is released: a
/// scene's destructor and a panic payload's can be user code.
fn retire(frame: Option<SceneSnapshot>, extra: Option<SlotFailure>, first: &mut FirstPanic) {
    if let Some(frame) = frame {
        first.contain(move || drop(frame));
    }
    match extra {
        Some(SlotFailure::Panicked(payload)) => discard_payload(payload),
        Some(SlotFailure::Refused(error)) => drop(error),
        None => {}
    }
}
