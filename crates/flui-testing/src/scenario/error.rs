//! [`TestError`]: why a [`Tester`](super::Tester) verb failed, with the [`Diagnosis`] an engineer
//! or an agent acts on without a rerun (ADR-0400 §2).

use std::fmt;
use std::panic::Location;
use std::time::Duration;

use flui_protocol::act::ActionabilityCheck;
use flui_protocol::error::ErrorObject;
use flui_protocol::locator::Locator;
use flui_protocol::wait::{Condition, Observation, UnsettledCause, WaitEnd};

use super::report::Registration;

/// Why a verb failed. Each carries the [`Diagnosis`] of the step that failed.
///
/// An error a `try_` verb returned is registered with its tester's report until it is dropped:
/// while one is alive, the tester's end runs no checks, since the error is on its way to the
/// caller (ADR-0400 §7). It is therefore neither `Clone` nor `Send`.
#[derive(Debug)]
#[non_exhaustive]
pub enum TestError {
    /// Nothing matches the find.
    #[non_exhaustive]
    NotFound {
        /// What was looked for.
        find: Box<Locator>,
        /// The step.
        diagnosis: Box<Diagnosis>,
    },
    /// The find requires one element and several match; every candidate is listed, and nothing
    /// was acted on.
    #[non_exhaustive]
    Ambiguous {
        /// What was looked for.
        find: Box<Locator>,
        /// How many elements match.
        matched: u32,
        /// The step; its candidates are the matches' outline lines.
        diagnosis: Box<Diagnosis>,
    },
    /// The element never became actionable within the act's timeout, and nothing was sent.
    #[non_exhaustive]
    NotActionable {
        /// What was acted on.
        find: Box<Locator>,
        /// The check that did not hold.
        check: Option<ActionabilityCheck>,
        /// What the hit-test reached instead.
        covered_by: Option<String>,
        /// The step.
        diagnosis: Box<Diagnosis>,
    },
    /// An expectation's condition did not hold within its limit, or the application went idle
    /// with it false.
    #[non_exhaustive]
    Unsatisfied {
        /// The condition the expectation lowered to.
        condition: Box<Condition>,
        /// How the wait ended: [`WaitEnd::Unsatisfied`] at the limit, [`WaitEnd::Idle`] when
        /// only the test could change the state.
        end: WaitEnd,
        /// What the application showed last.
        observation: Box<Observation>,
        /// The step; its causes are what kept the application busy.
        diagnosis: Box<Diagnosis>,
    },
    /// The application did not settle within the limit; every cause is named.
    #[non_exhaustive]
    NotSettled {
        /// How long the settle held out, on the connection's clock.
        limit: Duration,
        /// The step; its causes are what kept the application busy.
        diagnosis: Box<Diagnosis>,
    },
    /// The application, or the connection on its behalf, refused or failed the call.
    #[non_exhaustive]
    Agent {
        /// The protocol's error object.
        error: Box<ErrorObject>,
        /// The step.
        diagnosis: Box<Diagnosis>,
    },
    /// The call's answer did not arrive. The tester is poisoned: its end checks nothing.
    #[non_exhaustive]
    Unanswered {
        /// Whether the call may have reached the application.
        may_have_run: bool,
        /// Whether the agent answered that it released the input the lease holds.
        released: bool,
        /// The step.
        diagnosis: Box<Diagnosis>,
    },
}

impl TestError {
    /// The diagnosis of the step that failed.
    #[must_use]
    pub fn diagnosis(&self) -> &Diagnosis {
        match self {
            Self::NotFound { diagnosis, .. }
            | Self::Ambiguous { diagnosis, .. }
            | Self::NotActionable { diagnosis, .. }
            | Self::Unsatisfied { diagnosis, .. }
            | Self::NotSettled { diagnosis, .. }
            | Self::Agent { diagnosis, .. }
            | Self::Unanswered { diagnosis, .. } => diagnosis,
        }
    }

    /// Panic at the caller with this error, kept as the report's first failure.
    #[track_caller]
    pub(crate) fn raise(self) -> ! {
        let message = self.to_string();
        if let Some(registration) = &self.diagnosis().registration {
            registration.raised(&message);
        }
        drop(self);
        panic!("{message}");
    }

