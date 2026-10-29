//! An agent locates elements and reads whether input reaches them through the
//! owner inbox (ADR-0238, R6 and R11): the locator resolves over the committed
//! tree, and the actionability facts join the tree's with the render
//! hit-test's, run outside every semantics borrow.

use flui_protocol::{
    ActionName, ActionRequest, Actionability, ActionabilityParams, ElementRef, ErrorCode,
    LocateParams, Locator, LogicalPoint, Matches, Role, WindowId,
};
use flui_view::dev_agent::AgentWindow;
use std::sync::atomic::AtomicU32;

use super::agent_semantics::{answer, counter, find, frame, read};
use super::*;

static_assertions::assert_impl_all!(AgentReply<Matches>: Send);
static_assertions::assert_impl_all!(AgentReply<Actionability>: Send);

fn window_of(ui_runtime: &UiRuntime) -> WindowId {
    WindowId::from_u64(ui_runtime.presentation_id().as_u64())
        .expect("a presentation id packs a non-zero generation")
}

fn locate(ui_runtime: &UiRuntime, agent: &SemanticsAgent, locator: Locator) -> Matches {
    let reply = agent
        .locate(LocateParams::new(locator))
        .expect("the inbox has room");
    answer(ui_runtime, reply).expect("the committed tree resolves a locator")
}

fn actionability(
    ui_runtime: &UiRuntime,
    agent: &SemanticsAgent,
    params: ActionabilityParams,
) -> Result<Actionability, AgentError> {
    let reply = agent.actionability(params).expect("the inbox has room");
    answer(ui_runtime, reply)
}

/// A mounted counter, its agent and a sink, after the first frame.
fn shown(hide_at: Option<u32>) -> (UiRuntime, Arc<AtomicU32>, SemanticsAgent, ScriptedSink) {
    let ui_runtime = UiRuntime::for_test();
    let presses = counter(&ui_runtime, hide_at);
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    (ui_runtime, presses, agent, sink)
}

#[test]
fn a_locator_lists_the_element_a_read_reports_and_an_unknown_name_lists_none() {
    let (ui_runtime, _presses, agent, _sink) = shown(None);
    let button = find(&read(&ui_runtime, &agent), "Increment").id;

    let found = locate(
        &ui_runtime,
        &agent,
        Locator::role(Role::Button).name("Increment"),
    );
    assert_eq!(found.matches.len(), 1);
    let only = &found.matches[0];
    assert_eq!(
        only.element,
        ElementRef::new(window_of(&ui_runtime), button)
    );
    assert_eq!(only.node.name.as_deref(), Some("Increment"));
    assert_eq!(found.truncated, 0);

    let none = locate(&ui_runtime, &agent, Locator::named("Decrement"));
    assert!(none.matches.is_empty(), "no match is a list, not a fault");
}

#[test]
fn a_located_element_acts_through_the_owner_and_its_handle_goes_gone_with_it() {
    let (ui_runtime, presses, agent, mut sink) = shown(Some(1));
    let found = locate(&ui_runtime, &agent, Locator::named("Increment"));
    let button = found.matches[0].element.element;

    let tap = agent
        .act(ActionRequest::new(button, ActionName::Invoke))
        .expect("the inbox has room");
    answer(&ui_runtime, tap).expect("the handler runs");
    frame(&ui_runtime, &mut sink);
    frame(&ui_runtime, &mut sink);
    assert_eq!(presses.load(Ordering::Relaxed), 1);

    // The press replaced the button by a text: the handle the locator issued
    // is gone, and not one that was never issued.
    let again = agent
        .act(ActionRequest::new(button, ActionName::Invoke))
        .expect("the inbox has room");
    let failed = answer(&ui_runtime, again).expect_err("the button left the tree");
    assert_eq!(
        failed,
        AgentError::NodeNotFound {
            element: button,
            issued: true
        }
    );
    assert_eq!(failed.code(), ErrorCode::Gone);

    let facts = actionability(
        &ui_runtime,
        &agent,
        ActionabilityParams::new(ElementRef::new(window_of(&ui_runtime), button)),
    )
    .expect("a gone element is a fact, not a fault");
    assert!(!facts.attached);
    assert_eq!(
        facts.first_failed(),
        Some(flui_protocol::ActionabilityCheck::Attached)
    );
}

