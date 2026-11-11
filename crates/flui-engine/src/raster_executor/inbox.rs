//! A slot's inbox and record: what the owner sends it and what the executor
//! settles, behind the one lock the executor's protocol has (ADR-0305 §3–§6).
//!
//! The lock is held only to swap values. Nothing runs under it that the
//! inbox does not own: a superseded or settled scene is dropped, a panic
//! payload discarded, the host woken and every `tracing` event emitted after
//! it is released, since each of those can run user code.

use std::{any::Any, collections::VecDeque, fmt};

use flui_foundation::{PresentationAddress, SurfaceGeneration, geometry::DeviceSize};
use flui_layer::SceneSnapshot;
use parking_lot::{Mutex, MutexGuard};

use super::command::{RasterVerdict, SlotSettlement, SlotVisibility};
use crate::error::EngineError;

/// How many settlements an inline slot holds undrained: the executor starts
/// no frame of a slot whose ring is full (ADR-0305 §4, §5).
pub(crate) const INLINE_DEPTH: usize = 1;

/// Where a slot is in its life, as its record reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPhase {
    /// Sent to the executor and not attached by it yet.
    Attaching,
    /// Attached: its frames render.
    Attached,
    /// The executor could not attach it; [`RasterSender::take_failure`]
    /// says why. Detach it to free its id.
    ///
    /// [`RasterSender::take_failure`]: super::RasterSender::take_failure
    Refused,
    /// Taken down, by a detach or by the executor stopping. Its id is free
    /// once the owner mints again.
    Detached,
}

/// Why a slot failed: the first failure since the owner last took one.
pub enum SlotFailure {
    /// The backend refused to attach the slot.
    Refused(EngineError),
    /// A command on the slot panicked. The owner reports the payload where
    /// the slot's presentation is (ADR-0258 §1) and resumes or discards it.
    Panicked(Box<dyn Any + Send>),
}

impl fmt::Debug for SlotFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(error) => f.debug_tuple("Refused").field(error).finish(),
            Self::Panicked(payload) => f
                .debug_tuple("Panicked")
                .field(&flui_foundation::panic::payload_text(payload.as_ref()))
                .finish(),
        }
    }
}

/// A slot's record as the owner reads it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotRecord {
    /// Where the slot is in its life.
    pub phase: SlotPhase,
    /// Frames accepted and not settled yet: the count the owner's in-flight
    /// gate paces production by.
    pub in_flight: u32,
    /// Settlements written and not drained yet.
    pub undrained: usize,
    /// The generation the slot's next frame must be stamped with.
    pub required_generation: SurfaceGeneration,
}

/// The values one slot's owner and executor exchange.
pub(crate) struct InboxState {
    /// The frame waiting for the executor; a later frame supersedes it.
    pub(crate) frame: Option<SceneSnapshot>,
    /// The latest size asked for and not applied yet.
    pub(crate) size: Option<DeviceSize>,
    /// The latest visibility asked for and not applied yet.
    pub(crate) visibility: Option<SlotVisibility>,
    /// The slot's one surface-generation counter (ADR-0305 §6): a resize
    /// mints here, and so does the executor when the surface is lost.
    pub(crate) generation: SurfaceGeneration,
    /// Frames accepted and not settled yet.
    pub(crate) in_flight: u32,
    /// Settlements not drained yet, oldest first.
    pub(crate) settlements: VecDeque<SlotSettlement>,
    pub(crate) phase: SlotPhase,
    pub(crate) failure: Option<SlotFailure>,
}

/// A slot's inbox, shared by its owner and the executor.
pub(crate) struct SlotInbox {
    /// The presentation the slot is bound to.
    pub(crate) address: PresentationAddress,
    state: Mutex<InboxState>,
}

impl SlotInbox {
    pub(crate) fn new(address: PresentationAddress, generation: SurfaceGeneration) -> Self {
        Self {
            address,
            state: Mutex::new(InboxState {
                frame: None,
                size: None,
                visibility: None,
                generation,
                in_flight: 0,
                settlements: VecDeque::with_capacity(INLINE_DEPTH),
                phase: SlotPhase::Attaching,
                failure: None,
            }),
        }
    }

    /// The state, locked. Callers swap values and release it before running
    /// anything that can be user code.
    pub(crate) fn lock(&self) -> MutexGuard<'_, InboxState> {
        self.state.lock()
    }

    pub(crate) fn record(&self) -> SlotRecord {
        let state = self.lock();
        SlotRecord {
            phase: state.phase,
            in_flight: state.in_flight,
            undrained: state.settlements.len(),
            required_generation: state.generation,
        }
    }
}

impl InboxState {
    /// Settle `frame` (the one the executor took): one fewer in flight.
    pub(crate) fn settle(&mut self, frame: &SceneSnapshot, verdict: RasterVerdict) {
        debug_assert!(self.in_flight > 0, "BUG: a settled frame was in flight");
        self.in_flight = self.in_flight.saturating_sub(1);
        self.settlements.push_back(SlotSettlement {
            address: frame.stamp.address,
            epoch: frame.stamp.epoch,
            verdict,
        });
    }

    /// Keep `failure` if none is held; hand it back otherwise, for the caller
    /// to discard once the lock is released.
    #[must_use]
    pub(crate) fn hold_failure(&mut self, failure: SlotFailure) -> Option<SlotFailure> {
        if self.failure.is_none() {
            self.failure = Some(failure);
            None
        } else {
            Some(failure)
        }
    }
}
