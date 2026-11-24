//! A run's one report: every check with its outcome, the isolation its OS input ran in, the
//! OS-input announcements made, and one verdict with its exit code. It lives beside the vocabulary
//! so every client of a connection can write it (ADR-0400 §7).
//!
//! The verdict rule: `fail` when a check failed; else `cannot_verify` when a check that was asked
//! for could not run, or when nothing was checked at all; else `pass`. A check whose failure
//! [is cannot-verify](CheckFailure::is_cannot_verify) (a capability the target lacks, a lane the
//! host cannot run, the user taking the desktop back) is `cannot_verify`, never a failure and
//! never a pass, and the capability it lacked joins the report's list. A pass therefore always
//! says what it covered: the checks it lists.
//!
//! A report prints the same way everywhere: a human block, then one final line
//! `FLUI_DRIVER_VERDICT=<pass|fail|cannot_verify>`; with the `serde` feature it serializes to the
//! `--json` form.

use std::fmt;
use std::path::PathBuf;

use crate::session::Isolation;
use crate::verdict::Verdict;

/// The line prefix a report's last line starts with.
pub const VERDICT_LINE: &str = "FLUI_DRIVER_VERDICT=";

/// What an error a check's attempt failed with says about that check.
pub trait CheckFailure: fmt::Display {
    /// Whether the check could not be made, rather than made and failed: a capability the
    /// target lacks, a lane the host cannot run, the user taking the desktop back.
    fn is_cannot_verify(&self) -> bool;

    /// The capability whose absence made the check cannot-verify, when one was named.
    fn missing_capability(&self) -> Option<String>;
}

/// How one check came out.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// It held.
    Passed,
    /// It did not hold.
    Failed {
        /// What was wrong.
        reason: String,
    },
    /// It was asked for and could not be made.
    CannotVerify {
        /// Why.
        reason: String,
        /// The capability that was missing, when one was.
        capability: Option<String>,
    },
}

impl Outcome {
    /// The verdict a report of this check alone would have.
    #[must_use]
    pub const fn verdict(&self) -> Verdict {
        match self {
            Self::Passed => Verdict::Pass,
            Self::Failed { .. } => Verdict::Fail,
            Self::CannotVerify { .. } => Verdict::CannotVerify,
        }
    }

    /// The outcome of an attempt that failed with `error`: cannot-verify when the error says
    /// so, failed otherwise.
    #[must_use]
    pub fn of_failure(error: &impl CheckFailure) -> Self {
        if error.is_cannot_verify() {
            Self::CannotVerify {
                reason: error.to_string(),
                capability: error.missing_capability(),
            }
        } else {
            Self::Failed {
                reason: error.to_string(),
            }
        }
    }
}

/// One check of a report.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Check {
    /// What was checked.
    pub name: String,
    /// How it came out.
    pub outcome: Outcome,
    /// What it rests on: readings, captures, trace rows.
    pub evidence: Vec<String>,
}

impl Check {
    /// `name`, which came out as `outcome`, with no evidence yet.
    #[must_use]
    pub fn new(name: impl Into<String>, outcome: Outcome) -> Self {
        Self {
            name: name.into(),
            outcome,
            evidence: Vec::new(),
        }
    }

    /// The check `name` whose attempt answered `result`: passed on success, as
    /// [`Outcome::of_failure`] on an error.
    #[must_use]
    pub fn from_result<T, E: CheckFailure>(name: impl Into<String>, result: &Result<T, E>) -> Self {
        let outcome = match result {
            Ok(_) => Outcome::Passed,
            Err(error) => Outcome::of_failure(error),
        };
        Self::new(name, outcome)
    }

    /// With `evidence` added.
    #[must_use]
    pub fn evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence.push(evidence.into());
        self
    }
}

/// A check that did not pass: what a connection's end of a test hands back
/// ([`Connection::finish`](crate::client::Connection::finish)). A passed check is not one, so a
/// list of them is empty exactly when nothing failed and nothing went unchecked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpassedCheck(Check);

impl UnpassedCheck {
    /// The check `name` that did not hold, and what was wrong.
    #[must_use]
    pub fn failed(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self(Check::new(
            name,
            Outcome::Failed {
                reason: reason.into(),
            },
        ))
    }

