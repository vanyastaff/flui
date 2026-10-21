//! The binding-driven arena lifecycle: close on Down, sweep on Up, abandon on
//! Cancel, and a close that withdraws now and rejects members later.

use std::cell::RefCell;

use crate::{
    close::CloseMode,
    events::PointerEvent,
    gesture::{
        ContestCancel,
        arena::{DetachedArenaBatch, GestureArena},
    },
    ids::PointerId,
};

/// Arena entries are keyed by contact, and a contact belongs to the surface
/// that admitted its Down, so the arena needs no surface key of its own.
pub(super) struct ArenaDriver {
    arena: GestureArena,
    /// Generations a close detached while a dispatch was in flight; their
    /// members are rejected when the binding drains.
    withdrawn: RefCell<Vec<DetachedArenaBatch>>,
}

impl ArenaDriver {
    pub(super) fn new(arena: GestureArena) -> Self {
        Self {
            arena,
            withdrawn: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn arena(&self) -> &GestureArena {
        &self.arena
    }

    /// Settle a terminal contact's detached generation: Up sweeps it, Cancel
    /// abandons it without granting a winner.
    pub(super) fn settle(&self, terminal: &PointerEvent, batch: DetachedArenaBatch) {
        match terminal {
            PointerEvent::Up(_) => self.arena.sweep_detached(batch),
            PointerEvent::Cancel(cancel) => {
                GestureArena::abandon_detached(batch, ContestCancel::Pointer(cancel.reason));
            }
            _ => unreachable!("BUG: a terminal sequence ends with Up or Cancel"),
        }
    }

    pub(super) fn detach(&self, pointer: PointerId) -> DetachedArenaBatch {
        self.arena.detach(pointer)
    }

    /// Revoke the owner's authority without running member code, so the rest
    /// of a dispatch in flight resolves nothing.
    pub(super) fn withdraw(&self) {
        let batches = self.arena.withdraw_owner();
        self.withdrawn.borrow_mut().extend(batches);
    }

    /// Reject the members of every withdrawn generation, or retain them under
    /// a preserving close.
    pub(super) fn drain(&self, mode: CloseMode) {
        let mut batches = self.withdrawn.take();
        batches.extend(self.arena.withdraw_owner());
        self.arena.retire_owner(batches, mode);
    }
}
