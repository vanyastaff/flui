//! One source's cursor into the connected scroll chain.

use std::{cell::RefCell, rc::Rc};

use flui_foundation::{
    InputSequenceId,
    geometry::{Point, Vec2},
    panic::PanicRecovery,
};

use super::{
    link::ConnectionLink,
    run::{ScrollOutcome, SequenceRun},
    types::{ScrollCause, ScrollDevice, ScrollEndReason},
};

/// Held by a mounted input owner, never behind a callback-time widget borrow.
pub(crate) struct NestedScrollDispatcher {
    leaf: Rc<ConnectionLink>,
    current: RefCell<Option<Rc<SequenceRun>>>,
}

impl NestedScrollDispatcher {
    pub(crate) fn recover_cancelled(
        &self,
        sequence: InputSequenceId,
        focal: Point,
        device_pixel_ratio: f64,
    ) {
        let current = self.current.borrow().clone();
        let Some(run) = current.filter(|run| run.sequence == sequence && !run.has_ended()) else {
            return;
        };
        let Some(_guard) = self.leaf.gate.enter() else {
            return;
        };
        let mut recovery = PanicRecovery::new();
        run.recover_cancelled(focal, device_pixel_ratio, &mut recovery);
        self.leaf.gate.drain(&mut recovery);
        recovery.finish();
    }
    pub(crate) fn release(
        &self,
        sequence: InputSequenceId,
        focal: Point,
        velocity: Vec2,
        device_pixel_ratio: f64,
    ) {
        let current = self.current.borrow().clone();
        let Some(run) = current.filter(|run| run.sequence == sequence && !run.has_ended()) else {
            return;
        };
        let Some(_guard) = self.leaf.gate.enter() else {
            return;
        };
        let mut recovery = PanicRecovery::new();
        run.release(focal, velocity, device_pixel_ratio, &mut recovery);
        self.leaf.gate.drain(&mut recovery);
        recovery.finish();
    }
    pub(super) fn new(leaf: Rc<ConnectionLink>) -> Self {
        Self {
            leaf,
            current: RefCell::new(None),
        }
    }

    pub(crate) fn dispatch(
        &self,
        sequence: InputSequenceId,
        device: ScrollDevice,
        cause: ScrollCause,
        focal: Point,
        available: Vec2,
    ) -> Option<ScrollOutcome> {
        if !self.leaf.alive.get() || !available.is_finite() {
            return None;
        }
        let Some(_guard) = self.leaf.gate.enter() else {
            tracing::warn!(?sequence, "reentrant nested scroll dispatch refused");
            return None;
        };
        let existing = self.current.borrow().clone();
        let run = if let Some(run) =
            existing.filter(|run| run.sequence == sequence && !run.has_ended())
        {
            run
        } else {
            let incoming = SequenceRun::new(&self.leaf, sequence, device);
            let outgoing = self.current.replace(Some(incoming.clone()));
            if let Some(outgoing) = outgoing {
                outgoing.end(ScrollEndReason::Preempted);
            }
            incoming
        };
        let mut recovery = PanicRecovery::new();
        self.leaf.gate.drain(&mut recovery);
        let outcome = run.dispatch(focal, available, cause, &mut recovery);
        self.leaf.gate.drain(&mut recovery);
        recovery.finish();
        Some(outcome)
    }

    pub(crate) fn end(&self, sequence: InputSequenceId, reason: ScrollEndReason) {
        let outgoing = {
            let mut current = self.current.borrow_mut();
            if current.as_ref().is_none_or(|run| run.sequence != sequence) {
                return;
            }
            current.take()
        };
        if let Some(outgoing) = outgoing {
            outgoing.end(reason);
        }
        if let Some(_guard) = self.leaf.gate.enter() {
            let mut recovery = PanicRecovery::new();
            self.leaf.gate.drain(&mut recovery);
            recovery.finish();
        }
    }

    pub(crate) fn close(&self) {
        let outgoing = self.current.borrow_mut().take();
        if let Some(outgoing) = outgoing {
            outgoing.end(ScrollEndReason::Cancelled);
        }
        self.leaf.close();
    }
}