    /// The check `name` that was asked for and could not be made, why, and the capability it
    /// lacked when one was.
    #[must_use]
    pub fn cannot_verify(
        name: impl Into<String>,
        reason: impl Into<String>,
        capability: Option<String>,
    ) -> Self {
        Self(Check::new(
            name,
            Outcome::CannotVerify {
                reason: reason.into(),
                capability,
            },
        ))
    }

    /// `check`, unless it passed.
    #[must_use]
    pub fn try_from_check(check: Check) -> Option<Self> {
        (check.outcome != Outcome::Passed).then_some(Self(check))
    }

    /// The check, which did not pass.
    #[must_use]
    pub const fn check(&self) -> &Check {
        &self.0
    }

    /// The check, to add to a report.
    #[must_use]
    pub fn into_check(self) -> Check {
        self.0
    }
}

/// A run's report: see the [module docs](self).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    checks: Vec<Check>,
    isolation: Option<Isolation>,
    lane: Option<String>,
    announcements: Vec<String>,
    trace: Option<PathBuf>,
}

impl Report {
    /// An empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `check`.
    pub fn push(&mut self, check: Check) {
        self.checks.push(check);
    }

    /// Run `attempt` as the check `name` and record how it came out; its result is handed back.
    ///
    /// # Errors
    ///
    /// What `attempt` failed with, after it is recorded.
    pub fn check<T, E: CheckFailure>(
        &mut self,
        name: impl Into<String>,
        attempt: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let result = attempt();
        self.push(Check::from_result(name, &result));
        result
    }

    /// Record the check `name`, asked for but not run, and why: the verdict is at best
    /// `cannot_verify`.
    pub fn not_run(&mut self, name: impl Into<String>, reason: impl Into<String>) {
        self.push(Check::new(
            name,
            Outcome::CannotVerify {
                reason: reason.into(),
                capability: None,
            },
        ));
    }

    /// Run `scenario` as the check `name`, the scenario adding its own checks. What it fails
    /// with becomes the check's outcome; a scenario that returns having made no check is
    /// `cannot_verify`, since it covered nothing.
    pub fn run<E: CheckFailure>(
        &mut self,
        name: impl Into<String>,
        scenario: impl FnOnce(&mut Self) -> Result<(), E>,
    ) {
        let before = self.checks.len();
        let result = scenario(self);
        let checked_nothing = result.is_ok() && self.checks.len() == before;
        if checked_nothing {
            self.not_run(name, "the scenario made no check");
        } else {
            self.push(Check::from_result(name, &result));
        }
    }

    /// Name the isolation the run's OS input ran in.
    pub fn set_isolation(&mut self, isolation: Isolation) {
        self.isolation = Some(isolation);
    }

    /// Name the OS lane the run's OS input went through, or none.
    pub fn set_lane(&mut self, lane: Option<String>) {
        self.lane = lane;
    }

    /// The OS lane the run's OS input went through, when it had one.
    #[must_use]
    pub fn lane(&self) -> Option<&str> {
        self.lane.as_deref()
    }

    /// Record an OS-input announcement made during the run.
    pub fn announce(&mut self, line: impl Into<String>) {
        self.announcements.push(line.into());
    }

    /// Name the trace the run left.
    pub fn set_trace(&mut self, trace: impl Into<PathBuf>) {
        self.trace = Some(trace.into());
    }

    /// The checks, in the order they were made.
    #[must_use]
    pub fn checks(&self) -> &[Check] {
        &self.checks
    }

    /// The checks that ran.
    pub fn checked(&self) -> impl Iterator<Item = &Check> {
        self.checks
            .iter()
            .filter(|check| !matches!(check.outcome, Outcome::CannotVerify { .. }))
    }

    /// The checks asked for that could not be made, each with its reason.
    pub fn not_checked(&self) -> impl Iterator<Item = &Check> {
        self.checks
            .iter()
            .filter(|check| matches!(check.outcome, Outcome::CannotVerify { .. }))
    }

    /// The capabilities the cannot-verify checks lacked, each once, in the order first named.
    #[must_use]
    pub fn cannot_verify(&self) -> Vec<String> {
        let mut capabilities: Vec<String> = Vec::new();
        for check in &self.checks {
            if let Outcome::CannotVerify {
                capability: Some(capability),
                ..
            } = &check.outcome
                && !capabilities.contains(capability)
            {
                capabilities.push(capability.clone());
            }
        }
        capabilities
    }

