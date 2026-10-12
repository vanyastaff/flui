//! Borrow-free custody for synchronous reentry through public arena handles.
//!
//! An operation owns its recovery; this stack carries only passive preservation
//! signals and owned receipts from nested public calls. No callback, member or
//! borrowed recovery is stored here. Stack and queue borrows end before callouts.
//! The usual owner and one nested operation use inline storage.

use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
};

use flui_foundation::panic::{FailureClass, PanicRecovery, RecoveryFailure, RecoveryScope};
use smallvec::SmallVec;

#[derive(Default)]
pub(super) struct ArenaRecovery {
    frames: RefCell<SmallVec<[RecoveryFrame; 2]>>,
}

struct RecoveryFrame {
    /// A pending failure already makes opaque retirement unsafe, even before
    /// the enclosing callback returns and imports the receipt into its owner.
    preserving: bool,
    pending: SmallVec<[RecoveryFailure; 1]>,
}

struct Operation<'a> {
    recovery: &'a ArenaRecovery,
    nested: bool,
}

impl ArenaRecovery {
    fn enter(&self, preserving: bool) -> Operation<'_> {
        let mut frames = self.frames.borrow_mut();
        let nested = !frames.is_empty();
        let preserving = preserving || frames.last().is_some_and(|frame| frame.preserving);
        frames.push(RecoveryFrame {
            preserving,
            pending: SmallVec::new(),
        });
        Operation {
            recovery: self,
            nested,
        }
    }

    pub(super) fn preserving(&self) -> bool {
        self.frames
            .borrow()
            .last()
            .is_some_and(|frame| frame.preserving)
    }

    fn mark_failure(&self) {
        for frame in self.frames.borrow_mut().iter_mut() {
            frame.preserving = true;
        }
    }

    pub(super) fn drain(&self, scope: &mut RecoveryScope<'_>) {
        let pending = self
            .frames
            .borrow_mut()
            .last_mut()
            .map(|frame| std::mem::take(&mut frame.pending));
        if let Some(pending) = pending {
            // Import authority before applying any preservation signal. Turning
            // the receiver into an inherited owner first would demote its own
            // earliest authoritative receipt.
            for failure in pending {
                scope.capture_failure(failure);
            }
        }
        if scope.has_failure() {
            self.mark_failure();
        }
    }

    /// Nested receipts precede an escaping callback's own failure.
    pub(super) fn invoke(
        &self,
        class: FailureClass,
        scope: &mut RecoveryScope<'_>,
        action: impl FnOnce(&mut RecoveryScope<'_>),
    ) -> bool {
        self.drain(scope);
        let outcome = catch_unwind(AssertUnwindSafe(|| action(scope)));
        self.drain(scope);
        let returned = outcome.is_ok();
        if let Err(payload) = outcome {
            scope.capture_classified(class, payload);
            self.mark_failure();
        }
        returned
    }

    pub(super) fn scoped(
        &self,
        scope: &mut RecoveryScope<'_>,
        action: impl FnOnce(&mut RecoveryScope<'_>),
    ) {
        let operation = self.enter(scope.has_failure());
        self.invoke(FailureClass::Unknown, scope, action);
        drop(operation);
    }

    pub(super) fn standalone(&self, action: impl FnOnce(&mut RecoveryScope<'_>)) {
        let mut recovery = PanicRecovery::new();
        if self.preserving() {
            recovery.inherit_failure();
        }
        let operation = self.enter(recovery.has_failure());
        self.invoke(FailureClass::Unknown, &mut recovery.scope(), action);
        let nested = operation.nested;
        drop(operation);
        if nested {
            // A nested public API cannot resume across its enclosing callback:
            // that callback's class says nothing about nested opaque retirement.
            if let Some(failure) = recovery.into_failure() {
                self.mark_failure();
                self.frames
                    .borrow_mut()
                    .last_mut()
                    .expect("BUG: nested arena operation has an enclosing frame")
                    .pending
                    .push(failure);
            }
        } else {
            recovery.finish();
        }
    }
}

impl Drop for Operation<'_> {
    fn drop(&mut self) {
        let outgoing = self.recovery.frames.borrow_mut().pop();
        drop(outgoing);
    }
}
