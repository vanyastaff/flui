//! The owner half of `audit.a11y` (ADR-0238 §10, ADR-0400 §5): the accessibility rules run over
//! the window's committed semantics tree and its last applied frame, at the owner's turn.
//!
//! The rules are not written here. `Unnamed` and `TargetSize` are `flui-semantics`'
//! ([`SemanticsOwner::audit_unnamed`], [`SemanticsOwner::audit_target_size`]) and
//! `TextContrast` is `flui-rendering`'s ([`PipelineOwner::audit_text_contrast`]); the testing
//! kit calls the same functions. This module picks the rules asked for, gives each the facts
//! the window holds (its placement, its platform's minimum target, its scene), and folds what
//! they answer into one [`AuditReport`].
//!
//! # The report
//!
//! An empty `rules` asks for every rule the protocol names. A rule that ran is in `checked`,
//! with its findings. A rule asked for that did not run is in `not_checked` with why, and keeps
//! the report from `pass` (`cannot_verify` when nothing was found): a rule this agent does not
//! run (`focus_order`, `no_alternative`, `alternative_differs`), a semantics rule before the
//! window's first semantics tree is committed, and `text_contrast` before the window's first
//! frame is applied. A window whose tree is not committed yet is not an error: the audit
//! answers which rules it could not run, as for any other rule it cannot run.
//!
//! `text_contrast` judges every text the frame paints. When one or more of them cannot be
//! judged (its background is not one colour, it sits under an effect the rule does not
//! resolve), the rule is listed in `not_checked` with the texts it could not judge, and the
//! findings of the texts it judged are still reported: a partial check is never a pass.
//!
//! The target-size minimum is the platform's for its primary pointer: the touch minimum on
//! Android, iOS and Fuchsia, the mouse minimum elsewhere, as the window's gesture settings
//! resolve them (`GestureSettings::min_target`).
//!
//! [`SemanticsOwner::audit_unnamed`]: flui_semantics::SemanticsOwner::audit_unnamed
//! [`SemanticsOwner::audit_target_size`]: flui_semantics::SemanticsOwner::audit_target_size
//! [`PipelineOwner::audit_text_contrast`]: flui_rendering::PipelineOwner::audit_text_contrast

use flui_foundation::RenderId;
use flui_interaction::events::PointerKind;
use flui_platform::TargetPlatform;
use flui_protocol::audit::{AuditFinding, AuditParams, AuditReport, AuditRule};
use flui_protocol::method;
use flui_protocol::tree::{ElementId, ElementRef, WindowId};
use flui_rendering::audit::{ContrastOutcome, ContrastReading, TextContrast};
use flui_semantics::{AccessibilityNodeId, MinimumTarget, WireReadError};

use super::agent::{IssuedHandles, placement_of};
use super::agent_serve::{Answer, Serve, ServeContext};
use crate::presentation::PresentationState;

/// How many texts a `text_contrast` entry of `not_checked` names; the rest are counted.
const NAMED_UNCHECKED_TEXTS: usize = 3;

/// How many characters of a text a finding quotes.
const QUOTED_CHARS: usize = 40;

impl Serve for method::AuditA11y {
    fn record(report: &AuditReport, issued: &mut IssuedHandles) {
        for finding in &report.findings {
            issued.record(finding.node.element);
        }
    }

    /// Runs the rules asked for and answers what they found and what they could not check
    /// (module docs).
    fn serve(ctx: &mut ServeContext<'_, AuditReport>, params: AuditParams) -> Answer<AuditReport> {
        Answer::Now(Ok(audit(ctx.state, ctx.window, &params.rules)))
    }
}

/// What one rule came to.
enum RuleOutcome {
    /// It ran over everything it applies to.
    Checked(Vec<AuditFinding>),
    /// It ran over part of what it applies to: these findings, and why the rest was not.
    Partly(Vec<AuditFinding>, String),
    /// It did not run, for this reason.
    NotChecked(String),
}

/// The report of `rules` (every rule when empty) over `state`'s window.
fn audit(state: &PresentationState, window: WindowId, rules: &[AuditRule]) -> AuditReport {
    let mut asked: Vec<AuditRule> = Vec::new();
    for &rule in if rules.is_empty() {
        AuditRule::ALL
    } else {
        rules
    } {
        if !asked.contains(&rule) {
            asked.push(rule);
        }
    }
    let mut checked = Vec::new();
    let mut findings = Vec::new();
    let mut not_checked = Vec::new();
    for rule in asked {
        match run(state, window, rule) {
            RuleOutcome::Checked(found) => {
                checked.push(rule);
                findings.extend(found);
            }
            RuleOutcome::Partly(found, reason) => {
                findings.extend(found);
                not_checked.push((rule, reason));
            }
            RuleOutcome::NotChecked(reason) => not_checked.push((rule, reason)),
        }
    }
    not_checked.into_iter().fold(
        AuditReport::new(checked, findings),
        |report, (rule, reason)| report.not_checked(rule, reason),
    )
}