#[test]
fn a_button_is_visible_enabled_and_receives_the_pointer_at_its_centre() {
    let (ui_runtime, _presses, agent, _sink) = shown(None);
    let button = find(&read(&ui_runtime, &agent), "Increment");
    let rect = button.surface_rect.expect("a laid-out button has bounds");
    let element = ElementRef::new(window_of(&ui_runtime), button.id);

    let facts = actionability(&ui_runtime, &agent, ActionabilityParams::new(element))
        .expect("the facts read");
    assert!(facts.attached && facts.visible && facts.enabled);
    assert!(
        facts.receives,
        "the hit-test at the centre reaches the button"
    );
    assert_eq!(facts.covered_by, None);
    let point = facts.point.expect("a visible element has a point");
    assert!((point.x - (f64::from(rect.x) + f64::from(rect.width) / 2.0)).abs() < 1.0);
    assert!((point.y - (f64::from(rect.y) + f64::from(rect.height) / 2.0)).abs() < 1.0);
}

#[test]
fn a_point_on_another_element_names_what_covers_it() {
    let (ui_runtime, _presses, agent, _sink) = shown(None);
    let tree = read(&ui_runtime, &agent);
    let button = find(&tree, "Increment");
    let text = find(&tree, "Count: 0")
        .surface_rect
        .expect("a laid-out text has bounds");
    let on_text = LogicalPoint::new(
        f64::from(text.x) + f64::from(text.width) / 2.0,
        f64::from(text.y) + f64::from(text.height) / 2.0,
    );
    let mut params = ActionabilityParams::new(ElementRef::new(window_of(&ui_runtime), button.id));
    params.point = Some(on_text);

    let facts = actionability(&ui_runtime, &agent, params).expect("the facts read");
    assert!(
        !facts.receives,
        "the pointer lands on the count, not the button"
    );
    assert_eq!(facts.point, Some(on_text));
    let cover = facts.covered_by.expect("what was reached instead is named");
    assert!(cover.contains("Count: 0"), "{cover}");
}

#[test]
fn an_element_is_stable_once_a_frame_has_passed_with_the_same_bounds() {
    let (ui_runtime, _presses, agent, mut sink) = shown(None);
    let button = find(&read(&ui_runtime, &agent), "Increment").id;
    let params = ActionabilityParams::new(ElementRef::new(window_of(&ui_runtime), button));

    let first = actionability(&ui_runtime, &agent, params).expect("the facts read");
    assert!(!first.stable, "there is nothing yet to compare with");
    frame(&ui_runtime, &mut sink);
    let second = actionability(&ui_runtime, &agent, params).expect("the facts read");
    assert!(second.stable, "same bounds as the call before the frame");
    assert_eq!(second.rect, first.rect);
    assert_eq!(second.first_failed(), None);
}

#[test]
fn an_element_of_another_window_is_not_attached_here() {
    let (ui_runtime, _presses, agent, _sink) = shown(None);
    let button = find(&read(&ui_runtime, &agent), "Increment").id;
    let other =
        WindowId::from_u64(window_of(&ui_runtime).get() + (1 << 32)).expect("a non-zero window id");

    let facts = actionability(
        &ui_runtime,
        &agent,
        ActionabilityParams::new(ElementRef::new(other, button)),
    )
    .expect("the facts read");
    assert!(
        !facts.attached,
        "element numbers are scoped to their window"
    );
}

#[test]
fn locating_before_the_first_semantics_frame_is_busy() {
    let ui_runtime = UiRuntime::for_test();
    let _presses = counter(&ui_runtime, None);
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");

    let reply = agent
        .locate(LocateParams::new(Locator::named("Increment")))
        .expect("the inbox has room");
    let failed = answer(&ui_runtime, reply).expect_err("no tree is committed yet");
    assert_eq!(failed, AgentError::NoTreeYet);
    assert_eq!(failed.code(), ErrorCode::Busy);
}

