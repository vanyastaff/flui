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
//! The executor is inline: the owner runs its turn. Two locks exist, each
//! slot's inbox's and the device's record's, held only to swap values:
//! nothing that can run user code (a destructor, the host's wake, a `tracing`
//! event) runs under them.

mod backend;
mod command;
mod device_health;
mod inbox;
mod sender;
mod slot;

use std::{rc::Rc, sync::Arc};

use crossbeam_channel::{Receiver, bounded};
use flui_foundation::{
    SurfaceGeneration,
    clock::MonotonicClock,
    panic::{FirstPanic, discard_payload},
};
use flui_layer::{DamageRegion, FullReason, ResyncReason, SceneSnapshot};

pub use backend::SlotBackend;
pub use command::{
    ExecutorHealth, LIFECYCLE_CAPACITY, RasterCommand, RasterSendError, RasterVerdict, SendReceipt,
    SlotChange, SlotConfig, SlotSettlement, SlotTarget, SlotVisibility,
};
pub use device_health::DeviceHealth;
pub use inbox::{SlotFailure, SlotPhase, SlotRecord};
pub use sender::RasterSender;
pub(crate) use slot::SlotMint;
pub use slot::{SlotDomain, SlotId};

use self::{
    device_health::{DeviceRebuilds, FailedRebuild},
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
    /// The size the backend last refused, so a size refused again on every
    /// turn or every send warns once.
    refused_size: Option<flui_foundation::geometry::DeviceSize>,
    /// Whether the slot presents to a window's surface, which only its owner
    /// can replace.
    window: bool,
}

/// The raster executor: owns the backend and every slot, and renders them on
/// one mutator (ADR-0305 §10).
///
/// Inline: the owner runs [`turn`](RasterExecutor::turn) after it sends. Dropping the
/// executor shuts it down.
pub struct RasterExecutor<B: SlotBackend = crate::GpuContext> {
    backend: B,
    shared: Arc<Shared>,
    /// The lifecycle queue; dropped once the executor stopped and drained it.
    lifecycle: Option<Receiver<Lifecycle>>,
    /// The host's wake: called once a turn wrote a settlement or moved the
    /// device's health, with no lock held and contained. Owner-local, as the
    /// inline executor is; the threaded placement asks `Send` of it at its
    /// spawn (ADR-0305 §11).
    wake: Box<dyn Fn()>,
    /// The time the device's rebuilds are paced on: the UI runtime's clock,
    /// so a manual one walks the backoff without sleeping.
    clock: Rc<dyn MonotonicClock>,
    /// The context's device rebuilds (ADR-0305 §8).
    rebuilds: DeviceRebuilds,
    slots: Vec<Option<ExecSlot>>,
}

impl<B: SlotBackend> std::fmt::Debug for RasterExecutor<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RasterExecutor")
            .field("health", &self.shared.health())
            .field("device", &self.rebuilds.health())
            .field("slots", &self.slots.iter().flatten().count())
            .finish_non_exhaustive()
    }
}

