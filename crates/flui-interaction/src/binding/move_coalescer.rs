//! Frame-coalesced moves: one queued packet per pointer and surface.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use smallvec::SmallVec;

use super::pointer_sequences::PointerSequence;
use crate::{
    events::PointerEvent,
    ids::{InputSurface, PointerId},
    pointer::HitTestResult,
};

/// A contact move queues under the surface that admitted the contact; a hover
/// move under the surface that reported it.
pub(super) type MoveKey = (InputSurface, PointerId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PendingMoveGeneration(u64);

/// One frame-coalesced move and the only routing state needed to deliver it.
///
/// Contact moves reuse the generation-checked route cached at pointer-down.
/// Hover moves have no contact sequence, so they retain the fresh hit-test
/// result produced for that move and are dispatched ephemerally at frame flush.
pub(super) enum PendingMove {
    Contact {
        event: PointerEvent,
        sequence: PointerSequence,
    },
    Hover {
        event: PointerEvent,
        hit_test: HitTestResult,
    },
}

/// Exact slot for a direct/coalesced move in the canonical pending map.
///
/// `pending: Some` is queued; a flush takes the payload and leaves
/// `pending: None` as its in-flight marker before invoking any user callback.
/// Re-entrant input replaces or removes that marker, making the detached
/// payload stale without a second epoch map.
pub(super) struct PendingMoveState {
    generation: PendingMoveGeneration,
    pub(super) pending: Option<PendingMove>,
}

impl PendingMoveState {
    pub(super) const fn queued(generation: PendingMoveGeneration, pending: PendingMove) -> Self {
        Self {
            generation,
            pending: Some(pending),
        }
    }

    const fn is_queued(&self) -> bool {
        self.pending.is_some()
    }

    fn is_in_flight(&self, expected: PendingMoveGeneration) -> bool {
        self.generation == expected && self.pending.is_none()
    }
}

/// A queued move taken out for delivery, leaving its in-flight marker behind.
pub(super) struct FrozenMove {
    pub(super) key: MoveKey,
    pub(super) generation: PendingMoveGeneration,
    pub(super) pending: PendingMove,
}

#[derive(Default)]
pub(super) struct MoveCoalescer {
    /// Only the latest move per pointer and surface is kept.
    pending: RefCell<HashMap<MoveKey, PendingMoveState>>,
    /// Monotonic identity for direct/coalesced move slots. Unlike a pointer
    /// sequence, hover has no Down transaction, so it needs its own identity
    /// while moving through `Queued -> InFlight`.
    next_generation: Cell<u64>,
}

impl MoveCoalescer {
    pub(super) fn len(&self) -> usize {
        self.pending.borrow().len()
    }

    pub(super) fn has_queued(&self) -> bool {
        self.pending
            .borrow()
            .values()
            .any(PendingMoveState::is_queued)
    }

    pub(super) fn queued_count(&self) -> usize {
        self.pending
            .borrow()
            .values()
            .filter(|state| state.is_queued())
            .count()
    }

    pub(super) fn contains_pointer(&self, pointer: PointerId) -> bool {
        self.pending
            .borrow()
            .keys()
            .any(|(_, queued)| *queued == pointer)
    }

    /// Whether a flush round has taken this slot's payload and not yet retired it.
    pub(super) fn has_in_flight(&self, key: MoveKey) -> bool {
        self.pending
            .borrow()
            .get(&key)
            .is_some_and(|state| state.pending.is_none())
    }

    pub(super) fn allocate_generation(&self) -> PendingMoveGeneration {
        let generation = self
            .next_generation
            .get()
            .checked_add(1)
            .unwrap_or_else(|| panic!("BUG: pending move generation exhausted"));
        self.next_generation.set(generation);
        PendingMoveGeneration(generation)
    }

    /// Commit `state` and hand back the slot it replaced, for retirement
    /// outside the map borrow.
    pub(super) fn insert(&self, key: MoveKey, state: PendingMoveState) -> Option<PendingMoveState> {
        self.pending.borrow_mut().insert(key, state)
    }

    pub(super) fn remove(&self, key: MoveKey) -> Option<PendingMoveState> {
        self.pending.borrow_mut().remove(&key)
    }

    /// Remove every slot of `pointer`, on every surface.
    pub(super) fn remove_pointer(&self, pointer: PointerId) -> SmallVec<[PendingMoveState; 1]> {
        let mut removed: SmallVec<[(MoveKey, PendingMoveState); 1]> = self
            .pending
            .borrow_mut()
            .extract_if(|(_, queued), _| *queued == pointer)
            .collect();
        removed.sort_unstable_by_key(|(key, _)| *key);
        removed.into_iter().map(|(_, state)| state).collect()
    }

    /// Remove the queued hover moves `leaving` selects, in key order: their
    /// pointer left before the frame that would deliver them. Contact moves
    /// and in-flight markers stay.
    pub(super) fn remove_queued_hovers(
        &self,
        leaving: impl Fn(MoveKey, &PointerEvent) -> bool,
    ) -> SmallVec<[HitTestResult; 1]> {
        let mut removed: SmallVec<[(MoveKey, PendingMoveState); 1]> = self
            .pending
            .borrow_mut()
            .extract_if(|key, state| {
                matches!(&state.pending, Some(PendingMove::Hover { event, .. }) if leaving(*key, event))
            })
            .collect();
        removed.sort_unstable_by_key(|(key, _)| *key);
        removed
            .into_iter()
            .filter_map(|(_, state)| match state.pending {
                Some(PendingMove::Hover { hit_test, .. }) => Some(hit_test),
                _ => None,
            })
            .collect()
    }

    /// Freeze the complete queued batch in key order before any user callback
    /// runs. Re-entrant moves replace their exact marker and therefore always
    /// belong to the next frame.
    pub(super) fn freeze(&self) -> SmallVec<[FrozenMove; 4]> {
        let mut moves = self.pending.borrow_mut();
        let mut keys: SmallVec<[MoveKey; 4]> = moves
            .iter()
            .filter_map(|(&key, state)| state.is_queued().then_some(key))
            .collect();
        keys.sort_unstable();
        keys.into_iter()
            .filter_map(|key| {
                let entry = moves
                    .get_mut(&key)
                    .expect("BUG: snapshotted pending move remains present");
                let generation = entry.generation;
                entry.pending.take().map(|pending| FrozenMove {
                    key,
                    generation,
                    pending,
                })
            })
            .collect()
    }

    pub(super) fn is_in_flight(&self, key: MoveKey, generation: PendingMoveGeneration) -> bool {
        self.pending
            .borrow()
            .get(&key)
            .is_some_and(|state| state.is_in_flight(generation))
    }

    /// Retire a delivered slot unless reentrant input replaced it.
    pub(super) fn remove_if_in_flight(
        &self,
        key: MoveKey,
        generation: PendingMoveGeneration,
    ) -> Option<PendingMoveState> {
        let mut moves = self.pending.borrow_mut();
        if moves
            .get(&key)
            .is_some_and(|state| state.is_in_flight(generation))
        {
            moves.remove(&key)
        } else {
            None
        }
    }

    /// Yield every slot in key order; the map is empty before the caller
    /// retires any payload.
    pub(super) fn drain(&self) -> Vec<(MoveKey, PendingMoveState)> {
        let mut moves: Vec<_> = self.pending.take().into_iter().collect();
        moves.sort_unstable_by_key(|(key, _)| *key);
        moves
    }
}
