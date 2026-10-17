//! Weak participant identity and callback custody for one connected chain.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use flui_foundation::{InputSequenceId, RenderId, geometry::Axis, panic::PanicRecovery};
use flui_rendering::pipeline::WeakPipelineCell;
use smallvec::SmallVec;

use super::{
    ballistic::ScrollContinuation,
    connection::WeakConnection,
    run::SequenceRun,
    types::{OverscrollBehavior, ScrollEndReason},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParticipantKind {
    Connection,
    Owner {
        axis: Axis,
        overscroll: OverscrollBehavior,
    },
}

pub(super) struct ConnectionLink {
    pub(super) connection: WeakConnection,
    pub(crate) parent: Option<Weak<Self>>,
    pub(crate) node: Rc<Cell<Option<RenderId>>>,
    pub(crate) pipeline: Option<WeakPipelineCell>,
    pub(crate) alive: Cell<bool>,
    pub(crate) gate: Rc<DispatchGate>,
    runs: RefCell<SmallVec<[Weak<SequenceRun>; 1]>>,
}

impl ConnectionLink {
    pub(super) fn new(
        connection: WeakConnection,
        parent: Option<Weak<Self>>,
        node: Rc<Cell<Option<RenderId>>>,
        pipeline: Option<WeakPipelineCell>,
    ) -> Rc<Self> {
        let gate = parent.as_ref().and_then(Weak::upgrade).map_or_else(
            || Rc::new(DispatchGate::default()),
            |parent| parent.gate.clone(),
        );
        Rc::new(Self {
            connection,
            parent,
            node,
            pipeline,
            alive: Cell::new(true),
            gate,
            runs: RefCell::new(SmallVec::new()),
        })
    }

    pub(super) fn register(&self, run: &Rc<SequenceRun>) {
        let mut runs = self.runs.borrow_mut();
        runs.retain(|old| old.upgrade().is_some_and(|run| !run.has_ended()));
        runs.push(Rc::downgrade(run));
    }

    pub(crate) fn close(self: &Rc<Self>) {
        if !self.alive.replace(false) {
            return;
        }
        self.withdraw_runs();
    }

    /// Contain a participant failure: an owner keeps its mount for the next
    /// sequence, an authored connection stays closed until it is replaced.
    pub(super) fn fail(self: &Rc<Self>) {
        if matches!(self.connection.kind(), ParticipantKind::Owner { .. }) {
            self.withdraw_runs();
        } else {
            self.close();
        }
    }

    pub(super) fn withdraw_runs(self: &Rc<Self>) {
        let runs = std::mem::take(&mut *self.runs.borrow_mut());
        for run in runs.into_iter().filter_map(|run| run.upgrade()) {
            run.end_link(self, ScrollEndReason::Cancelled);
        }
        if let Some(_guard) = self.gate.enter() {
            let mut recovery = PanicRecovery::new();
            self.gate.drain(&mut recovery);
            recovery.finish();
        }
    }
}

#[derive(Default)]
pub(crate) struct DispatchGate {
    busy: Cell<bool>,
    ends: RefCell<Vec<EndDelivery>>,
    completions: RefCell<Vec<(ScrollContinuation, bool)>>,
}

struct EndDelivery {
    connection: WeakConnection,
    sequence: InputSequenceId,
    reason: ScrollEndReason,
}

impl DispatchGate {
    pub(super) fn complete(&self, continuation: ScrollContinuation, success: bool) {
        self.completions.borrow_mut().push((continuation, success));
    }
    pub(crate) fn enter(self: &Rc<Self>) -> Option<DispatchGuard> {
        (!self.busy.replace(true)).then(|| DispatchGuard(self.clone()))
    }

    pub(crate) fn end(
        &self,
        connection: WeakConnection,
        sequence: InputSequenceId,
        reason: ScrollEndReason,
    ) {
        self.ends.borrow_mut().push(EndDelivery {
            connection,
            sequence,
            reason,
        });
    }

    pub(crate) fn drain(&self, recovery: &mut PanicRecovery) {
        loop {
            let completions = std::mem::take(&mut *self.completions.borrow_mut());
            let had_completions = !completions.is_empty();
            for (continuation, success) in completions {
                if let Some(run) = continuation.run.upgrade() {
                    run.complete_continuation_held(&continuation, success, recovery);
                }
            }
            let ends = std::mem::take(&mut *self.ends.borrow_mut());
            if ends.is_empty() && !had_completions {
                break;
            }
            for end in ends {
                if let Some(connection) = end.connection.upgrade() {
                    recovery.run(|| {
                        connection
                            .callbacks()
                            .sequence_ended(end.sequence, end.reason);
                    });
                    recovery.retire(connection);
                }
            }
        }
    }
}

pub(crate) struct DispatchGuard(Rc<DispatchGate>);

impl Drop for DispatchGuard {
    fn drop(&mut self) {
        self.0.busy.set(false);
    }
}
