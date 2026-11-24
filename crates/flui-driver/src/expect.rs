//! Retrying assertions (ADR-0238 §7): each is one `wait.until` the app evaluates at its frame boundaries,
//! never a client-side poll, with a 5 s default. A failure says what was expected and what was
//! last seen. [`expect_soft`] records the outcome in a report instead of returning it.

use std::time::Duration;

use flui_protocol::locator::{Locator, StateFilter, TextMatch};
use flui_protocol::tree::Node;
use flui_protocol::wait::{Condition, Expectation, WaitEnd, WaitOutcome};
use flui_protocol::wire::Checked;

use crate::compose;
use crate::error::{Error, Result};
use crate::window::ElementLocator;
use flui_protocol::report::{Check, Report};

/// The assertions about what `subject` describes.
pub fn expect(subject: ElementLocator<'_>) -> Expect<'_> {
    Expect {
        subject,
        timeout: None,
    }
}

/// The assertions about what `subject` describes, each recorded in `report` as a check rather
/// than returned: a failed one makes the report fail, and the scenario goes on.
pub fn expect_soft<'a>(report: &'a mut Report, subject: ElementLocator<'a>) -> SoftExpect<'a> {
    SoftExpect {
        expect: expect(subject),
        report,
    }
}

/// See [`expect`].
#[must_use = "an expectation checks nothing until one of its assertions is called"]
pub struct Expect<'a> {
    subject: ElementLocator<'a>,
    timeout: Option<Duration>,
}

impl Expect<'_> {
    /// Hold out at most `timeout`; the session's (5 s by default) when not set.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Its value is exactly `value`.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] with the last value seen.
    pub fn to_have_value(self, value: &str) -> Result<()> {
        self.matches(
            Expectation::Value(TextMatch::exact(value)),
            format!("value {value:?}"),
            |node| format!("value {:?}", node.value.as_deref().unwrap_or("")),
        )
    }

    /// Its name is exactly `name`.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] with the last name seen.
    pub fn to_have_name(self, name: &str) -> Result<()> {
        self.matches(
            Expectation::Name(TextMatch::exact(name)),
            format!("name {name:?}"),
            |node| format!("name {:?}", node.name.as_deref().unwrap_or("")),
        )
    }

    /// It is checked.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] with the last checked state seen.
    pub fn to_be_checked(self) -> Result<()> {
        let mut state = StateFilter::default();
        state.checked = Some(Checked::True);
        self.matches(Expectation::State(state), "checked".into(), |node| {
            format!("checked {:?}", node.checked)
        })
    }

    /// Neither it nor an ancestor is disabled.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] when it stayed disabled.
    pub fn to_be_enabled(self) -> Result<()> {
        let mut state = StateFilter::default();
        state.disabled = Some(false);
        self.matches(Expectation::State(state), "enabled".into(), |node| {
            if node.disabled { "disabled" } else { "enabled" }.to_owned()
        })
    }

    /// It holds keyboard focus.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] when it did not take it.
    pub fn to_be_focused(self) -> Result<()> {
        self.matches(Expectation::Focused, "focused".into(), |node| {
            if node.focused {
                "focused"
            } else {
                "not focused"
            }
            .to_owned()
        })
    }

    /// The locator matches exactly `count` elements.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] with the last count seen.
    pub fn to_have_count(self, count: u32) -> Result<()> {
        let locator = self.subject.locator().clone();
        let outcome = self.wait(Condition::Matches {
            locator,
            expect: Expectation::Count(count),
        })?;
        settle(
            &outcome,
            format!("{count} elements"),
            format!("{} elements", outcome.observation.count),
        )
    }

    /// The locator matches nothing.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] with what still matched.
    pub fn to_be_gone(self) -> Result<()> {
        let locator = self.subject.locator().clone();
        let outcome = self.wait(Condition::Gone(locator))?;
        let actual = describe_matched(&outcome, outline);
        settle(&outcome, "nothing to match".into(), actual)
    }

    fn matches(
        self,
        expectation: Expectation,
        expected: String,
        actual: impl Fn(&Node) -> String,
    ) -> Result<()> {
        let locator = self.subject.locator().clone();
        let outcome = self.wait(Condition::Matches {
            locator,
            expect: expectation,
        })?;
        let actual = describe_matched(&outcome, actual);
        settle(&outcome, expected, actual)
    }

    fn wait(mut self, condition: Condition) -> Result<WaitOutcome> {
        let locator: &Locator = self.subject.locator();
        let window = locator.window;
        let driver = self.subject.driver();
        let timeout = self.timeout.unwrap_or(driver.timeout);
        let window = match window {
            Some(window) => window,
            None => compose::main_window(driver)?,
        };
        compose::wait_until(driver, window, condition, timeout)
    }
}

/// See [`expect_soft`].
#[must_use = "an expectation checks nothing until one of its assertions is called"]
pub struct SoftExpect<'a> {
    expect: Expect<'a>,
    report: &'a mut Report,
}

impl SoftExpect<'_> {
    /// Hold out at most `timeout`.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.expect = self.expect.timeout(timeout);
        self
    }

    /// As [`Expect::to_have_value`], recorded.
    pub fn to_have_value(self, value: &str) {
        let name = format!("{:?} has value {value:?}", self.expect.subject.locator());
        let result = self.expect.to_have_value(value);
        self.report.push(Check::from_result(name, &result));
    }

    /// As [`Expect::to_have_name`], recorded.
    pub fn to_have_name(self, name: &str) {
        let check = format!("{:?} has name {name:?}", self.expect.subject.locator());
        let result = self.expect.to_have_name(name);
        self.report.push(Check::from_result(check, &result));
    }

    /// As [`Expect::to_have_count`], recorded.
    pub fn to_have_count(self, count: u32) {
        let check = format!(
            "{:?} matches {count} elements",
            self.expect.subject.locator()
        );
        let result = self.expect.to_have_count(count);
        self.report.push(Check::from_result(check, &result));
    }

    /// As [`Expect::to_be_gone`], recorded.
    pub fn to_be_gone(self) {
        let check = format!("{:?} is gone", self.expect.subject.locator());
        let result = self.expect.to_be_gone();
        self.report.push(Check::from_result(check, &result));
    }
}

impl std::fmt::Debug for Expect<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Expect")
            .field("subject", &self.subject)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl std::fmt::Debug for SoftExpect<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoftExpect")
            .field("expect", &self.expect)
            .finish_non_exhaustive()
    }
}

fn settle(outcome: &WaitOutcome, expected: String, actual: String) -> Result<()> {
    if outcome.end == WaitEnd::Satisfied {
        Ok(())
    } else {
        Err(Error::Expectation { expected, actual })
    }
}

fn describe_matched(outcome: &WaitOutcome, describe: impl Fn(&Node) -> String) -> String {
    match outcome.observation.matched.first() {
        Some(node) => describe(node),
        None => "no element matched".to_owned(),
    }
}

fn outline(node: &Node) -> String {
    flui_protocol::tree::outline(std::slice::from_ref(node))
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}