    fn headline(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { find, .. } => write!(f, "nothing matches {find:?}"),
            Self::Ambiguous { find, matched, .. } => write!(
                f,
                "{matched} elements match {find:?}, which names one; narrow it, or pick one with \
                 `.all().first()`, `.nth(..)` or `.last()`"
            ),
            Self::NotActionable {
                find,
                check,
                covered_by,
                ..
            } => {
                write!(f, "{find:?} did not become actionable")?;
                if let Some(check) = check {
                    write!(f, ": not {}", check.name())?;
                }
                if let Some(by) = covered_by {
                    write!(f, ", the hit-test reaches {by}")?;
                }
                Ok(())
            }
            Self::Unsatisfied {
                condition,
                end,
                observation,
                ..
            } => {
                let ended = match end {
                    WaitEnd::Idle => "the application went idle with it false",
                    _ => "it did not hold within the limit",
                };
                write!(
                    f,
                    "expected {condition:?}: {ended}; {} element(s) matched",
                    observation.count
                )
            }
            Self::NotSettled { limit, .. } => {
                write!(f, "the application did not settle within {limit:?}")
            }
            Self::Agent { error, .. } => write!(f, "the window answered {error}"),
            Self::Unanswered {
                may_have_run,
                released,
                ..
            } => write!(
                f,
                "the call had no answer ({}; {})",
                if *may_have_run {
                    "it may have run"
                } else {
                    "it did not run"
                },
                if *released {
                    "the agent released the input it held"
                } else {
                    "the agent did not answer that it released its input"
                }
            ),
        }
    }
}

/// A `try_` verb's result, raised at the plain verb's caller.
pub(crate) trait OrRaise<T> {
    /// The value, or a panic at the caller with the error.
    fn or_raise(self) -> T;
}

impl<T> OrRaise<T> for Result<T, TestError> {
    #[track_caller]
    fn or_raise(self) -> T {
        match self {
            Ok(value) => value,
            Err(error) => error.raise(),
        }
    }
}

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.headline(f)?;
        write!(f, "\n{}", self.diagnosis())
    }
}

impl std::error::Error for TestError {}

/// What a failed step was and what the window showed (ADR-0400 §2): the journal step and its
/// caller, the frame and the time, the nearest candidates, an outline excerpt, and for a wait the
/// causes that kept the application busy.
pub struct Diagnosis {
    /// The step's number in the tester's journal, from 1.
    pub step: u64,
    /// The verb.
    pub verb: &'static str,
    /// Where the test called it.
    pub caller: &'static Location<'static>,
    /// The frame the window last presented, when the connection knows it.
    pub frame: Option<u64>,
    /// The connection's clock.
    pub time: Duration,
    /// The elements nearest what was looked for, as outline lines.
    pub candidates: Vec<String>,
    /// The window's outline, cut to its first lines.
    pub outline: String,
    /// What kept the application from being idle, for a wait.
    pub unsettled: Vec<UnsettledCause>,
    /// The steps before this one, the latest last, each with its caller.
    pub journal: Vec<String>,
    pub(crate) registration: Option<Registration>,
}

impl fmt::Debug for Diagnosis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Diagnosis")
            .field("step", &self.step)
            .field("verb", &self.verb)
            .field("caller", &self.caller)
            .field("frame", &self.frame)
            .field("time", &self.time)
            .field("candidates", &self.candidates)
            .field("outline", &self.outline)
            .field("unsettled", &self.unsettled)
            .field("journal", &self.journal)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Diagnosis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "step {} `{}` at {}, frame {}, {:?} on the clock",
            self.step,
            self.verb,
            self.caller,
            self.frame
                .map_or_else(|| "unknown".to_owned(), |frame| frame.to_string()),
            self.time,
        )?;
        if !self.candidates.is_empty() {
            write!(f, "\nCandidates:")?;
            for candidate in &self.candidates {
                write!(f, "\n  {candidate}")?;
            }
        }
        if !self.unsettled.is_empty() {
            write!(f, "\nThe application was kept busy by:")?;
            for cause in &self.unsettled {
                write!(
                    f,
                    "\n  {:?}, cleared by {:?}",
                    cause.reason, cause.cleared_by
                )?;
                if let Some(window) = cause.window {
                    write!(f, " in {window}")?;
                }
                if let Some(due) = cause.due_in_ms {
                    write!(f, ", due in {due} ms")?;
                }
            }
        }
        if !self.journal.is_empty() {
            write!(f, "\nThe steps before it:")?;
            for step in &self.journal {
                write!(f, "\n  {step}")?;
            }
        }
        write!(f, "\nThe window shows:\n{}", self.outline)
    }
}
