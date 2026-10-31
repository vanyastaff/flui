//! An agent's reads of a window's trees (ADR-0238 §6, ADR-0242): the element, render and layer
//! trees as the typed snapshot they describe themselves with, a subtree by its handle or by a
//! reference that must name this window, and the limits.

use std::num::NonZeroU32;

use flui_foundation::PresentationId;
use flui_foundation::inspect::{Link, NodeRef};
use flui_protocol::{
    ErrorCode, InspectLevel, InspectNode, InspectParams, InspectReading, WindowId,
};
use flui_view::dev_agent::InspectTarget;

use super::agent_semantics::{counter, frame};
use super::pump_transaction::manual_clock_ui_runtime;
use super::*;

static_assertions::assert_impl_all!(AgentReply<InspectReading>: Send);

fn window(ui_runtime: &UiRuntime) -> WindowId {
    WindowId::from_u64(ui_runtime.presentation_id().as_u64())
        .expect("a presentation id packs a non-zero generation")
}

/// A counter on a manual clock, one frame in, and its agent.
fn shown() -> (UiRuntime, SemanticsAgent) {
    let clock = flui_foundation::ManualClock::new();
    let ui_runtime = manual_clock_ui_runtime(&clock);
    let _presses = counter(&ui_runtime, None);
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    (ui_runtime, agent)
}

fn inspect(
    ui_runtime: &UiRuntime,
    agent: &SemanticsAgent,
    target: InspectTarget,
    params: InspectParams,
) -> Result<InspectReading, AgentError> {
    let mut reply = agent.inspect(target, params).expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    reply.try_take().expect("the owner answered at the drain")
}

fn whole(
    ui_runtime: &UiRuntime,
    agent: &SemanticsAgent,
    target: InspectTarget,
) -> Result<InspectReading, AgentError> {
    inspect(
        ui_runtime,
        agent,
        target,
        InspectParams::new(window(ui_runtime)),
    )
}

fn nodes(node: &InspectNode) -> Vec<&InspectNode> {
    let mut all = vec![node];
    let mut next = 0;
    while next < all.len() {
        let children = all[next].children.iter().collect::<Vec<_>>();
        all.extend(children);
        next += 1;
    }
    all
}

#[test]
fn each_tree_answers_with_its_own_handles() {
    let (ui_runtime, agent) = shown();
    for (target, prefix) in [(InspectTarget::Widgets, 'v'), (InspectTarget::Render, 'r')] {
        let reading = whole(&ui_runtime, &agent, target)
            .unwrap_or_else(|error| panic!("{target:?}: {error}"));
        assert_eq!(reading.roots.len(), 1, "{target:?} has one root");
        let all = nodes(&reading.roots[0]);
        assert!(all.len() > 1, "{target:?}: a counter has more than a root");
        assert_eq!(
            usize::try_from(reading.count).expect("a count fits"),
            all.len(),
            "{target:?}: the count is the nodes reported"
        );
        assert!(!reading.truncated, "{target:?}: nothing was cut");
        for node in all {
            let link: Link = node
                .id
                .parse()
                .unwrap_or_else(|error| panic!("{target:?}: {error}"));
            assert_eq!(link.prefix(), prefix, "{target:?}: {}", node.id);
            assert!(!node.kind.is_empty(), "{target:?}: {} has a kind", node.id);
        }
    }
}

#[test]
fn a_subtree_is_read_by_its_handle_or_by_a_reference_to_this_window() {
    let (ui_runtime, agent) = shown();
    let all = whole(&ui_runtime, &agent, InspectTarget::Render).expect("the render tree");
    let child = &all.roots[0].children[0];

    let mut params = InspectParams::new(window(&ui_runtime));
    params.root = Some(child.id.clone());
    let sub = inspect(&ui_runtime, &agent, InspectTarget::Render, params).expect("a subtree");
    assert_eq!(sub.roots[0].id, child.id);
    assert!(sub.count < all.count, "a subtree is a part of the tree");

    let link = child.id.parse::<Link>().expect("a handle");
    let reference = NodeRef::new(ui_runtime.presentation_id(), link);
    let mut params = InspectParams::new(window(&ui_runtime));
    params.root = Some(reference.to_string());
    let by_reference =
        inspect(&ui_runtime, &agent, InspectTarget::Render, params).expect("by reference");
    assert_eq!(by_reference, sub, "the reference reads the same subtree");
}

