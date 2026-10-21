//! Wheel and trackpad scroll latches: the first consumer of a source's
//! sequence on a surface keeps it until the sequence ends.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use flui_foundation::geometry::Point;
use flui_foundation::{IdMint, InputSequenceId};
use flui_platform_api::pointer::{ScrollEvent, ScrollPhase};

use super::{SignalSource, pointer_sequences::MAX_SIMULTANEOUS_POINTERS};
use crate::{
    events::PointerInfo, gesture::settings::LatchIdle, ids::InputSurface, pointer::ScrollRoute,
};

/// Rc identity is the admission authority: a reentrant Begin cannot be
/// overwritten by the Stop or terminal cleanup of an older scroll round.
/// No user callback or capture is owned by this record.
pub(super) struct ScrollSequence {
    /// The wheel or trackpad sequence this latch serves (ADR-0207 §4).
    pub(super) id: InputSequenceId,
    pub(super) route: Cell<Option<ScrollRoute>>,
    /// The arena contact this sequence opened because feeds were on the hit
    /// path at its start (ADR-0251). Such a sequence never claims a route.
    pub(super) contact: Cell<Option<ScrollContact>>,
    last_packet: Cell<flui_platform_api::EventTime>,
    /// How long the latch may idle after its last packet: the preset's
    /// phase-less or phased value (ADR-0209 §5).
    idle: Cell<std::time::Duration>,
}

impl ScrollSequence {
    /// When the latch lapses without another packet: its idle time after
    /// the last one, on the owner clock [`prepare`](ScrollSequences::prepare)
    /// reads. `None` only past the end of the event timeline.
    pub(super) fn lapses_at(&self) -> Option<flui_platform_api::EventTime> {
        self.last_packet.get().checked_add(self.idle.get())
    }
}

/// The arena contact a phased touchpad pan opened: the virtual pointer that
/// carries it and where its finger-displacement integral stands.
#[derive(Debug, Clone, Copy)]
pub(super) struct ScrollContact {
    pub(super) surface: InputSurface,
    pub(super) pointer: PointerInfo,
    pub(super) position: Point<f64>,
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
    /// Sources whose momentum packets belong to a recognizer that took the
    /// touchpad pan: the platform keeps scrolling after the fingers lift, and
    /// the recognizer already ended with its own velocity (ADR-0251 §5).
    momentum_dropped: RefCell<HashSet<(InputSurface, SignalSource)>>,
}

impl ScrollSequences {
    /// Commit source admission (or terminal withdrawal) before hit testing and
    /// observers, which may reenter the binding.
    ///
    /// A latch idle past its deadline on the owner clock ends here: a
    /// phase-less wheel burst after the preset's contactless idle time, a
    /// phased one that never sent its end phase after the longer phased
    /// time. The next packet then mints a new sequence.
    pub(super) fn prepare(
        &self,
        surface: InputSurface,
        event: &ScrollEvent,
        now: flui_platform_api::EventTime,
        idle: LatchIdle,
        mint: &IdMint<InputSequenceId>,
        orphans: &mut Vec<ScrollContact>,
    ) -> ScrollAdmission {
        let key = (surface, SignalSource::from(event.pointer));
        let mut sequences = self.latches.borrow_mut();
        sequences.retain(|_, sequence| {
            let live =
                now.saturating_duration_since(sequence.last_packet.get()) < sequence.idle.get();
            if !live {
                orphans.extend(sequence.contact.take());
            }
            live
        });
        let idle = if event.phase.is_none() {
            idle.phase_less
        } else {
            idle.phased
        };
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
        ) && let Some(replaced) = sequences.remove(&key)
        {
            orphans.extend(replaced.contact.take());
        }
        if let Some(sequence) = sequences.get(&key) {
            sequence.last_packet.set(now);
            sequence.idle.set(idle);
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
            contact: Cell::new(None),
            last_packet: Cell::new(now),
            idle: Cell::new(idle),
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

    /// End `source`'s latch on every surface: the source cancelled or left. The
    /// contacts those latches opened are returned, so the caller cancels them.
    pub(super) fn withdraw_source(&self, source: SignalSource) -> Vec<ScrollContact> {
        let mut orphans = Vec::new();
        self.latches.borrow_mut().retain(|(_, latched), sequence| {
            let keep = *latched != source;
            if !keep {
                orphans.extend(sequence.contact.take());
            }
            keep
        });
        self.momentum_dropped
            .borrow_mut()
            .retain(|(_, dropped)| *dropped != source);
        orphans
    }

    /// Record that `source`'s momentum now belongs to nobody.
    pub(super) fn drop_momentum_of(&self, surface: InputSurface, pointer: PointerInfo) {
        self.momentum_dropped
            .borrow_mut()
            .insert((surface, SignalSource::from(pointer)));
    }

    /// Whether `event` is a momentum packet of a pan a recognizer took. Any
    /// other packet of the source, or the end of the momentum, ends the drop.
    pub(super) fn drops_momentum(&self, surface: InputSurface, event: &ScrollEvent) -> bool {
        let key = (surface, SignalSource::from(event.pointer));
        let mut dropped = self.momentum_dropped.borrow_mut();
        match event.phase {
            Some(ScrollPhase::MomentumBegan | ScrollPhase::MomentumChanged) => {
                dropped.contains(&key)
            }
            Some(ScrollPhase::MomentumEnded) => dropped.remove(&key),
            _ => {
                dropped.remove(&key);
                false
            }
        }
    }

    /// Release every latch. The records own no user code.
    pub(super) fn drain(&self) {
        drop(self.latches.take());
        self.momentum_dropped.take();
    }
}
