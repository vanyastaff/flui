//! The driver as a [`Connection`] (ADR-0400 §1): one launched or attached target, driven through
//! the window that had content when the connection was first used, so a scenario written over
//! the connection runs on a device as it runs headless. That window stays the connection's for
//! the session: a window the app opens later is addressed by a call's own parameters.
//!
//! A call goes the way [`Driver::call`] sends it, through the session's input policy and lanes.
//! A call the app's agent left unanswered asks the agent to release the held input of the window
//! the call addressed, and says whether it answered that it did. A tool on the host that hung is
//! the lane's failure, answered with its message. An OS lane releases what it pressed on every
//! exit of each send, so between calls it holds nothing.

use std::any::{Any, TypeId};
use std::time::Duration;

use flui_protocol::client::{At, CallError, Connection, EndChecks, Performed};
use flui_protocol::input::ActionSequence;
use flui_protocol::method::{Addressed, WindowMethod};
use flui_protocol::report::{Check, Failure};
use flui_protocol::tree::WindowId;
use flui_protocol::wait::{Condition, WaitOutcome};

use crate::compose;
use crate::driver::Driver;
use crate::error::{Awaited, Error, Result};

impl Connection for Driver {
    fn call<M: WindowMethod>(&mut self, params: M::Params) -> Result<M::Output, CallError> {
        let addressed = params.window();
        let answer = typed_call::<M>(self, params);
        answer.map_err(|error| call_error(self, addressed, error))
    }

    fn wait(&mut self, condition: &Condition, limit: Duration) -> Result<WaitOutcome, CallError> {
        let window = self.connection_window();
        let answer =
            window.and_then(|window| compose::wait_until(self, window, condition.clone(), limit));
        answer.map_err(|error| call_error(self, None, error))
    }

    fn perform(&mut self, actions: &ActionSequence) -> Result<Performed, CallError> {
        let window = self.connection_window();
        let answer = window.and_then(|window| self.window(window).perform(actions.clone()));
        answer
            .map(Performed::from)
            .map_err(|error| call_error(self, None, error))
    }

    fn now(&self) -> At {
        // The driver reads no frame number it could hold exact.
        At::new(self.clock(), self.virtual_clock())
    }

    fn finish(&mut self, checks: &EndChecks) -> Vec<Failure> {
        let released = self
            .connection_window()
            .and_then(|window| self.window(window).release_input().map(|_| ()));
        let mut failures: Vec<Failure> =
            Failure::try_from_check(Check::from_result("release held input", &released))
                .into_iter()
                .collect();
        // A device has no exact frame to settle at or audit, and no ledger or runtime state to
        // read (ADR-0400 §7): each check asked for is named, never passed.
        let unread = [
            (
                checks.settle,
                "settle",
                "a device holds no exact frame to settle at",
            ),
            (
                !checks.audit.is_empty(),
                "audit",
                "a device audits through the test's own audit, not at its end",
            ),
            (
                checks.drop_ledger,
                "drop ledger",
                "a device reads no drop ledger",
            ),
            (
                checks.left_behind,
                "nothing left behind",
                "a device reads no timer, ticker or background work",
            ),
        ];
        failures.extend(
            unread
                .into_iter()
                .filter(|(asked, ..)| *asked)
                .map(|(_, name, reason)| Failure::cannot_verify(name, reason, None)),
        );
        failures
    }
}

/// What a call that failed with `error` answers. A call the app's agent left unanswered releases
/// the held input of the window it addressed (`addressed`, or the connection's window when it
/// named none) and says whether the agent answered that it did; every other failure, a host
/// tool's timeout included, is answered as it stands.
fn call_error(driver: &mut Driver, addressed: Option<WindowId>, error: Error) -> CallError {
    if let Error::Timeout {
        awaited: Awaited::Agent,
        ..
    } = error
    {
        let window = addressed.map_or_else(|| driver.connection_window(), Ok);
        let released = window
            .and_then(|window| driver.window(window).release_input())
            .is_ok();
        return CallError::Unanswered {
            may_have_run: error.may_have_run(),
            released,
        };
    }
    CallError::failed(error.to_wire())
}

/// [`Driver::call`] of the window method `M`. The method table is sealed, so `M` is one of its
/// rows: the call is made as that row, whose parameters and output serialize.
fn typed_call<M: WindowMethod>(driver: &mut Driver, params: M::Params) -> Result<M::Output> {
    let params: Box<dyn Any> = Box::new(params);
    macro_rules! window_methods {
        ($($ty:ident = $name:literal, $effect:ident, $tier:ident, $params:ty => $output:ty;)+) => {
            $(
                if TypeId::of::<M>() == TypeId::of::<flui_protocol::method::$ty>() {
                    let params = *params
                        .downcast::<$params>()
                        .expect("BUG: a method's parameters are the type its row names");
                    let output: Box<dyn Any> =
                        Box::new(driver.call::<flui_protocol::method::$ty>(params)?);
                    return Ok(*output
                        .downcast::<M::Output>()
                        .expect("BUG: a method's output is the type its row names"));
                }
            )+
        };
    }
    flui_protocol::for_each_window_method!(window_methods);
    panic!("BUG: every window method is a row of the sealed method table")
}
