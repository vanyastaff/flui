//! A test's report (ADR-0400 §7): the one owner of its first failure, its soft failures, whether
//! a call may have run, and the [`TestError`](super::TestError)s still alive. Every exit of a
//! [`Tester`](super::Tester) reads it, so a failure is raised once and never lost.
//!
//! The report is the tester's and lives on its thread: an `Rc` the tester, its live errors and
//! the guards of its running verbs share.

use std::cell::RefCell;
use std::fmt;
use std::panic::Location;
use std::rc::Rc;

use flui_protocol::report::{Outcome, UnpassedCheck};

/// How a test ends: what decides whether an error a `try_` verb returned, still alive, skips the
/// end checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exit {
    /// The test called `finish`: it says it is done, so an error it kept is one it expected.
    Finish,
    /// The tester dropped: an error still alive may be on its way to the caller through `?`.
    Drop,
}

/// The shared report of one tester.
#[derive(Clone, Default)]
pub(crate) struct Report(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    /// The first failure a verb raised, as its message.
    first_failure: Option<String>,
    /// Why the tester is poisoned: a verb unwound, or a call had no answer.
    poisoned: Option<String>,
    /// Soft failures not yet raised.
    soft: Vec<String>,
    /// Whether a call that had no answer may have reached the application.
    may_have_run: bool,
    /// How many errors a `try_` verb returned are alive.
    live_errors: usize,
}

impl Report {
    /// A registration for an error a `try_` verb returns: it counts as alive until dropped.
    pub(crate) fn register(&self) -> Registration {
        self.0.borrow_mut().live_errors += 1;
        Registration(Rc::clone(&self.0))
    }

    /// A guard over a running verb: a verb that unwinds poisons the report as the guard drops,
    /// whether or not the panic is caught later.
    pub(crate) fn guard(&self, verb: &'static str, caller: &'static Location<'static>) -> Running {
        Running {
            report: self.clone(),
            verb,
            caller,
        }
    }

    /// A call had no answer: the tester is poisoned, and `may_have_run` says whether it may have
    /// reached the application.
    pub(crate) fn unanswered(&self, verb: &'static str, may_have_run: bool) {
        let mut state = self.0.borrow_mut();
        state.may_have_run |= may_have_run;
        state
            .poisoned
            .get_or_insert_with(|| format!("{verb} had no answer"));
    }

    /// A verb raises `message`: the first raised is kept, and the tester is poisoned, since the
    /// raise unwinds.
    pub(crate) fn raised(&self, message: &str) {
        raise_into(&mut self.0.borrow_mut(), message);
    }

    /// Add a soft failure.
    pub(crate) fn soft(&self, failure: String) {
        self.0.borrow_mut().soft.push(failure);
    }

    /// The soft failures not yet raised, taken.
    pub(crate) fn take_soft(&self) -> Vec<String> {
        std::mem::take(&mut self.0.borrow_mut().soft)
    }

    /// Why the end checks do not run now, or `None` when they do: the report is poisoned, the
    /// thread is unwinding, or, as the tester drops, an error a `try_` verb returned is alive (it
    /// may be travelling to the caller through `?`). An explicit finish runs them beside a kept
    /// error: the test says it is done.
    pub(crate) fn skips_end_checks(&self, exit: Exit) -> Option<String> {
        let state = self.0.borrow();
        if let Some(poisoned) = &state.poisoned {
            let mut reason = format!("the tester is poisoned: {poisoned}");
            if let Some(first) = &state.first_failure
                && !poisoned.starts_with(RAISED)
            {
                let first = first.lines().next().unwrap_or_default();
                reason.push_str("; the first failure raised: ");
                reason.push_str(first);
            }
            if state.may_have_run {
                reason.push_str("; a call that had no answer may have run in the application");
            }
            return Some(reason);
        }
        if exit == Exit::Drop && state.live_errors > 0 {
            return Some(format!(
                "{} error(s) a `try_` verb returned are still alive, on their way to the caller",
                state.live_errors
            ));
        }
        if std::thread::panicking() {
            return Some("the thread is unwinding".to_owned());
        }
        None
    }
}

