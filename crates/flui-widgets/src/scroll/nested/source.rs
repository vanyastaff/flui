//! Where a leaf of the channel takes the identity of a sequence it begins
//! (ADR-0209 §5): the binding's gesture arena, which also says which
//! sequence the input it is dispatching belongs to and where its latch
//! ends.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::panic::PanicRecovery;
use flui_interaction::{GestureArena, InputSequenceId};

use super::ScrollSequence;
use super::idle_end::follow_latch;

/// The binding's sequence issuer as a leaf holds it: acquired in a
/// lifecycle hook, read when input arrives.
#[derive(Default)]
pub(crate) struct SequenceSource {
    arena: RefCell<Option<GestureArena>>,
}

impl SequenceSource {
    /// Issue from `arena` from now on.
    pub(crate) fn bind(&self, arena: GestureArena) {
        let outgoing = self.arena.replace(Some(arena));
        drop(outgoing);
    }

    /// A fresh contactless sequence; `None` before the leaf is bound or once
    /// identities are exhausted, when the leaf moves without one.
    pub(crate) fn mint(&self) -> Option<InputSequenceId> {
        let arena = self.arena.borrow().clone()?;
        match arena.begin_contactless_sequence() {
            Ok(sequence) => Some(sequence),
            Err(error) => {
                tracing::debug!(%error, "a scroll moves without a sequence: identities exhausted");
                None
            }
        }
    }

    /// The sequence the binding is dispatching input of, or a fresh one.
    pub(crate) fn dispatched_or_mint(&self) -> Option<InputSequenceId> {
        let dispatched = self
            .arena
            .borrow()
            .as_ref()
            .and_then(GestureArena::dispatched_sequence);
        dispatched.or_else(|| self.mint())
    }

    /// A wheel or trackpad packet was handled: the sequence `led` names for
    /// the packet's latch, the one the leaf leads for it, ends where the
    /// binding says the latch ends, at its idle deadline or with this packet.
    pub(crate) fn follow_dispatched_latch(
        &self,
        led: impl FnOnce(InputSequenceId) -> Option<Rc<ScrollSequence>>,
    ) {
        let Some(arena) = self.arena.borrow().clone() else {
            return;
        };
        let (Some(id), Some(end)) = (arena.dispatched_sequence(), arena.dispatched_latch_end())
        else {
            return;
        };
        if let Some(sequence) = led(id) {
            let mut recovery = PanicRecovery::new();
            follow_latch(&sequence, &arena, end, &mut recovery);
            recovery.finish();
        }
    }
}
