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

/// A render object's `debug_name` is app code the read calls: its panic answers the request with
/// `InspectPanicked` before the panic continues on the owner thread, so the agent is not told
/// the runtime is gone.
#[test]
fn a_render_object_that_panics_while_read_answers_platform_before_the_owner_resumes() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use flui_foundation::geometry::Size;
    use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintCx, RenderBox};
    use flui_view::{IntoView, RenderView, View};
    use flui_widgets::{Stack, Text};

    #[derive(Debug)]
    struct PanicsWhenNamedBox(Arc<AtomicBool>);
    impl flui_foundation::inspect::Inspect for PanicsWhenNamedBox {
        fn kind(&self) -> &'static str {
            assert!(
                !self.0.load(Ordering::SeqCst),
                "a render object that cannot be named"
            );
            "PanicsWhenNamedBox"
        }

        fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
    }
    impl RenderBox for PanicsWhenNamedBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;
        fn perform_layout(
            &mut self,
            _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
        ) -> flui_rendering::RenderResult<Size> {
            Ok(Size::new(10.0, 10.0))
        }
        fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
    }

    #[derive(Clone)]
    struct PanicsWhenNamed(Arc<AtomicBool>);
    impl RenderView for PanicsWhenNamed {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = PanicsWhenNamedBox;
        fn create_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
        ) -> PanicsWhenNamedBox {
            PanicsWhenNamedBox(Arc::clone(&self.0))
        }
        fn update_render_object(
            &self,
            _ctx: &flui_view::RenderObjectContext<'_>,
            _render_object: &mut PanicsWhenNamedBox,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }
    impl View for PanicsWhenNamed {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::render_variable(self)
        }
    }

    let panics = Arc::new(AtomicBool::new(false));
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .enter(|ui_runtime| {
            ui_runtime.attach_root_widget_to_for_test(
                ui_runtime.presentation_id(),
                &Stack::new(vec![
                    Text::new("Press").into_view().boxed(),
                    PanicsWhenNamed(Arc::clone(&panics)).into_view().boxed(),
                ]),
            )
        })
        .expect("the stack attaches");
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);

    panics.store(true, Ordering::SeqCst);
    let mut reply = agent
        .inspect(
            InspectTarget::Render,
            InspectParams::new(window(&ui_runtime)),
        )
        .expect("the inbox has room");
    let unwound = catch_unwind(AssertUnwindSafe(|| ui_runtime.drain_commands()));
    panics.store(false, Ordering::SeqCst);
    assert!(unwound.is_err(), "the panic continues on the owner");
    let failed = reply
        .try_take()
        .expect("the reply was answered before unwinding")
        .expect_err("the panic is the answer");
    assert_eq!(failed, AgentError::InspectPanicked);
    assert_eq!(failed.code(), ErrorCode::Platform);
    assert!(!failed.may_have_run(), "a read changes nothing");
}