/// How a poison set by a raise begins: the first failure is then already its reason.
const RAISED: &str = "a verb raised: ";

/// An error's registration with its tester's report: alive until the error is dropped.
pub(crate) struct Registration(Rc<RefCell<State>>);

impl Registration {
    /// The error is raised as `message`: it is the report's first failure unless one was raised
    /// before.
    pub(crate) fn raised(&self, message: &str) {
        raise_into(&mut self.0.borrow_mut(), message);
    }
}

/// `message` is raised: the first failure unless one was raised before, and the poison.
fn raise_into(state: &mut State, message: &str) {
    state
        .first_failure
        .get_or_insert_with(|| message.to_owned());
    state
        .poisoned
        .get_or_insert_with(|| format!("{RAISED}{}", message.lines().next().unwrap_or_default()));
}

impl Drop for Registration {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        state.live_errors = state.live_errors.saturating_sub(1);
    }
}

/// A running verb's guard: see [`Report::guard`].
pub(crate) struct Running {
    report: Report,
    verb: &'static str,
    caller: &'static Location<'static>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // `try_borrow_mut`: a panic raised while the report was borrowed must not become a
            // second panic here, which would abort.
            if let Ok(mut state) = self.report.0.try_borrow_mut() {
                state
                    .poisoned
                    .get_or_insert_with(|| format!("{} at {} unwound", self.verb, self.caller));
            }
        }
    }
}

/// What a test's end found (ADR-0400 §7): every check that did not pass, and the soft failures
/// never raised. Empty is a clean end.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct TestReport {
    /// The end checks that did not pass, a check the connection cannot make among them as
    /// cannot-verify.
    pub unpassed: Vec<UnpassedCheck>,
    /// Soft failures never raised.
    pub soft: Vec<String>,
    /// Why the end checks did not run, when they did not.
    pub skipped: Option<String>,
}

impl TestReport {
    /// Whether nothing failed, nothing went unchecked and the checks ran.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.unpassed.is_empty() && self.soft.is_empty() && self.skipped.is_none()
    }
}

impl fmt::Display for TestReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_clean() {
            return f.write_str("the test's end checks passed");
        }
        f.write_str("the test's end did not pass:")?;
        if let Some(skipped) = &self.skipped {
            write!(f, "\n- the end checks did not run: {skipped}")?;
        }
        for unpassed in &self.unpassed {
            let check = unpassed.check();
            let outcome = match &check.outcome {
                Outcome::Failed { reason } => format!("failed: {reason}"),
                Outcome::CannotVerify { reason, .. } => format!("cannot be verified: {reason}"),
                other => format!("{other:?}"),
            };
            write!(f, "\n- {} {outcome}", check.name)?;
            for evidence in &check.evidence {
                write!(f, "\n    {evidence}")?;
            }
        }
        for soft in &self.soft {
            write!(f, "\n- a soft failure: {soft}")?;
        }
        Ok(())
    }
}

impl std::error::Error for TestReport {}

/// Write what the end of a test found to the test's output, and as `tracing` events a capture
/// reads.
pub(crate) fn write_report(report: &TestReport) {
    if let Some(skipped) = &report.skipped {
        tracing::info!(reason = %skipped, "the end checks did not run");
    }
    for unpassed in &report.unpassed {
        let check = unpassed.check();
        tracing::warn!(
            check = %check.name,
            outcome = ?check.outcome,
            evidence = ?check.evidence,
            "an end check did not pass"
        );
    }
    for soft in &report.soft {
        tracing::error!(failure = %soft, "a soft failure was never raised");
    }
    if !report.unpassed.is_empty() || !report.soft.is_empty() {
        eprintln!("{report}");
    }
}
