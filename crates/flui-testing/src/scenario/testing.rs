//! Doubles for the scenario tester's own unit tests (ADR-0243): what the tester asks of its
//! connection, which no test through the public API can see.

use std::any::Any;
use std::time::Duration;

use flui_protocol::client::{CallError, ClockReading, Connection, EndChecks, Performed};
use flui_protocol::input::ActionSequence;
use flui_protocol::locator::LocateParams;
use flui_protocol::method::WindowMethod;
use flui_protocol::report::UnpassedCheck;
use flui_protocol::wait::{Condition, WaitOutcome};

/// A connection that forwards every request to `inner` and records the `max_matches` of each
/// `semantics.locate` it carries, in order.
pub(crate) struct RecordingConnection<C> {
    inner: C,
    located: Vec<Option<u32>>,
}

impl<C> RecordingConnection<C> {
    pub(crate) fn new(inner: C) -> Self {
        Self {
            inner,
            located: Vec::new(),
        }
    }

    /// The `max_matches` of each locate request so far.
    pub(crate) fn located(&self) -> &[Option<u32>] {
        &self.located
    }
}

impl<C: Connection> Connection for RecordingConnection<C> {
    fn call<M: WindowMethod>(&mut self, params: M::Params) -> Result<M::Output, CallError> {
        if let Some(locate) = (&params as &dyn Any).downcast_ref::<LocateParams>() {
            self.located.push(locate.max_matches);
        }
        self.inner.call::<M>(params)
    }

    fn wait(&mut self, condition: &Condition, limit: Duration) -> Result<WaitOutcome, CallError> {
        self.inner.wait(condition, limit)
    }

    fn perform(&mut self, actions: &ActionSequence) -> Result<Performed, CallError> {
        self.inner.perform(actions)
    }

    fn now(&self) -> ClockReading {
        self.inner.now()
    }

    fn finish(&mut self, checks: &EndChecks) -> Vec<UnpassedCheck> {
        self.inner.finish(checks)
    }
}
