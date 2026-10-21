//! When a binding's close retires its state: at once when idle, or when the
//! outermost dispatch in flight returns.

use std::cell::Cell;

use crate::close::{CloseMode, CloseTombstone};

/// Whether a close retires its state now or after the dispatch in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Retirement {
    Now,
    AfterDispatch,
}

/// Closing sets a flag. Retired state is yielded by each service's `drain`
/// only once no dispatch of this binding is on the stack, so no callback a
/// close runs can observe a service mid-transaction.
#[derive(Default)]
pub(super) struct CloseCoordinator {
    closed: Cell<bool>,
    tombstone: CloseTombstone,
    /// Entry points of this binding currently on the stack.
    depth: Cell<usize>,
    /// The close owed to the outermost dispatch; preserving dominates.
    pending: Cell<Option<CloseMode>>,
    draining: Cell<bool>,
}

impl CloseCoordinator {
    pub(super) fn is_closed(&self) -> bool {
        self.closed.get()
    }

    pub(super) fn tombstone(&self) -> &CloseTombstone {
        &self.tombstone
    }

    /// A closed binding no longer delivers on its retained routes, except to
    /// the cancellations its own drain owes them.
    pub(super) fn withholds_delivery(&self) -> bool {
        self.closed.get() && !self.draining.get()
    }

    pub(super) fn close(&self, mode: CloseMode) -> Retirement {
        self.closed.set(true);
        if self.depth.get() == 0 {
            return Retirement::Now;
        }
        let owed = match (self.pending.get(), mode) {
            (Some(CloseMode::PreservingFailure), _) | (_, CloseMode::PreservingFailure) => {
                CloseMode::PreservingFailure
            }
            _ => CloseMode::Ordinary,
        };
        self.pending.set(Some(owed));
        Retirement::AfterDispatch
    }

    pub(super) fn enter(&self) {
        self.depth.set(
            self.depth
                .get()
                .checked_add(1)
                .expect("BUG: binding dispatch depth overflow"),
        );
    }

    /// Leave a dispatch; `true` when it was the outermost one.
    pub(super) fn leave(&self) -> bool {
        let depth = self
            .depth
            .get()
            .checked_sub(1)
            .expect("BUG: binding dispatch left more often than entered");
        self.depth.set(depth);
        depth == 0
    }

    /// The close a returning outermost dispatch owes, if any.
    pub(super) fn take_pending(&self) -> Option<CloseMode> {
        self.pending.take()
    }

    /// Mark the binding's own drain in progress until the scope drops, so
    /// the cancellations it owes still reach the routes it retires.
    pub(super) fn draining(&self) -> DrainScope<'_> {
        DrainScope {
            close: self,
            outer: self.draining.replace(true),
        }
    }
}

pub(super) struct DrainScope<'a> {
    close: &'a CloseCoordinator,
    outer: bool,
}

impl Drop for DrainScope<'_> {
    fn drop(&mut self) {
        self.close.draining.set(self.outer);
    }
}
