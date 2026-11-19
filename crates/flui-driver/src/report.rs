//! A scenario's one report: every check with its outcome, the isolation its OS input ran in,
//! the OS-input announcements made, and one verdict with its exit code.
//!
//! The verdict rule: `fail` when a check failed; else `cannot_verify` when a check that was asked
//! for could not run, or when nothing was checked at all; else `pass`. A check whose failure
//! [is cannot-verify](Error::is_cannot_verify) (a capability the target lacks, a lane the host
//! cannot run, the user taking the desktop back) is `cannot_verify`, never a failure and never a
//! pass, and the capability it lacked joins the report's list. A pass therefore always says what
//! it covered: the checks it lists.
//!
//! A report prints the same way everywhere: a human block, then one final line
//! `FLUI_DRIVER_VERDICT=<pass|fail|cannot_verify>`; [`Report::to_json`] is the `--json` form.

use std::fmt;
use std::path::PathBuf;

pub use flui_protocol::session::Isolation;
pub use flui_protocol::verdict::Verdict;

use crate::driver::Driver;
use crate::error::Result;

/// The line prefix a report's last line starts with.
pub const VERDICT_LINE: &str = "FLUI_DRIVER_VERDICT=";

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

    /// The check `name` whose attempt answered `result`: passed on success, cannot-verify on a
    /// [cannot-verify](Error::is_cannot_verify) failure, failed on any other.
    #[must_use]
    pub fn from_result<T>(name: impl Into<String>, result: &Result<T>) -> Self {
        let outcome = match result {
            Ok(_) => Outcome::Passed,
            Err(error) if error.is_cannot_verify() => Outcome::CannotVerify {
                reason: error.to_string(),
                capability: error.missing_capability(),
            },
            Err(error) => Outcome::Failed {
                reason: error.to_string(),
            },
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

/// A scenario's report: see the [module docs](self).
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
    pub fn check<T>(
        &mut self,
        name: impl Into<String>,
        attempt: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
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

    /// Run `scenario` against `driver` as the check `name`, the scenario adding its own checks.
    /// What it fails with becomes the check's outcome; the driver's OS-input announcements and
    /// isolation join the report.
    pub fn run_scenario(
        &mut self,
        name: impl Into<String>,
        driver: &mut Driver,
        scenario: impl FnOnce(&mut Driver, &mut Self) -> Result<()>,
    ) {
        let result = scenario(driver, self);
        self.push(Check::from_result(name, &result));
        self.isolation = Some(driver.isolation());
        self.lane = driver.lane_name().map(str::to_owned);
        self.announcements.extend(driver.take_announcements());
    }

    /// Name the isolation the run's OS input ran in.
    pub fn set_isolation(&mut self, isolation: Isolation) {
        self.isolation = Some(isolation);
    }

    /// Name the OS lane the run's OS input went through.
    pub fn set_lane(&mut self, lane: impl Into<String>) {
        self.lane = Some(lane.into());
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

    /// The report as JSON, the `--json` form of [`Display`](fmt::Display).
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let checks: Vec<serde_json::Value> = self
            .checks
            .iter()
            .map(|check| {
                let mut row = serde_json::json!({
                    "name": check.name,
                    "verdict": check.outcome.verdict().name(),
                });
                match &check.outcome {
                    Outcome::Passed => {}
                    Outcome::Failed { reason } => row["reason"] = reason.as_str().into(),
                    Outcome::CannotVerify { reason, capability } => {
                        row["reason"] = reason.as_str().into();
                        if let Some(capability) = capability {
                            row["capability"] = capability.as_str().into();
                        }
                    }
                }
                if !check.evidence.is_empty() {
                    row["evidence"] = check.evidence.clone().into();
                }
                row
            })
            .collect();
        serde_json::json!({
            "verdict": self.verdict().name(),
            "exit_code": self.exit_code(),
            "checks": checks,
            "cannot_verify": self.cannot_verify(),
            "isolation": self.isolation.map(Isolation::name),
            "lane": self.lane,
            "announcements": self.announcements,
            "trace": self.trace.as_ref().map(|path| path.display().to_string()),
        })
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
