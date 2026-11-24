//! `audit.a11y` as the agent serves it (ADR-0238 §10, ADR-0400 §5): the rules asked for run
//! over the window's committed tree and last frame, a planted violation is found on its
//! element, and a rule the agent cannot run keeps the report from `pass`.

use flui_painting::styling::Color;
use flui_painting::typography::TextStyle;
use flui_protocol::audit::{AuditParams, AuditReport, AuditRule};
use flui_protocol::method;
use flui_protocol::verdict::Verdict;
use flui_view::dev_agent::AgentWindow;
use flui_widgets::{ColoredBox, Column, RawButton, Text};

use super::agent_semantics::{answer, find, frame, read};
use super::*;

/// A white page with a readable text, a faint one, a named button and an unnamed 10 px one.
#[derive(Clone, StatelessView)]
struct Planted;

impl StatelessView for Planted {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        let ink = |color| TextStyle::default().with_color(color).with_font_size(16.0);
        ColoredBox::new(Color::WHITE).child(Column::new((
            Text::new("Readable").style(ink(Color::BLACK)),
            Text::new("Faint").style(ink(Color::rgb(0xcc, 0xcc, 0xcc))),
            RawButton::new(
                SizedBox::new(120.0, 48.0).child(Text::new("Named").style(ink(Color::BLACK))),
            )
            .on_pressed(|_| {}),
            RawButton::new(SizedBox::square(10.0)).on_pressed(|_| {}),
        )))
    }
}

/// A runtime showing [`Planted`], one frame in, and its window.
fn planted() -> (UiRuntime, AgentWindow, ScriptedSink) {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .enter(|ui_runtime| {
            ui_runtime.attach_root_widget_to_for_test(ui_runtime.presentation_id(), &Planted)
        })
        .expect("the page attaches");
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    (ui_runtime, window, sink)
}

/// The first button with no name under `nodes`, in tree order.
fn unnamed_button(nodes: &[flui_protocol::tree::Node]) -> Option<flui_protocol::tree::ElementId> {
    nodes.iter().find_map(|node| {
        (node.role == flui_protocol::wire::Role::Button && node.name.is_none())
            .then_some(node.id)
            .or_else(|| unnamed_button(&node.children))
    })
}

/// The report of `rules` over `window`.
fn audit(ui_runtime: &UiRuntime, window: &AgentWindow, rules: &[AuditRule]) -> AuditReport {
    let mut params = AuditParams::new(window.id());
    params.rules = rules.to_vec();
    let reply = window
        .call::<method::AuditA11y>(params)
        .expect("the inbox has room");
    answer(ui_runtime, reply).expect("the audit is served")
}

/// Each rule finds its planted violation on the element that has it, and only there: the
/// unnamed button for `unnamed`, the 10 px button for `target_size`, the faint text for
/// `text_contrast`. Every element a finding names is recorded as issued, so the agent's next
/// call on it is not an unknown handle.
#[test]
fn an_audit_finds_each_planted_violation_on_its_element() {
    let (ui_runtime, window, _sink) = planted();
    let tree = read(&ui_runtime, &window);
    let faint = find(&tree, "Faint").id;
    let named = find(&tree, "Named").id;
    let unnamed = unnamed_button(&tree.roots).unwrap_or_else(|| {
        panic!(
            "no unnamed button in\n{}",
            flui_protocol::tree::outline(&tree.roots)
        )
    });

    for (rule, element) in [
        (AuditRule::Unnamed, unnamed),
        (AuditRule::TargetSize, unnamed),
        (AuditRule::TextContrast, faint),
    ] {
        let report = audit(&ui_runtime, &window, &[rule]);
        assert_eq!(report.checked, vec![rule], "{rule:?}: {report:#?}");
        assert!(report.not_checked.is_empty(), "{rule:?}: {report:#?}");
        assert_eq!(report.verdict, Verdict::Fail, "{rule:?}: {report:#?}");
        let nodes: Vec<_> = report
            .findings
            .iter()
            .map(|finding| finding.node.element)
            .collect();
        assert_eq!(nodes, vec![element], "{rule:?}: {report:#?}");
        assert_ne!(element, named, "the named button breaks no rule");
        let state = ui_runtime
            .presentations
            .get(ui_runtime.presentation_id())
            .expect("the primary presentation is hosted");
        assert!(
            crate::ui_runtime::agent::issued_of(state)
                .borrow()
                .was_issued(element),
            "{rule:?}: the finding's element is recorded as issued"
        );
    }
    let contrast = audit(&ui_runtime, &window, &[AuditRule::TextContrast]);
    let detail = &contrast.findings[0].detail;
    assert!(
        detail.contains("\"Faint\"") && detail.contains("#cccccc on #ffffff"),
        "the detail names the text and its colours: {detail}"
    );
}

/// A rule the agent does not run is listed with why and the verdict is `cannot_verify`, never
/// `pass`; asking for no rule asks for every rule, the ones it runs checked and the rest listed.
#[test]
fn a_rule_the_agent_cannot_run_makes_the_audit_cannot_verify() {
    let (ui_runtime, window, _sink) = planted();
    let report = audit(&ui_runtime, &window, &[AuditRule::FocusOrder]);
    assert_eq!(report.verdict, Verdict::CannotVerify, "{report:#?}");
    assert!(
        report.checked.is_empty() && report.findings.is_empty(),
        "{report:#?}"
    );
    let listed: Vec<AuditRule> = report.not_checked.iter().map(|entry| entry.rule).collect();
    assert_eq!(listed, vec![AuditRule::FocusOrder], "{report:#?}");
    assert!(!report.not_checked[0].reason.is_empty());

    let every = audit(&ui_runtime, &window, &[]);
    assert_eq!(
        every.checked,
        vec![
            AuditRule::Unnamed,
            AuditRule::TargetSize,
            AuditRule::TextContrast
        ],
        "{every:#?}"
    );
    let listed: Vec<AuditRule> = every.not_checked.iter().map(|entry| entry.rule).collect();
    assert_eq!(
        listed,
        vec![
            AuditRule::FocusOrder,
            AuditRule::NoAlternative,
            AuditRule::AlternativeDiffers
        ],
        "{every:#?}"
    );
    assert_eq!(
        every.verdict,
        Verdict::Fail,
        "the planted findings still fail it"
    );
}

/// Before the window's first frame no rule can run: each is listed as not checked, and the
/// report is `cannot_verify`, not an error.
#[test]
fn an_audit_before_the_first_frame_checks_nothing_and_says_so() {
    let ui_runtime = UiRuntime::for_test();
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let report = audit(
        &ui_runtime,
        &window,
        &[AuditRule::Unnamed, AuditRule::TextContrast],
    );
    assert_eq!(report.verdict, Verdict::CannotVerify, "{report:#?}");
    assert!(report.checked.is_empty(), "{report:#?}");
    let listed: Vec<AuditRule> = report.not_checked.iter().map(|entry| entry.rule).collect();
    assert_eq!(
        listed,
        vec![AuditRule::Unnamed, AuditRule::TextContrast],
        "{report:#?}"
    );
}
