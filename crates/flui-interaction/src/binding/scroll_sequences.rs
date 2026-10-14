//! Wheel and trackpad scroll latches: the first consumer of a source's
//! sequence on a surface keeps it until the sequence ends.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use flui_foundation::{IdMint, InputSequenceId};
use flui_platform_api::pointer::{ScrollEvent, ScrollPhase};

use super::{SignalSource, pointer_sequences::MAX_SIMULTANEOUS_POINTERS};
use crate::{events::PointerInfo, ids::InputSurface, pointer::ScrollRoute};

/// A phase-less wheel burst ends after owner-clock inactivity, independently
/// of native timestamps (which may be absent or restart).
const SCROLL_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Rc identity is the admission authority: a reentrant Begin cannot be
/// overwritten by the Stop or terminal cleanup of an older scroll round.
/// No user callback or capture is owned by this record.
pub(super) struct ScrollSequence {
    /// The wheel or trackpad sequence this latch serves (ADR-0207 §4).
    pub(super) id: InputSequenceId,
    pub(super) route: Cell<Option<ScrollRoute>>,
    last_packet: Cell<flui_platform_api::EventTime>,
    phase_less: Cell<bool>,
}

pub(super) enum ScrollAdmission {
    Active(Rc<ScrollSequence>),
    Terminal(Option<Rc<ScrollSequence>>),
    Refused,
}

/// First consumptive scroll route per source and surface, bounded like
/// contact routes.
#[derive(Default)]
pub(super) struct ScrollSequences {
    latches: RefCell<HashMap<(InputSurface, SignalSource), Rc<ScrollSequence>>>,
}

impl ScrollSequences {
    /// Commit source admission (or terminal withdrawal) before hit testing and
    /// observers, which may reenter the binding.
    pub(super) fn prepare(
        &self,
        surface: InputSurface,
        event: &ScrollEvent,
        now: flui_platform_api::EventTime,
        mint: &IdMint<InputSequenceId>,
    ) -> ScrollAdmission {
        let key = (surface, SignalSource::from(event.pointer));
        let mut sequences = self.latches.borrow_mut();
        sequences.retain(|_, sequence| {
            !sequence.phase_less.get()
                || now.saturating_duration_since(sequence.last_packet.get())
                    < SCROLL_INACTIVITY_TIMEOUT
        });
        if matches!(
            event.phase,
            Some(ScrollPhase::Ended | ScrollPhase::Cancelled | ScrollPhase::MomentumEnded)
        ) {
            // Withdrawal precedes terminal observers and consumer callbacks.
            return ScrollAdmission::Terminal(sequences.remove(&key));
        }
        if matches!(
            event.phase,
            Some(ScrollPhase::Began | ScrollPhase::MomentumBegan)
        ) {
            sequences.remove(&key);
        }
        if let Some(sequence) = sequences.get(&key) {
            sequence.last_packet.set(now);
            sequence.phase_less.set(event.phase.is_none());
            return ScrollAdmission::Active(Rc::clone(sequence));
        }
        if sequences.len() >= MAX_SIMULTANEOUS_POINTERS {
            // Refuse consumption rather than publish an unlatched gesture.
            // Its fresh observers still receive the packet.
            return ScrollAdmission::Refused;
        }
        let Ok(id) = mint.next() else {
            // An exhausted binding refuses new latches permanently.
            return ScrollAdmission::Refused;
        };
        let sequence = Rc::new(ScrollSequence {
            id,
            route: Cell::new(None),
            last_packet: Cell::new(now),
            phase_less: Cell::new(event.phase.is_none()),
        });
        sequences.insert(key, Rc::clone(&sequence));
        ScrollAdmission::Active(sequence)
    }

    pub(super) fn is_current(
        &self,
        surface: InputSurface,
        pointer: PointerInfo,
        sequence: &Rc<ScrollSequence>,
    ) -> bool {
        self.latches
            .borrow()
            .get(&(surface, SignalSource::from(pointer)))
            .is_some_and(|current| Rc::ptr_eq(current, sequence))
    }

    /// End `source`'s latch on every surface: the source cancelled or left.
    pub(super) fn withdraw_source(&self, source: SignalSource) {
        self.latches
            .borrow_mut()
            .retain(|(_, latched), _| *latched != source);
    }

    /// Release every latch. The records own no user code.
    pub(super) fn drain(&self) {
        drop(self.latches.take());
    }
}
