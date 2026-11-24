//! A client's connection to one window of an application (ADR-0400 §1): the one surface a test
//! drives at every depth, from a view mounted in process to an application on a device.
//!
//! A [`Connection`] names protocol types and `std` alone: it is a contract with no runtime behind
//! it, so the test kit and the driver implement the same trait and a scenario written over it
//! runs on either. Its window is the one the connection was opened on; a call's parameters may
//! still address another ([`Addressed::window`](crate::method::Addressed::window)).

use std::fmt;
use std::time::Duration;

use crate::act::{ActOutcome, Effect, JournalSpan};
use crate::audit::AuditRule;
use crate::error::{ErrorCode, ErrorObject};
use crate::input::{ActionSequence, InputPath};
use crate::method::WindowMethod;
use crate::report::{CheckFailure, Failure};
use crate::wait::{Condition, WaitOutcome};

/// A connection to one window of an application: see the [module docs](self).
pub trait Connection {
    /// One request to the window's agent. Never runs a frame and never moves the clock.
    ///
    /// # Errors
    ///
    /// [`CallError::Failed`] with what the application or the connection refused;
    /// [`CallError::Unanswered`] when no answer arrived.
    fn call<M: WindowMethod>(&mut self, params: M::Params) -> Result<M::Output, CallError>;

    /// Hold out until the application answers `condition` (ADR-0400 §3), at most `limit` of the
    /// connection's clock. A wait that runs out is an answer, unsatisfied, not an error.
    ///
    /// # Errors
    ///
    /// As [`Self::call`].
    fn wait(&mut self, condition: &Condition, limit: Duration) -> Result<WaitOutcome, CallError>;

    /// Perform `actions` on the window, by the connection's input path.
    ///
    /// # Errors
    ///
    /// As [`Self::call`].
    fn perform(&mut self, actions: &ActionSequence) -> Result<Performed, CallError>;

    /// Where the connection's clock stands.
    fn now(&self) -> At;

    /// The end of a test (ADR-0400 §7): the connection performs and releases what it can of
    /// `checks`, and hands back every check that did not pass, a check it cannot make among
    /// them as cannot-verify. An empty list is a clean end.
    fn finish(&mut self, checks: &EndChecks) -> Vec<Failure>;
}

/// Why a call has no answer to hand back.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CallError {
    /// The application, or the connection on its behalf, refused the call or failed it: the
    /// error says which code, whether to retry, and how much of an action reached the app.
    Failed(Box<ErrorObject>),
    /// The call's answer did not arrive. The connection has asked the agent to release the
    /// input its lease holds; `released` says whether the agent answered that it did, never
    /// assumed.
    Unanswered {
        /// Whether the call may have reached the application: an acting call is never resent.
        may_have_run: bool,
        /// Whether the agent answered that it released what the lease holds.
        released: bool,
    },
}

impl CallError {
    /// The error `object`, refused or failed.
    #[must_use]
    pub fn failed(object: ErrorObject) -> Self {
        Self::Failed(Box::new(object))
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(error) => error.fmt(f),
            Self::Unanswered {
                may_have_run,
                released,
            } => {
                let ran = if *may_have_run {
                    "it may have run"
                } else {
                    "it did not run"
                };
                let held = if *released {
                    "the agent released its input"
                } else {
                    "the agent did not answer that it released its input"
                };
                write!(f, "the call had no answer: {ran}, and {held}")
            }
        }
    }
}

impl std::error::Error for CallError {}

/// A check whose call failed is cannot-verify when the target lacks what it needed
/// (`not_supported`); an unanswered call is a failure, never cannot-verify.
impl CheckFailure for CallError {
    fn is_cannot_verify(&self) -> bool {
        matches!(self, Self::Failed(error) if error.code == ErrorCode::NotSupported)
    }

    fn missing_capability(&self) -> Option<String> {
        match self {
            Self::Failed(error) if error.code == ErrorCode::NotSupported => Some(
                error
                    .capability
                    .clone()
                    .unwrap_or_else(|| "unnamed".to_owned()),
            ),
            _ => None,
        }
    }
}

/// Where a connection's clock stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct At {
    /// The frame the window last presented, when the connection knows it exactly.
    pub frame: Option<u64>,
    /// How long the connection has been open, on its clock.
    pub time: Duration,
    /// Whether the clock is virtual: moved by the test, never by the wall.
    pub virtual_clock: bool,
}

impl At {
    /// `time` on a clock that is virtual or not, at no frame the connection knows.
    #[must_use]
    pub const fn new(time: Duration, virtual_clock: bool) -> Self {
        Self {
            frame: None,
            time,
            virtual_clock,
        }
    }

    /// At `frame`.
    #[must_use]
    pub const fn frame(mut self, frame: u64) -> Self {
        self.frame = Some(frame);
        self
    }
}

/// What performing an action sequence did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Performed {
    /// The input path the sequence took.
    pub via: InputPath,
    /// How much of it reached the app.
    pub effect: Effect,
    /// The input-journal entries it produced, when the path reads them.
    pub observed: Option<JournalSpan>,
}

impl From<ActOutcome> for Performed {
    fn from(outcome: ActOutcome) -> Self {
        Self {
            via: outcome.via,
            effect: outcome.effect,
            observed: outcome.observed,
        }
    }
}

/// The checks a test's end asks of its connection (ADR-0400 §7). The default asks them all, with
/// the audit of [`AuditRule::Unnamed`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EndChecks {
    /// Settle the window first, within the settle bound.
    pub settle: bool,
    /// Audit the settled frame by these rules; none audits nothing.
    pub audit: Vec<AuditRule>,
    /// Check that every value the test dropped was dropped.
    pub drop_ledger: bool,
    /// Check that nothing is left behind: no timer, ticker or background work.
    pub left_behind: bool,
}

impl Default for EndChecks {
    fn default() -> Self {
        Self {
            settle: true,
            audit: vec![AuditRule::Unnamed],
            drop_ledger: true,
            left_behind: true,
        }
    }
}

impl EndChecks {
    /// No check: the connection only releases what it holds.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            settle: false,
            audit: Vec::new(),
            drop_ledger: false,
            left_behind: false,
        }
    }

    /// With the settle asked for or not.
    #[must_use]
    pub const fn settle(mut self, settle: bool) -> Self {
        self.settle = settle;
        self
    }

    /// Auditing by `rules`.
    #[must_use]
    pub fn audit(mut self, rules: Vec<AuditRule>) -> Self {
        self.audit = rules;
        self
    }

    /// With the drop ledger checked or not.
    #[must_use]
    pub const fn drop_ledger(mut self, check: bool) -> Self {
        self.drop_ledger = check;
        self
    }

    /// With what is left behind checked or not.
    #[must_use]
    pub const fn left_behind(mut self, check: bool) -> Self {
        self.left_behind = check;
        self
    }
}