#[test]
fn a_reference_to_another_window_is_unknown_and_never_read_here() {
    let (ui_runtime, agent) = shown();
    let all = whole(&ui_runtime, &agent, InspectTarget::Render).expect("the render tree");
    let link = all.roots[0].id.parse::<Link>().expect("a handle");

    let own = ui_runtime.presentation_id();
    let reused = PresentationId::new_gen(
        own.index(),
        NonZeroU32::new(own.generation().get() + 1).expect("a generation"),
    );
    let elsewhere = PresentationId::new_gen(own.index() + 1, own.generation());
    for other in [reused, elsewhere] {
        let mut params = InspectParams::new(window(&ui_runtime));
        params.root = Some(NodeRef::new(other, link).to_string());
        let error = inspect(&ui_runtime, &agent, InspectTarget::Render, params)
            .expect_err("a reference to another window");
        assert_eq!(error, AgentError::UnknownPresentation, "{other:?}");
        assert_eq!(error.code(), ErrorCode::UnknownHandle);
        assert_eq!(error.handle_kind(), Some("window"));
    }
}

#[test]
fn a_root_that_is_not_a_node_of_the_tree_asked_is_refused_or_gone() {
    let (ui_runtime, agent) = shown();
    let render = whole(&ui_runtime, &agent, InspectTarget::Render).expect("the render tree");

    for (root, target) in [
        (render.roots[0].id.as_str(), InspectTarget::Widgets),
        ("banana", InspectTarget::Widgets),
        ("p1/", InspectTarget::Render),
        ("", InspectTarget::Render),
    ] {
        let mut params = InspectParams::new(window(&ui_runtime));
        params.root = Some(root.to_owned());
        let error =
            inspect(&ui_runtime, &agent, target, params).expect_err("not a handle of that tree");
        assert_eq!(error.code(), ErrorCode::InvalidArgument, "`{root}`");
    }

    let mut params = InspectParams::new(window(&ui_runtime));
    params.root = Some("r999999".to_owned());
    let error = inspect(&ui_runtime, &agent, InspectTarget::Render, params)
        .expect_err("no such render object");
    assert_eq!(error.code(), ErrorCode::Gone);
    assert_eq!(
        error,
        AgentError::NodeGone {
            node: "r999999".to_owned()
        }
    );
}

#[test]
fn the_limits_cut_the_tree_and_say_so() {
    let (ui_runtime, agent) = shown();
    let mut params = InspectParams::new(window(&ui_runtime));
    params.max_nodes = Some(2);
    let cut = inspect(&ui_runtime, &agent, InspectTarget::Widgets, params).expect("a cut read");
    assert_eq!(cut.count, 2);
    assert!(cut.truncated);
    assert!(
        nodes(&cut.roots[0])
            .iter()
            .any(|node| node.omitted_children > 0)
    );

    let mut params = InspectParams::new(window(&ui_runtime));
    params.max_depth = Some(0);
    let root_only =
        inspect(&ui_runtime, &agent, InspectTarget::Render, params).expect("a root only");
    assert_eq!(root_only.count, 1);
    assert!(root_only.truncated);
    assert_eq!(root_only.roots[0].children, []);
}

#[test]
fn a_deeper_level_adds_fields() {
    let (ui_runtime, agent) = shown();
    let fields = |level| {
        let mut params = InspectParams::new(window(&ui_runtime));
        params.level = level;
        let reading =
            inspect(&ui_runtime, &agent, InspectTarget::Render, params).expect("the render tree");
        nodes(&reading.roots[0])
            .iter()
            .map(|node| node.fields.len())
            .sum::<usize>()
    };
    assert!(fields(InspectLevel::Debug) > fields(InspectLevel::Summary));
}

#[test]
fn layers_are_not_readable_because_the_sink_takes_the_scene() {
    let (ui_runtime, agent) = shown();

    let error = whole(&ui_runtime, &agent, InspectTarget::Layers)
        .expect_err("a presented scene is the sink's");
    assert_eq!(error, AgentError::LayersNotKept);
    assert_eq!(error.code(), ErrorCode::NotSupported);
}

#[test]
fn a_window_that_closed_answers_gone() {
    let mut ui_runtime = UiRuntime::for_test();
    let second = ui_runtime.install_second_presentation_for_test();
    let agent = ui_runtime
        .semantics_agent(second)
        .expect("the ui_runtime hosts the second presentation");
    assert!(ui_runtime.close_presentation_entered(second));

    let error = inspect(
        &ui_runtime,
        &agent,
        InspectTarget::Widgets,
        InspectParams::new(
            WindowId::from_u64(second.as_u64()).expect("a presentation id packs a generation"),
        ),
    )
    .expect_err("the window is gone");
    assert_eq!(error, AgentError::PresentationGone);
}
