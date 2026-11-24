//! The driver as a [`Connection`] (ADR-0400 §1): one launched or attached target, driven through
//! its first window, so a scenario written over the connection runs on a device as it runs
//! headless.
//!
//! A call goes the way [`Driver::call`] sends it, through the session's input policy and lanes.
//! A call whose answer does not arrive asks the agent to release the window's held input, and
//! says whether it answered that it did. An OS lane releases what it pressed on every exit of each
//! send, so between calls it holds nothing.

use std::any::{Any, TypeId};
use std::time::Duration;

use flui_protocol::client::{At, CallError, Connection, EndChecks, Performed};
use flui_protocol::input::ActionSequence;
use flui_protocol::method::WindowMethod;
use flui_protocol::report::{Check, Failure};
use flui_protocol::tree::WindowId;
use flui_protocol::wait::{Condition, WaitOutcome};

use crate::compose;
use crate::driver::Driver;
use crate::error::{Error, Result};

impl Connection for Driver {
    fn call<M: WindowMethod>(&mut self, params: M::Params) -> Result<M::Output, CallError> {
        let answer = typed_call::<M>(self, params);
        answer.map_err(|error| call_error(self, error))
    }

    fn wait(&mut self, condition: &Condition, limit: Duration) -> Result<WaitOutcome, CallError> {
        let answer = first_window(self)
            .and_then(|window| compose::wait_until(self, window, condition.clone(), limit));
        answer.map_err(|error| call_error(self, error))
    }

    fn perform(&mut self, actions: &ActionSequence) -> Result<Performed, CallError> {
        let answer = self
            .main_window()
            .and_then(|mut window| window.perform(actions.clone()));
        answer
            .map(Performed::from)
            .map_err(|error| call_error(self, error))
    }

    fn now(&self) -> At {
        // The driver reads no frame number it could hold exact.
        At::new(self.clock(), self.virtual_clock())
    }

    fn finish(&mut self, checks: &EndChecks) -> Vec<Failure> {
        let released =
            first_window(self).and_then(|window| self.window(window).release_input().map(|_| ()));
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

/// The window the connection drives: the target's first window with content.
fn first_window(driver: &mut Driver) -> Result<WindowId> {
    driver.main_window().map(|window| window.id())
}

/// What a call that failed with `error` answers: an answer that did not arrive releases the
/// window's held input first, and says whether the agent answered that it did.
fn call_error(driver: &mut Driver, error: Error) -> CallError {
    if let Error::Timeout { .. } = error {
        let released = first_window(driver)
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