impl<B: SlotBackend> RasterExecutor<B> {
    /// An inline executor over `backend`, and the owner's sender for it.
    ///
    /// `clock` paces the rebuilds of a lost device; `wake` is the host's: the
    /// executor calls it once a turn wrote a settlement or moved the device's
    /// health, with no lock held and contained.
    pub fn new(
        backend: B,
        clock: Rc<dyn MonotonicClock>,
        wake: impl Fn() + 'static,
    ) -> (Self, RasterSender) {
        let (lifecycle_tx, lifecycle) = bounded(LIFECYCLE_CAPACITY);
        let shared = Arc::new(Shared::new(lifecycle_tx));
        let sender = RasterSender::new(Arc::clone(&shared));
        (
            Self {
                backend,
                shared,
                lifecycle: Some(lifecycle),
                wake: Box::new(wake),
                clock,
                rebuilds: DeviceRebuilds::default(),
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

    /// The backend, to arm a fault a test injects.
    #[cfg(test)]
    pub(crate) fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
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
        while let Some(message) = self
            .lifecycle
            .as_ref()
            .and_then(|queue| queue.try_recv().ok())
        {
            outcome.commands += 1;
            outcome.settled += match message {
                Lifecycle::Attach {
                    slot,
                    target,
                    config,
                    generation,
                    inbox,
                } => self.attach(slot, target, config, generation, inbox, &mut first),
                Lifecycle::Release { slot } => {
                    self.release(slot, &mut first);
                    0
                }
                Lifecycle::Restore {
                    slot,
                    surface,
                    generation,
                } => {
                    self.restore(slot, surface, generation, &mut first);
                    0
                }
                Lifecycle::Detach { slot, inbox } => {
                    self.detach(slot, &inbox, &mut first);
                    0
                }
                Lifecycle::RetryDevice => {
                    self.rebuilds.retry();
                    0
                }
            };
        }
        let mut device_moved = self.recover_device(&mut first);
        for index in 0..self.slots.len() {
            outcome.settled += self.service(index, &mut first);
        }
        // A loss this turn's own frames met is published now, so the owner
        // knows a rebuild is due without waiting for another turn to see it.
        device_moved |= self.note_loss(&mut first);
        if outcome.settled > 0 || device_moved {
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
        // Every send that saw the executor running has queued its command by
        // now, and every later one answers `Gone`: draining the queue answers
        // every command a send accepted.
        let queue = self.lifecycle.take();
        for message in queue.iter().flat_map(Receiver::try_iter) {
            let inbox = match message {
                Lifecycle::Attach { inbox, target, .. } => {
                    // A window target's destructor can be user code.
                    first.contain(move || drop(target));
                    inbox
                }
                Lifecycle::Release { .. } | Lifecycle::RetryDevice => continue,
                Lifecycle::Restore { surface, .. } => {
                    first.contain(move || drop(surface));
                    continue;
                }
                Lifecycle::Detach { slot, inbox } => {
                    self.detach(slot, &inbox, &mut first);
                    inbox
                }
            };
            settled += close(&inbox, &mut first);
        }
        drop(queue);
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
        let window = matches!(target, SlotTarget::Window(_));
        let failure = match caught.contain(|| self.backend.attach(slot, target, config)) {
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
                refused_size: None,
                window,
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

    /// See whether the device is lost, rebuild it when its backoff allows,
    /// and publish its health; answers whether the health moved, so the
    /// owner is woken to read it.
    ///
    /// A rebuild that fails or panics counts toward the backoff. Its error
    /// waits in the device's record for the owner; its panic is resumed at
    /// the end of the turn, once every slot is served.
    fn recover_device(&mut self, first: &mut FirstPanic) -> bool {
        let before = self.rebuilds.health();
        let lost = self.backend.device_lost();
        let mut rebuilt = false;
        if lost {
            self.rebuilds.note_lost();
        } else if before.is_lost() {
            // The backend renders again without a rebuild this executor saw
            // succeed (one that panicked after its device was in place): the
            // slots are restamped all the same, so "healthy after lost" always
            // means every frame stamped before the loss is refused.
            self.rebuilds.recovered();
            self.after_rebuild(Vec::new(), first);
            rebuilt = true;
        }
        let mut failure = None;
        let mut failed = None;
        let mut now = self.clock.now();
        if self.rebuilds.due(now) {
            if self.backend.rebuilds_inline() {
                let attempt = first.contain(|| self.backend.rebuild_device());
                // The attempt blocks for as long as the driver takes: the next
                // one waits out its backoff from when this one ended.
                now = self.clock.now();
                match attempt {
                    Some(Ok(refused)) => {
                        self.rebuilds.recovered();
                        // The slots are restamped before the health says the
                        // device renders, so no owner stamps a frame for the
                        // old device after reading it.
                        self.after_rebuild(refused, first);
                        rebuilt = true;
                    }
                    Some(Err(error)) => {
                        failure = Some(error);
                        failed = Some(self.rebuilds.record_failure(now));
                    }
                    None => failed = Some(self.rebuilds.record_failure(now)),
                }
                if failed.is_some() && !self.backend.device_lost() {
                    // The rebuild failed with its new device in place: the
                    // slots are restamped before this turn serves them, as
                    // after a rebuild that succeeded.
                    self.rebuilds.recovered();
                    self.after_rebuild(Vec::new(), first);
                    rebuilt = true;
                }
            } else {
                // No attempt can succeed here: give up at once instead of
                // walking the backoff (the web, ADR-0305 §8).
                self.rebuilds.give_up();
                failure = Some(EngineError::GpuUnavailable);
                failed = Some(FailedRebuild::GivenUp);
            }
        }
        let after = self.publish_device(failure, now, first);
        if lost && !before.is_lost() {
            report(Report::DeviceLost, first);
        }
        if let Some(failed) = failed {
            report(Report::RebuildFailed(failed, after), first);
        } else if rebuilt {
            report(Report::DeviceRebuilt, first);
        }
        before != after || rebuilt
    }

    /// See whether this turn's frames lost the device while it was published
    /// healthy, and publish the loss with its rebuild due now; answers
    /// whether it did.
    fn note_loss(&mut self, first: &mut FirstPanic) -> bool {
        if self.rebuilds.health().is_lost() || !self.backend.device_lost() {
            return false;
        }
        self.rebuilds.note_lost();
        let _published = self.publish_device(None, self.clock.now(), first);
        report(Report::DeviceLost, first);
        true
    }

    /// Write the device's health, its next attempt and a new failure into the
    /// record the owner reads; answers the health written. The failure it
    /// replaces is dropped outside the lock and contained.
    fn publish_device(
        &mut self,
        failure: Option<EngineError>,
        now: web_time::Instant,
        first: &mut FirstPanic,
    ) -> DeviceHealth {
        let health = self.rebuilds.health();
        let replaced = {
            let mut record = self.shared.device();
            record.health = health;
            record.next_attempt_at = self.rebuilds.next_attempt_at(now);
            failure.and_then(|error| record.failure.replace(error))
        };
        if let Some(error) = replaced {
            let _dropped = first.contain(move || drop(error));
        }
        health
    }

    /// The device was rebuilt: every frame stamped before it is refused on
    /// its generation, as after a restore, and every slot redraws in full. A
    /// size still pending or refused keeps its barrier: only a size the
    /// backend accepts authorises frames laid out for it. A slot the new
    /// device cannot present to waits for a new surface from its owner.
    fn after_rebuild(&mut self, refused: Vec<(SlotId, EngineError)>, first: &mut FirstPanic) {
        for slot in self.slots.iter_mut().flatten() {
            slot.owe_full(RESET);
            let mut state = slot.inbox.lock();
            // A window slot that never had a size has no frame to refuse.
            if state.generation != SurfaceGeneration::ZERO {
                state.generation = state.generation.next();
                if state.size.is_none() && slot.refused_size.is_none() {
                    slot.applied = state.generation;
                }
            }
        }
        for (id, error) in refused {
            let Some(slot) = self.attached(id) else {
                let _dropped = first.contain(move || drop(error));
                continue;
            };
            let extra = {
                let mut state = slot.inbox.lock();
                // A window slot dropped its surface or failed to configure it,
                // and the owner makes another; a slot over the new device's
                // limit, window or offscreen, waits for a size that fits.
                state.wants_surface |=
                    slot.window && !matches!(error, EngineError::InvalidTargetSize { .. });
                state.hold_failure(SlotFailure::Refused(error))
            };
            retire(None, extra, first);
            report(
                Report::Refused {
                    slot: id,
                    cause: "refused",
                },
                first,
            );
        }
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

    /// The attached slot `slot` names; `None` for a stale or refused one.
    fn attached(&mut self, slot: SlotId) -> Option<&mut ExecSlot> {
        self.slots
            .get_mut(slot.index() as usize)
            .and_then(Option::as_mut)
            .filter(|held| held.id == slot)
    }

    fn release(&mut self, slot: SlotId, first: &mut FirstPanic) {
        let Self { backend, slots, .. } = self;
        let Some(held) = slots
            .get_mut(slot.index() as usize)
            .and_then(Option::as_mut)
            .filter(|held| held.id == slot)
        else {
            return;
        };
        held.owe_full(RESET);
        let mut caught = FirstPanic::new();
        if caught.contain(|| backend.release(slot)).is_none() {
            hold_panic(&held.inbox, caught, first);
        }
    }

    fn restore(
        &mut self,
        slot: SlotId,
        surface: crate::UnconfiguredSurface,
        generation: SurfaceGeneration,
        first: &mut FirstPanic,
    ) {
        if self.attached(slot).is_none() {
            // The slot was refused or detached: the surface leaves with it.
            first.contain(move || drop(surface));
            return;
        }
        let Self { backend, slots, .. } = self;
        let held = slots[slot.index() as usize]
            .as_mut()
            .expect("BUG: the slot was found attached above");
        // The restore answers for every frame stamped before it, whatever its
        // outcome: a refused or panicked one leaves the slot released, whose
        // frames settle not shown. It authorises its generation only while
        // that is still the slot's newest: a loss settled after the restore
        // was sent minted (and, without a barrier, authorised) a newer one,
        // which the restored surface serves. A size still pending or refused
        // keeps its barrier: the restored surface is configured at the size
        // the slot has, and only a size the backend accepts authorises frames
        // laid out for a newer one.
        let newest = {
            let state = held.inbox.lock();
            state.generation == generation && state.size.is_none()
        };
        if newest && held.refused_size.is_none() {
            held.applied = generation;
        }
        held.owe_full(FullReason::SurfaceChanged);
        let mut caught = FirstPanic::new();
        match caught.contain(|| backend.restore(slot, surface)) {
            Some(Ok(())) => held.inbox.lock().wants_surface = false,
            Some(Err(error)) => {
                let extra = held.inbox.lock().hold_failure(SlotFailure::Refused(error));
                retire(None, extra, first);
                report(
                    Report::Refused {
                        slot,
                        cause: "refused",
                    },
                    first,
                );
            }
            None => hold_panic(&held.inbox, caught, first),
        }
    }

    /// Apply slot `index`'s pending changes and render its pending frame;
    /// answers how many settlements it wrote.
    fn service(&mut self, index: usize, first: &mut FirstPanic) -> usize {
        let Self { backend, slots, .. } = self;
        let Some(slot) = slots.get_mut(index).and_then(Option::as_mut) else {
            return 0;
        };
        let (size, generation, visibility, color, latency, frame) = {
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
                state.color.take(),
                state.latency.take(),
                frame,
            )
        };
        if let Some(visibility) = visibility {
            if !slot.visibility.is_shown() && visibility.is_shown() {
                slot.owe_full(RESET);
            }
            slot.visibility = visibility;
        }
        if let Some(latency) = latency {
            let mut caught = FirstPanic::new();
            if caught
                .contain(|| backend.set_latency(slot.id, latency))
                .is_none()
            {
                hold_panic(&slot.inbox, caught, first);
            }
        }
        if let Some(color) = color {
            let mut caught = FirstPanic::new();
            match caught.contain(|| backend.set_display_color(slot.id, color)) {
                Some(Ok(())) => slot.owe_full(FullReason::SurfaceChanged),
                Some(Err(error)) => {
                    let extra = slot.inbox.lock().hold_failure(SlotFailure::Refused(error));
                    retire(None, extra, first);
                }
                None => hold_panic(&slot.inbox, caught, first),
            }
        }
        let mut failure = None;
        // A resize that panicked or was refused for now stays pending, so the
        // next turn retries it: the owner already stamps frames with the
        // generation it minted. One the backend always refuses is not retried:
        // the slot keeps its configuration until the owner sends another size.
        let mut retry = None;
        let mut resize_report = None;
        if let Some(size) = size {
            let mut caught = FirstPanic::new();
            match caught.contain(|| backend.resize(slot.id, size)) {
                Some(Ok(())) => {
                    slot.applied = generation;
                    slot.refused_size = None;
                    slot.owe_full(FullReason::SurfaceChanged);
                }
                Some(Err(error)) => {
                    let transient = error.recoverability() == Recoverability::Recoverable;
                    if transient {
                        retry = Some(size);
                    }
                    // One warning a size, however often it is refused.
                    if slot.refused_size.replace(size) != Some(size) {
                        resize_report = Some(Report::ResizeRefused {
                            slot: slot.id,
                            retried: transient,
                        });
                    }
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
        let (verdict, render_failure, surface_loss, render_report) = render(backend, slot, &frame);
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
            if surface_loss != SurfaceLoss::Kept {
                state.generation = state.generation.next();
                // A resize still pending or refused keeps its barrier: only a
                // size the backend accepts authorises frames laid out for it.
                if state.size.is_none() && slot.refused_size.is_none() {
                    slot.applied = state.generation;
                }
            }
            if surface_loss == SurfaceLoss::Dropped {
                state.wants_surface = true;
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
/// what it did to the surface, and what to report once the frame settled.
fn render<B: SlotBackend>(
    backend: &mut B,
    slot: &mut ExecSlot,
    frame: &SceneSnapshot,
) -> (
    RasterVerdict,
    Option<SlotFailure>,
    SurfaceLoss,
    Option<Report>,
) {
    let stamped = frame.stamp.surface_generation;
    if !slot.visibility.is_shown() {
        slot.owe_full(RESET);
        return (RasterVerdict::NotShown, None, SurfaceLoss::Kept, None);
    }
    if stamped == SurfaceGeneration::ZERO || stamped != slot.applied {
        slot.owe_full(FullReason::SurfaceChanged);
        let report = Report::Stale {
            slot: slot.id,
            stamped,
            applied: slot.applied,
        };
        return (
            RasterVerdict::SurfaceStale,
            None,
            SurfaceLoss::Kept,
            Some(report),
        );
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
            SurfaceLoss::Kept,
            None,
        );
    };
    match rendered {
        Ok(PresentDisposition::Presented) => {
            slot.full_redraw = None;
            (RasterVerdict::Presented, None, SurfaceLoss::Kept, None)
        }
        Ok(PresentDisposition::NoDamage) => {
            (RasterVerdict::NoPresent, None, SurfaceLoss::Kept, None)
        }
        Ok(PresentDisposition::NotShown) => {
            slot.owe_full(RESET);
            (RasterVerdict::NotShown, None, SurfaceLoss::Kept, None)
        }
        Err(error) => {
            slot.owe_full(RESET);
            let (verdict, surface_loss) = classify(&error);
            let report = Report::FrameFailed {
                slot: slot.id,
                error,
                verdict,
            };
            (verdict, None, surface_loss, Some(report))
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
        /// Whether the next turn retries it.
        retried: bool,
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
    /// The device was seen lost.
    DeviceLost,
    /// A rebuild failed; the device's record holds why.
    RebuildFailed(FailedRebuild, DeviceHealth),
    /// The device renders again.
    DeviceRebuilt,
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
        Report::ResizeRefused { slot, retried } => {
            tracing::warn!(
                ?slot,
                retried,
                "raster executor: resize refused; the record holds why"
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
        Report::DeviceLost => tracing::error!("raster executor: GPU device lost; rebuilding it"),
        // The first failure, the backoff reaching its ceiling and the
        // rebuild given up are errors; a failure between them is not news.
        Report::RebuildFailed(FailedRebuild::First, health) => tracing::error!(
            ?health,
            "raster executor: GPU device rebuild failed; retrying with backoff; the sender holds why"
        ),
        Report::RebuildFailed(FailedRebuild::ReachedCap, health) => tracing::error!(
            ?health,
            "raster executor: GPU device rebuild still failing at the backoff's ceiling"
        ),
        Report::RebuildFailed(FailedRebuild::Again { wait }, health) => tracing::debug!(
            ?health,
            ?wait,
            "raster executor: GPU device rebuild failed; retrying"
        ),
        Report::RebuildFailed(FailedRebuild::GivenUp, health) => tracing::error!(
            ?health,
            "raster executor: GPU device rebuild given up; the slots render again once the \
             owner sends RetryDevice"
        ),
        Report::DeviceRebuilt => tracing::warn!("raster executor: GPU device rebuilt"),
    });
}

fn report_each<const N: usize>(reports: [Option<Report>; N], first: &mut FirstPanic) {
    for each in reports.into_iter().flatten() {
        report(each, first);
    }
}

/// What a frame did to its slot's surface.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SurfaceLoss {
    /// The surface is as it was.
    Kept,
    /// The surface was lost, and the backend configures it again: the slot's
    /// next generation is minted.
    Lost,
    /// The surface was lost for good and dropped: the next generation is
    /// minted and the record asks the owner for a new surface.
    Dropped,
}

/// A render error's verdict, and what it did to the surface.
fn classify(error: &EngineError) -> (RasterVerdict, SurfaceLoss) {
    match error {
        EngineError::SurfaceReplacementNeeded => (RasterVerdict::NotShown, SurfaceLoss::Dropped),
        EngineError::DeviceLost | EngineError::GpuUnavailable | EngineError::GpuProgress { .. } => {
            (RasterVerdict::DeviceLost, SurfaceLoss::Kept)
        }
        EngineError::SurfaceLost | EngineError::SurfaceValidation => {
            (RasterVerdict::SurfaceStale, SurfaceLoss::Lost)
        }
        other if other.recoverability() == Recoverability::Recoverable => {
            (RasterVerdict::Retry, SurfaceLoss::Kept)
        }
        _ => (RasterVerdict::Failed, SurfaceLoss::Kept),
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

/// Hold the panic `caught` contained in `inbox`'s record, or discard it when
/// the record holds a failure already.
fn hold_panic(inbox: &SlotInbox, caught: FirstPanic, first: &mut FirstPanic) {
    let Some(payload) = caught.into_payload() else {
        return;
    };
    let extra = inbox.lock().hold_failure(SlotFailure::Panicked(payload));
    retire(None, extra, first);
}
