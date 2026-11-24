//! `audit.a11y` as the agent serves it (ADR-0238 §10, ADR-0400 §5): the rules asked for run
//! over the window's committed tree and last frame, a planted violation is found on its
//! element, and a rule the agent cannot run keeps the report from `pass`.

use flui_painting::styling::Color;
use flui_painting::typography::TextStyle;
use flui_platform::TargetPlatform;
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
    // No read has reported an element yet: only the audit's answer can record its findings'.
    let first = audit(
        &ui_runtime,
        &window,
        &[AuditRule::Unnamed, AuditRule::TextContrast],
    );
    let state = ui_runtime
        .presentations
        .get(ui_runtime.presentation_id())
        .expect("the primary presentation is hosted");
    let issued = crate::ui_runtime::agent::issued_of(state);
    assert_eq!(first.findings.len(), 2, "{first:#?}");
    for finding in &first.findings {
        assert!(
            issued.borrow().was_issued(finding.node.element),
            "the element of {finding:?} is recorded as issued"
        );
    }
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
    assert_ne!(report.not_checked[0].reason, "", "the reason is given");

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

/// Asking for a rule twice runs it once: it is checked once and its findings are not repeated.
#[test]
fn a_rule_asked_twice_runs_once() {
    let (ui_runtime, window, _sink) = planted();
    let once = audit(
        &ui_runtime,
        &window,
        &[AuditRule::TargetSize, AuditRule::Unnamed],
    );
    let twice = audit(
        &ui_runtime,
        &window,
        &[
            AuditRule::TargetSize,
            AuditRule::Unnamed,
            AuditRule::TargetSize,
            AuditRule::Unnamed,
        ],
    );
    assert_eq!(
        twice.checked,
        vec![AuditRule::TargetSize, AuditRule::Unnamed],
        "{twice:#?}"
    );
    assert_eq!(twice.findings, once.findings, "{twice:#?}");
}

/// A white strip with one 30 px button.
#[derive(Clone, StatelessView)]
struct ThirtyPixelButton;

impl StatelessView for ThirtyPixelButton {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        ColoredBox::new(Color::WHITE).child(Column::new((RawButton::new(
            SizedBox::square(30.0).child(Text::new("Go")),
        )
        .on_pressed(|_| {}),)))
    }
}

/// `target_size` judges against the minimum of the platform's primary pointer, from the
/// window's gesture settings: the touch minimum (44 px in the neutral presets) on Android, iOS
/// and Fuchsia, where a 30 px button is too small, and the mouse minimum (24 px) elsewhere,
/// where it is not.
#[test]
fn target_size_judges_against_the_platform_primary_pointer_minimum() {
    for (platform, found) in [
        (TargetPlatform::Android, true),
        (TargetPlatform::iOS, true),
        (TargetPlatform::Fuchsia, true),
        (TargetPlatform::Linux, false),
        (TargetPlatform::MacOS, false),
        (TargetPlatform::Windows, false),
        (TargetPlatform::Unknown, false),
    ] {
        let ui_runtime = UiRuntime::try_new(
            test_window().with_platform(platform),
            1.0,
            crate::runtime_services::RuntimeHostServices::new(
                Arc::new(|| {}),
                Arc::new(AtomicBool::new(false)),
                &flui_painting::FontCollection::new(),
                flui_scheduler::ClockSource::Platform,
            ),
        )
        .expect("runtime");
        ui_runtime
            .enter(|ui_runtime| {
                ui_runtime.attach_root_widget_to_for_test(
                    ui_runtime.presentation_id(),
                    &ThirtyPixelButton,
                )
            })
            .expect("the page attaches");
        let window = ui_runtime
            .dev_agent_window(ui_runtime.presentation_id())
            .expect("the ui_runtime hosts its primary presentation");
        let mut sink = ScriptedSink::always_presents();
        frame(&ui_runtime, &mut sink);
        let report = audit(&ui_runtime, &window, &[AuditRule::TargetSize]);
        assert_eq!(report.checked, vec![AuditRule::TargetSize], "{platform:?}");
        let details: Vec<&str> = report
            .findings
            .iter()
            .map(|finding| finding.detail.as_str())
            .collect();
        if found {
            assert!(
                matches!(details.as_slice(), [detail] if detail.contains("30 × 30") && detail.contains("44 px minimum")),
                "{platform:?}: {report:#?}"
            );
        } else {
            assert!(details.is_empty(), "{platform:?}: {report:#?}");
        }
    }
}

/// A faint text on white, then four texts with nothing painted behind them, one longer than a
/// finding quotes.
#[derive(Clone, StatelessView)]
struct HalfJudged;

/// The long text [`HalfJudged`] shows: past the 40 characters a finding quotes.
const LONG: &str = "A sentence long enough to be cut where a finding quotes it";

impl StatelessView for HalfJudged {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        let ink = |color| TextStyle::default().with_color(color).with_font_size(16.0);
        Column::new((
            ColoredBox::new(Color::WHITE)
                .child(Text::new("Faint").style(ink(Color::rgb(0xcc, 0xcc, 0xcc)))),
            Text::new("Bare one").style(ink(Color::BLACK)),
            Text::new("Bare two").style(ink(Color::BLACK)),
            Text::new(LONG).style(ink(Color::BLACK)),
            Text::new("Bare four").style(ink(Color::BLACK)),
        ))
    }
}

/// When some texts cannot be judged, `text_contrast` is listed in `not_checked` with how many,
/// the first three quoted (a long one cut at 40 characters) and the rest counted, while the
/// findings of the texts it judged are still reported; it is not in `checked`.
#[test]
fn text_contrast_with_texts_it_cannot_judge_reports_a_partial_check() {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .enter(|ui_runtime| {
            ui_runtime.attach_root_widget_to_for_test(ui_runtime.presentation_id(), &HalfJudged)
        })
        .expect("the page attaches");
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    let report = audit(&ui_runtime, &window, &[AuditRule::TextContrast]);
    assert!(report.checked.is_empty(), "{report:#?}");
    assert_eq!(
        report.verdict,
        Verdict::Fail,
        "the faint text fails it: {report:#?}"
    );
    let faint = find(&read(&ui_runtime, &window), "Faint").id;
    let nodes: Vec<_> = report
        .findings
        .iter()
        .map(|finding| finding.node.element)
        .collect();
    assert_eq!(nodes, vec![faint], "{report:#?}");
    let [entry] = report.not_checked.as_slice() else {
        panic!("one rule is not checked: {report:#?}");
    };
    assert_eq!(entry.rule, AuditRule::TextContrast);
    let why = flui_rendering::audit::ContrastUnknown::NothingOpaqueBehind;
    let cut: String = LONG.chars().take(40).collect();
    assert_eq!(
        entry.reason,
        format!(
            "4 of 5 texts could not be judged: \"Bare one\": {why}; \"Bare two\": {why}; \
             \"{cut}…\": {why}; and 1 more"
        ),
    );
}
