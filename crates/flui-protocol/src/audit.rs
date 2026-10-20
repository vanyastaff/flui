//! Audits of a screen (ADR-0238 §10): rules run over the published tree, and a report that
//! says what was checked and what was not.
//!
//! A rule asked for that did not run keeps the report from `pass`: it is listed in
//! `not_checked` with why, and the verdict is `cannot_verify`.

use crate::tree::{ElementRef, WindowId};
use crate::verdict::Verdict;

vocabulary! {
    /// A rule an audit applies.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum AuditRule {
        /// An interactive element has no accessible name.
        Unnamed => "unnamed",
        /// Keyboard focus order disagrees with reading order.
        FocusOrder => "focus_order",
        /// An interactive element is smaller than the platform's minimum target.
        TargetSize => "target_size",
        /// A gesture has no semantics action that does the same.
        NoAlternative => "no_alternative",
        /// A gesture's semantics alternative has a different effect.
        AlternativeDiffers => "alternative_differs",
    }
}

/// `audit.a11y` and `audit.screen`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AuditParams {
    /// The window.
    pub window: WindowId,
    /// The rules to apply; every rule the method knows when empty.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub rules: Vec<AuditRule>,
}

impl AuditParams {
    /// Every rule over `window`.
    #[must_use]
    pub fn new(window: WindowId) -> Self {
        Self {
            window,
            rules: Vec::new(),
        }
    }
}

/// One element breaking one rule.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AuditFinding {
    /// The rule.
    pub rule: AuditRule,
    /// The element.
    pub node: ElementRef,
    /// What is wrong, for people.
    pub detail: String,
}

impl AuditFinding {
    /// `node` breaks `rule`.
    #[must_use]
    pub fn new(rule: AuditRule, node: ElementRef, detail: impl Into<String>) -> Self {
        Self {
            rule,
            node,
            detail: detail.into(),
        }
    }
}

/// A rule asked for that did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct NotChecked {
    /// The rule.
    pub rule: AuditRule,
    /// Why it did not run.
    pub reason: String,
}

/// What an audit answers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AuditReport {
    /// `pass` only when every rule asked for ran and found nothing.
    pub verdict: Verdict,
    /// What was found.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub findings: Vec<AuditFinding>,
    /// The rules that ran.
    pub checked: Vec<AuditRule>,
    /// The rules asked for that did not run.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub not_checked: Vec<NotChecked>,
}

impl AuditReport {
    /// The report of `checked` rules finding `findings`, with every rule asked for run; the
    /// verdict follows from them.
    #[must_use]
    pub fn new(checked: Vec<AuditRule>, findings: Vec<AuditFinding>) -> Self {
        let verdict = if findings.is_empty() {
            Verdict::Pass
        } else {
            Verdict::Fail
        };
        Self {
            verdict,
            findings,
            checked,
            not_checked: Vec::new(),
        }
    }

    /// The report, with `rule` not run for `reason`: a report with nothing found becomes
    /// `cannot_verify`.
    #[must_use]
    pub fn with_not_checked(mut self, rule: AuditRule, reason: impl Into<String>) -> Self {
        self.not_checked.push(NotChecked {
            rule,
            reason: reason.into(),
        });
        if self.verdict == Verdict::Pass {
            self.verdict = Verdict::CannotVerify;
        }
        self
    }
}