/// Runs `rule` over `state`'s window.
fn run(state: &PresentationState, window: WindowId, rule: AuditRule) -> RuleOutcome {
    let placement = placement_of(state, window);
    let semantics = |read: &dyn Fn(
        &flui_semantics::SemanticsOwner,
    ) -> Result<Vec<AuditFinding>, WireReadError>| {
        state
            .pipeline()
            .with(|pipeline| match pipeline.semantics_owner() {
                None => RuleOutcome::NotChecked(NO_TREE.to_owned()),
                Some(owner) => match read(owner) {
                    Ok(found) => RuleOutcome::Checked(found),
                    Err(WireReadError::NoTree) => RuleOutcome::NotChecked(NO_TREE.to_owned()),
                    Err(_) => RuleOutcome::NotChecked(
                        "the published semantics tree is malformed".to_owned(),
                    ),
                },
            })
    };
    match rule {
        AuditRule::Unnamed => semantics(&|owner| owner.audit_unnamed(placement)),
        AuditRule::TargetSize => match minimum_target(state) {
            Some(minimum) => semantics(&|owner| owner.audit_target_size(minimum, placement)),
            None => RuleOutcome::NotChecked(
                "the platform's minimum target is not a finite positive size".to_owned(),
            ),
        },
        AuditRule::TextContrast => text_contrast(state, window),
        _ => RuleOutcome::NotChecked("this app's agent does not run this rule".to_owned()),
    }
}

/// Why a semantics rule did not run before the first committed tree.
const NO_TREE: &str = "no semantics tree has been committed yet";

/// The minimum target of `state`'s platform for its primary pointer (module docs).
fn minimum_target(state: &PresentationState) -> Option<MinimumTarget> {
    let kind = match state.platform {
        TargetPlatform::Android | TargetPlatform::iOS | TargetPlatform::Fuchsia => {
            PointerKind::Touch
        }
        _ => PointerKind::Mouse,
    };
    MinimumTarget::try_new(state.gesture_settings.provider().min_target_for(kind))
}

/// `text_contrast` over the frame `state`'s scene last applied.
fn text_contrast(state: &PresentationState, window: WindowId) -> RuleOutcome {
    let Ok(version) = state.scene.current_version() else {
        return RuleOutcome::NotChecked("a frame holds the window's scene".to_owned());
    };
    let Some(texts) = state
        .pipeline()
        .with(|pipeline| pipeline.audit_text_contrast(&version))
    else {
        return RuleOutcome::NotChecked("the window has not painted a frame yet".to_owned());
    };
    let mut findings = Vec::new();
    let mut unchecked: Vec<String> = Vec::new();
    for text in &texts {
        match (&text.outcome, text.element) {
            (ContrastOutcome::Meets(_), _) => {}
            (ContrastOutcome::Below(reading), Some(element)) => findings.push(AuditFinding::new(
                AuditRule::TextContrast,
                ElementRef::new(window, element_id(element)),
                below_detail(text, reading),
            )),
            (ContrastOutcome::Below(_), None) => unchecked.push(format!(
                "\"{}\": the render object that paints it has no element",
                quoted(&text.text)
            )),
            (ContrastOutcome::NotChecked(why), _) => {
                unchecked.push(format!("\"{}\": {why}", quoted(&text.text)));
            }
            (_, _) => unchecked.push(format!(
                "\"{}\": an outcome this agent does not know",
                quoted(&text.text)
            )),
        }
    }
    if unchecked.is_empty() {
        return RuleOutcome::Checked(findings);
    }
    let count = unchecked.len();
    let more = match count.saturating_sub(NAMED_UNCHECKED_TEXTS) {
        0 => String::new(),
        more => format!("; and {more} more"),
    };
    let named = unchecked
        .iter()
        .take(NAMED_UNCHECKED_TEXTS)
        .map(String::as_str)
        .collect::<Vec<&str>>()
        .join("; ");
    RuleOutcome::Partly(
        findings,
        format!(
            "{count} of {} texts could not be judged: {named}{more}",
            texts.len()
        ),
    )
}

/// The wire handle of the element `render` contributes: its accessibility id.
fn element_id(render: RenderId) -> ElementId {
    ElementId::from_u64(AccessibilityNodeId::from(render).as_u64())
        .expect("BUG: an accessibility id derived from a render id is non-zero")
}

/// A finding's detail for a text below its contrast.
fn below_detail(text: &TextContrast, reading: &ContrastReading) -> String {
    let [fr, fg, fb, _] = reading.foreground.to_rgba8();
    let [br, bg, bb, _] = reading.background.to_rgba8();
    format!(
        "the text \"{}\" shows #{fr:02x}{fg:02x}{fb:02x} on #{br:02x}{bg:02x}{bb:02x}, a contrast \
         of {:.2}:1, under the {}:1 its {:.1} px{} size needs: darken or lighten one of them",
        quoted(&text.text),
        reading.ratio,
        reading.required,
        reading.font_size,
        if reading.bold { " bold" } else { "" },
    )
}

/// `text` as a finding quotes it: its first [`QUOTED_CHARS`] characters.
fn quoted(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(QUOTED_CHARS).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}
