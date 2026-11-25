//! The owner half of `audit.a11y` (ADR-0238 §10, ADR-0400 §5): the accessibility rules run over
//! the window's committed semantics tree and its last applied frame, at the owner's turn.
//!
//! The rules are not written here. `Unnamed` and `TargetSize` are `flui-semantics`'
//! ([`SemanticsOwner::audit_unnamed`], [`SemanticsOwner::audit_target_size`]) and
//! `TextContrast` is `flui-rendering`'s ([`PipelineOwner::audit_text_contrast`]). This module
//! picks the rules asked for (each once, in the order first asked), gives each the facts the
//! window holds (its placement, its platform's minimum target, its scene), and folds what they
//! answer into one [`AuditReport`].
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
//! resolve, something is painted over it, the text below its contrast has no element to name),
//! the rule is listed in `not_checked` with how many, the first three quoted with why and the
//! rest counted, and the findings of the texts it judged are still reported: a partial check is
//! never a pass. The text of a disabled control, or of anything under one, is not judged: WCAG
//! 1.4.3 exempts an inactive component, so each such text is listed in `not_checked` with that
//! reason instead of a finding nobody can fix.
//!
//! The target-size minimum is the window's primary pointer's, the pointer a composed tap plays
//! with (`agent_gesture::primary_pointer`): a touch contact on Android, iOS and Fuchsia, a mouse
//! elsewhere, at the minimum the window's gesture settings resolve for it
//! (`GestureSettings::min_target`). Each finding names the pointer it was judged for. The testing
//! kit's gesture-alternatives check holds a control to a finger's minimum instead, since it
//! presses with one on every platform.
//!
//! [`SemanticsOwner::audit_unnamed`]: flui_semantics::SemanticsOwner::audit_unnamed
//! [`SemanticsOwner::audit_target_size`]: flui_semantics::SemanticsOwner::audit_target_size
//! [`PipelineOwner::audit_text_contrast`]: flui_rendering::PipelineOwner::audit_text_contrast

use flui_interaction::events::PointerKind;
use flui_protocol::audit::{AuditFinding, AuditParams, AuditReport, AuditRule};
use flui_protocol::method;
use flui_protocol::tree::{ElementId, ElementRef, Node, ReadQuery, WindowId};
use flui_rendering::audit::{ContrastOutcome, ContrastReading, TextContrast};
use flui_semantics::{MinimumTarget, SemanticsOwner, WireReadError};
use rustc_hash::FxHashSet;

use super::agent::{IssuedHandles, placement_of, read_tree};
use super::agent_gesture::primary_pointer;
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
    match rule {
        AuditRule::Unnamed => semantics_rule(state, |owner| owner.audit_unnamed(placement)),
        AuditRule::TargetSize => match minimum_target(state) {
            (Some(minimum), pointer) => {
                match semantics_rule(state, |owner| owner.audit_target_size(minimum, placement)) {
                    RuleOutcome::Checked(found) => RuleOutcome::Checked(
                        found
                            .into_iter()
                            .map(|finding| judged_for(finding, pointer))
                            .collect::<Vec<AuditFinding>>(),
                    ),
                    outcome => outcome,
                }
            }
            (None, pointer) => RuleOutcome::NotChecked(format!(
                "the minimum target of {pointer}, this window's primary pointer, is not a finite \
                 positive size"
            )),
        },
        AuditRule::TextContrast => text_contrast(state, window),
        _ => RuleOutcome::NotChecked("this app's agent does not run this rule".to_owned()),
    }
}

/// A rule of `flui-semantics`, `read` over `state`'s committed tree.
fn semantics_rule(
    state: &PresentationState,
    read: impl FnOnce(&SemanticsOwner) -> Result<Vec<AuditFinding>, WireReadError>,
) -> RuleOutcome {
    state
        .pipeline()
        .with(|pipeline| match pipeline.semantics_owner().map(read) {
            None | Some(Err(WireReadError::NoTree)) => RuleOutcome::NotChecked(NO_TREE.to_owned()),
            Some(Ok(found)) => RuleOutcome::Checked(found),
            Some(Err(_)) => {
                RuleOutcome::NotChecked("the published semantics tree is malformed".to_owned())
            }
        })
}

/// Why a semantics rule did not run before the first committed tree.
const NO_TREE: &str = "no semantics tree has been committed yet";

/// The minimum target of `state`'s window for its primary pointer, the pointer a composed tap
/// plays with ([`primary_pointer`]), and the pointer's name for a finding (module docs).
fn minimum_target(state: &PresentationState) -> (Option<MinimumTarget>, &'static str) {
    let kind = primary_pointer(state);
    let name = if kind == PointerKind::Touch {
        "a touch contact"
    } else {
        "a mouse"
    };
    (
        MinimumTarget::try_new(state.gesture_settings.provider().min_target_for(kind)),
        name,
    )
}

/// `finding` of `target_size`, naming the pointer whose minimum it was judged against.
fn judged_for(mut finding: AuditFinding, pointer: &str) -> AuditFinding {
    finding.detail = format!(
        "{}, judged for {pointer}, this window's primary pointer",
        finding.detail
    );
    finding
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
    let inactive = inactive_elements(state, window);
    let mut findings = Vec::new();
    let mut unchecked: Vec<String> = Vec::new();
    for text in &texts {
        let element = text.element.map(flui_semantics::element_id_of);
        if element.is_some_and(|element| inactive.contains(&element)) {
            unchecked.push(format!(
                "\"{}\": its control is disabled, and an inactive control's text has no contrast \
                 requirement",
                quoted(&text.text)
            ));
            continue;
        }
        match (&text.outcome, text.element) {
            (ContrastOutcome::Meets(_), _) => {}
            (ContrastOutcome::Below(reading), Some(element)) => findings.push(AuditFinding::new(
                AuditRule::TextContrast,
                ElementRef::new(window, flui_semantics::element_id_of(element)),
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

/// The elements of `state`'s committed semantics tree that are inactive: disabled, or under a
/// disabled node, whose text WCAG 1.4.3 exempts from its contrast requirement. The walk reads at
/// most [`INACTIVE_READ_NODES`] nodes; a node past them is judged as an active one.
fn inactive_elements(state: &PresentationState, window: WindowId) -> FxHashSet<ElementId> {
    fn walk(nodes: &[Node], under_disabled: bool, inactive: &mut FxHashSet<ElementId>) {
        for node in nodes {
            let disabled = under_disabled || node.disabled;
            if disabled {
                inactive.insert(node.id);
            }
            walk(&node.children, disabled, inactive);
        }
    }
    let mut inactive = FxHashSet::default();
    let query = ReadQuery::new().max_nodes(INACTIVE_READ_NODES);
    if let Ok(tree) = read_tree(state, window, &query, true) {
        walk(&tree.roots, false, &mut inactive);
    }
    inactive
}

/// The most semantics nodes [`inactive_elements`] reads.
const INACTIVE_READ_NODES: usize = 1 << 16;

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
