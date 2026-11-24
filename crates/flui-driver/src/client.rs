//! The driver as a [`Connection`] (ADR-0400 §1): one launched or attached target, driven through
//! one window, the window that had content when the connection was first used. Every call, wait,
//! input and end addresses that window: a call whose parameters name none is sent to it, and one
//! that names another is refused, so the connection's end releases all the input it held. A
//! window the app opens later never takes its place, and once it closes every operation answers
//! `gone`: the connection is not moved to another window.
//!
//! A call goes the way [`Driver::call`] sends it, through the session's input policy and lanes.
//! A call the app's agent left unanswered asks the agent to release the window's held input, and
//! says whether it answered that it did. A tool on the host that hung is the lane's failure,
//! answered with its message. An OS lane releases what it pressed on every exit of each send, so
//! between calls it holds nothing.

use std::any::{Any, TypeId};
use std::time::Duration;

use flui_protocol::client::{CallError, ClockReading, Connection, EndChecks, Performed};
use flui_protocol::error::HandleKind;
use flui_protocol::input::ActionSequence;
use flui_protocol::method::{Addressed, WindowMethod};
use flui_protocol::report::{Check, UnpassedCheck};
use flui_protocol::tree::WindowId;
use flui_protocol::wait::{Condition, WaitOutcome};

use crate::compose;
use crate::driver::Driver;
use crate::error::{Awaited, Error, Result};

impl Connection for Driver {
    fn call<M: WindowMethod>(&mut self, mut params: M::Params) -> Result<M::Output, CallError> {
        let window = self
            .connection_window()
            .map_err(|error| call_error(self, None, error))?;
        params.address(window);
        if let Some(named) = params.window().filter(|named| *named != window) {
            return Err(CallError::failed(
                Error::invalid_argument(format!(
                    "the connection drives window {window}, and the call names window {named}"
                ))
                .to_wire(),
            ));
        }
        typed_call::<M>(self, params).map_err(|error| call_error(self, Some(window), error))
    }

    fn wait(&mut self, condition: &Condition, limit: Duration) -> Result<WaitOutcome, CallError> {
        let window = self
            .connection_window()
            .map_err(|error| call_error(self, None, error))?;
        compose::wait_until(self, window, condition.clone(), limit)
            .map_err(|error| call_error(self, Some(window), error))
    }

    fn perform(&mut self, actions: &ActionSequence) -> Result<Performed, CallError> {
        let window = self
            .connection_window()
            .map_err(|error| call_error(self, None, error))?;
        self.window(window)
            .perform(actions.clone())
            .map(Performed::from)
            .map_err(|error| call_error(self, Some(window), error))
    }

    fn now(&self) -> ClockReading {
        // The driver reads no frame number it could hold exact.
        ClockReading::new(self.clock(), self.virtual_clock())
    }

    fn finish(&mut self, checks: &EndChecks) -> Vec<UnpassedCheck> {
        let released = self
            .connection_window()
            .and_then(|window| release(self, window));
        let mut unpassed: Vec<UnpassedCheck> =
            UnpassedCheck::try_from_check(Check::from_result("release held input", &released))
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
        unpassed.extend(
            unread
                .into_iter()
                .filter(|(asked, ..)| *asked)
                .map(|(_, name, reason)| UnpassedCheck::cannot_verify(name, reason, None)),
        );
        unpassed
    }
}

/// What a call that failed with `error` answers. A call the app's agent left unanswered releases
/// the held input of the connection's window, when one was resolved (`window`), and says whether
/// the agent answered that it did; every other failure, a host tool's timeout included, is
/// answered as it stands.
fn call_error(driver: &mut Driver, window: Option<WindowId>, error: Error) -> CallError {
    if let Error::Timeout {
        awaited: Awaited::Agent,
        ..
    } = error
    {
        let released = window.is_some_and(|window| release(driver, window).is_ok());
        return CallError::Unanswered {
            may_have_run: error.may_have_run(),
            released,
        };
    }
    CallError::failed(error.to_wire())
}

/// Ask the agent to release the input `window`'s lease holds. A window that closed holds none:
/// its lease went with it, so the agent's `gone` for it is a release.
fn release(driver: &mut Driver, window: WindowId) -> Result<()> {
    match driver.window(window).release_input() {
        Ok(_)
        | Err(Error::Gone {
            kind: Some(HandleKind::Window),
            ..
        }) => Ok(()),
        Err(error) => Err(error),
    }
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