#[test]
fn a_panic_while_reading_the_facts_is_the_owners_and_answers_platform_first() {
    let (ui_runtime, _presses, agent, _sink) = shown(None);
    let button = find(&read(&ui_runtime, &agent), "Increment").id;
    let mut reply = agent
        .actionability(ActionabilityParams::new(ElementRef::new(
            window_of(&ui_runtime),
            button,
        )))
        .expect("the inbox has room");

    let pipeline = ui_runtime.pipeline_for_test();
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        pipeline.with_mut(|_| ui_runtime.drain_commands())
    }));
    assert!(
        unwound.is_err(),
        "reading against a borrowed pipeline panics"
    );
    let failed = reply
        .try_take()
        .expect("the reply was answered before unwinding")
        .expect_err("the panic is the answer");
    assert_eq!(failed, AgentError::ResolvePanicked { element: button });
    assert_eq!(failed.code(), ErrorCode::Platform);
    assert!(!failed.may_have_run());
}

#[test]
fn a_development_window_locates_and_reads_facts_until_its_presentation_closes() {
    let (ui_runtime, _presses, _agent, _sink) = shown(None);
    let window: AgentWindow = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");

    let mut located = window
        .locate(LocateParams::new(Locator::named("Increment")))
        .expect("the window is open");
    let _report = ui_runtime.drain_commands();
    let found = located
        .try_take()
        .expect("the drain answered")
        .expect("the tree resolves");
    let button = found.matches[0].element;

    let mut facts = window
        .actionability(ActionabilityParams::new(button))
        .expect("the window is open");
    let _report = ui_runtime.drain_commands();
    let facts = facts
        .try_take()
        .expect("the drain answered")
        .expect("the facts read");
    assert!(facts.attached && facts.receives);
}

/// A hit-test that panics in a render object is a render fault the agent
/// answers once and the owner resumes: the reply names the hit-test and the
/// element, and nothing reached the app.
#[test]
fn a_render_object_that_panics_in_the_hit_test_answers_platform_before_the_owner_resumes() {
    use flui_foundation::geometry::Size;
    use flui_rendering::prelude::{
        BoxHitTestContext, BoxLayoutContext, BoxParentData, Leaf, PaintCx, RenderBox,
    };
    use flui_view::{IntoView, RenderView, View};
    use flui_widgets::{RawButton, Stack, Text};

    #[derive(Debug, Default)]
    struct PanicsWhenHitBox;
    impl flui_foundation::Diagnosticable for PanicsWhenHitBox {}
    impl RenderBox for PanicsWhenHitBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;
        fn perform_layout(
            &mut self,
            _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
        ) -> flui_rendering::RenderResult<Size> {
            Ok(Size::new(200.0, 200.0))
        }
        fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
        fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
            panic!("a render object that cannot be hit")
        }
    }

    #[derive(Clone)]
    struct PanicsWhenHit;
    impl RenderView for PanicsWhenHit {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = PanicsWhenHitBox;
        fn create_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
        ) -> PanicsWhenHitBox {
            PanicsWhenHitBox
        }
        fn update_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
            _render_object: &mut PanicsWhenHitBox,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }
    impl View for PanicsWhenHit {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::render_variable(self)
        }
    }

    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .enter(|ui_runtime| {
            ui_runtime.attach_root_widget_to_for_test(
                ui_runtime.presentation_id(),
                &Stack::new(vec![
                    RawButton::new(Text::new("Press"))
                        .on_press(|_| {})
                        .into_view()
                        .boxed(),
                    PanicsWhenHit.into_view().boxed(),
                ]),
            )
        })
        .expect("the stack attaches");
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    let element = find(&read(&ui_runtime, &agent), "Press").id;

    let mut reply = agent
        .actionability(ActionabilityParams::new(ElementRef::new(
            window_of(&ui_runtime),
            element,
        )))
        .expect("the inbox has room");
    let unwound = catch_unwind(AssertUnwindSafe(|| ui_runtime.drain_commands()));
    assert!(
        unwound.is_err(),
        "the render object's panic continues on the owner"
    );
    let failed = reply
        .try_take()
        .expect("the reply was answered before unwinding")
        .expect_err("the panic is the answer");
    assert_eq!(failed, AgentError::HitTestPanicked { element });
    assert_eq!(failed.code(), ErrorCode::Platform);
    assert!(!failed.may_have_run(), "a hit-test only reads");
}