    /// The isolation the run's OS input ran in, when it was named.
    #[must_use]
    pub fn isolation(&self) -> Option<Isolation> {
        self.isolation
    }

    /// The OS-input announcements made.
    #[must_use]
    pub fn announcements(&self) -> &[String] {
        &self.announcements
    }

    /// The verdict: see the [module docs](self).
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        let verdicts = self.checks.iter().map(|check| check.outcome.verdict());
        if verdicts.clone().any(|verdict| verdict == Verdict::Fail) {
            Verdict::Fail
        } else if self.checks.is_empty()
            || verdicts
                .clone()
                .any(|verdict| verdict == Verdict::CannotVerify)
        {
            Verdict::CannotVerify
        } else {
            Verdict::Pass
        }
    }

    /// The process exit code the verdict maps to, the same everywhere: `0` pass, `1` fail, `2`
    /// cannot verify.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        verdict_exit_code(self.verdict())
    }
}

/// The exit code of `verdict`: `0` pass, `1` fail, `2` cannot verify.
#[must_use]
pub const fn verdict_exit_code(verdict: Verdict) -> i32 {
    match verdict {
        Verdict::Pass => 0,
        Verdict::Fail => 1,
        // `cannot_verify`, and a verdict a newer protocol adds, which is not a pass.
        _ => 2,
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for check in &self.checks {
            match &check.outcome {
                Outcome::Passed => writeln!(f, "pass  {}", check.name)?,
                Outcome::Failed { reason } => writeln!(f, "FAIL  {}: {reason}", check.name)?,
                Outcome::CannotVerify { reason, .. } => {
                    writeln!(f, "SKIP  {} (cannot verify): {reason}", check.name)?;
                }
            }
            for evidence in &check.evidence {
                writeln!(f, "      {evidence}")?;
            }
        }
        if self.checks.is_empty() {
            writeln!(f, "nothing was checked")?;
        }
        if let Some(isolation) = self.isolation {
            writeln!(f, "isolation: {}", isolation.name())?;
        }
        if let Some(lane) = &self.lane {
            writeln!(f, "lane: {lane}")?;
        }
        for line in &self.announcements {
            writeln!(f, "announced: {line}")?;
        }
        let missing = self.cannot_verify();
        if !missing.is_empty() {
            writeln!(f, "cannot verify without: {}", missing.join(", "))?;
        }
        if let Some(trace) = &self.trace {
            writeln!(f, "trace: {}", trace.display())?;
        }
        write!(f, "{VERDICT_LINE}{}", self.verdict().name())
    }
}

/// The `--json` form: `verdict`, `exit_code`, `checks` (each with its `name` and `verdict`, and
/// its `reason`, `capability` and `evidence` when it has them), `cannot_verify`, `isolation`,
/// `lane`, `announcements` and `trace`.
#[cfg(feature = "serde")]
impl serde::Serialize for Report {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let mut report = serializer.serialize_struct("Report", 8)?;
        report.serialize_field("verdict", self.verdict().name())?;
        report.serialize_field("exit_code", &self.exit_code())?;
        report.serialize_field("checks", &self.checks)?;
        report.serialize_field("cannot_verify", &self.cannot_verify())?;
        report.serialize_field("isolation", &self.isolation.map(Isolation::name))?;
        report.serialize_field("lane", &self.lane)?;
        report.serialize_field("announcements", &self.announcements)?;
        report.serialize_field(
            "trace",
            &self.trace.as_ref().map(|path| path.display().to_string()),
        )?;
        report.end()
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Check {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;
        let mut row = serializer.serialize_map(None)?;
        row.serialize_entry("name", &self.name)?;
        row.serialize_entry("verdict", self.outcome.verdict().name())?;
        match &self.outcome {
            Outcome::Passed => {}
            Outcome::Failed { reason } => row.serialize_entry("reason", reason)?,
            Outcome::CannotVerify { reason, capability } => {
                row.serialize_entry("reason", reason)?;
                if let Some(capability) = capability {
                    row.serialize_entry("capability", capability)?;
                }
            }
        }
        if !self.evidence.is_empty() {
            row.serialize_entry("evidence", &self.evidence)?;
        }
        row.end()
    }
}
