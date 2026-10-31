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
    impl flui_foundation::Diagnosticable for PanicsWhenNamedBox {}
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
        fn debug_name(&self) -> &'static str {
            assert!(
                !self.0.load(Ordering::SeqCst),
                "a render object that cannot be named"
            );
            "PanicsWhenNamed"
        }
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

fn park_layers(
    ui_runtime: &UiRuntime,
    agent: &SemanticsAgent,
    params: InspectParams,
) -> AgentReply<InspectReading> {
    let reply = agent
        .inspect(InspectTarget::Layers, params)
        .expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    reply
}

#[test]
fn the_layers_are_read_from_the_next_scene_the_window_shows() {
    let (ui_runtime, agent) = shown();
    let mut parked = park_layers(&ui_runtime, &agent, InspectParams::new(window(&ui_runtime)));
    assert!(
        parked.try_take().is_none(),
        "the read waits for a scene, it does not answer from the last one"
    );

    frame(&ui_runtime, &mut ScriptedSink::always_presents());
    let reading = parked
        .try_take()
        .expect("the window showed a scene")
        .expect("the layers are read");
    assert_eq!(reading.roots.len(), 1);
    let all = nodes(&reading.roots[0]);
    assert!(all.len() > 1, "a counter paints more than a root layer");
    for node in all {
        let link: Link = node.id.parse().expect("a layer handle");
        assert_eq!(link.prefix(), 'l', "{}", node.id);
    }

    frame(&ui_runtime, &mut ScriptedSink::always_presents());
    assert!(
        ui_runtime
            .presentations
            .get(ui_runtime.presentation_id())
            .expect("the primary presentation is open")
            .agent_layer_reads
            .borrow()
            .is_empty(),
        "a scene answers every read parked for it, and later ones keep no copy"
    );
}

#[test]
fn a_window_parks_a_bounded_number_of_layer_reads() {
    let (ui_runtime, agent) = shown();
    let mut parked: Vec<_> = (0..16)
        .map(|_| park_layers(&ui_runtime, &agent, InspectParams::new(window(&ui_runtime))))
        .collect();
    let mut refused = park_layers(&ui_runtime, &agent, InspectParams::new(window(&ui_runtime)));
    let error = refused
        .try_take()
        .expect("a full list answers at once")
        .expect_err("busy");
    assert_eq!(error.code(), ErrorCode::Busy);
    assert!(matches!(error, AgentError::TreeBusy(_)), "{error:?}");

    frame(&ui_runtime, &mut ScriptedSink::always_presents());
    for read in &mut parked {
        assert!(read.try_take().expect("answered by the scene").is_ok());
    }
}

#[test]
fn a_layer_read_names_a_layer_and_this_window_before_it_waits() {
    let (ui_runtime, agent) = shown();
    let mut params = InspectParams::new(window(&ui_runtime));
    params.root = Some("r1".to_owned());
    let error = park_layers(&ui_runtime, &agent, params)
        .try_take()
        .expect("a bad root answers at once")
        .expect_err("not a layer");
    assert_eq!(error.code(), ErrorCode::InvalidArgument);

    let own = ui_runtime.presentation_id();
    let elsewhere = PresentationId::new_gen(own.index() + 1, own.generation());
    let link: Link = "l1".parse().expect("a layer handle");
    let mut params = InspectParams::new(window(&ui_runtime));
    params.root = Some(NodeRef::new(elsewhere, link).to_string());
    let error = park_layers(&ui_runtime, &agent, params)
        .try_take()
        .expect("another window answers at once")
        .expect_err("another window");
    assert_eq!(error, AgentError::UnknownPresentation);
}

#[test]
fn a_layer_read_whose_window_closes_before_its_scene_is_gone() {
    let mut ui_runtime = UiRuntime::for_test();
    let second = ui_runtime.install_second_presentation_for_test();
    let agent = ui_runtime
        .semantics_agent(second)
        .expect("the ui_runtime hosts the second presentation");
    let mut parked = park_layers(
        &ui_runtime,
        &agent,
        InspectParams::new(WindowId::from_u64(second.as_u64()).expect("a generation")),
    );
    assert!(parked.try_take().is_none());

    assert!(ui_runtime.close_presentation_entered(second));
    let error = parked
        .try_take()
        .expect("closing answers it")
        .expect_err("the window is gone");
    assert_eq!(error, AgentError::PresentationGone);
}

/// A scene the sink could not show is not what the window shows: a layer read waits for the
/// retry that does.
#[test]
fn a_layer_read_waits_for_a_scene_that_is_shown_not_one_the_surface_withheld() {
    use crate::sink::SubmitVerdict;

    let (ui_runtime, agent) = shown();
    let mut parked = park_layers(&ui_runtime, &agent, InspectParams::new(window(&ui_runtime)));
    let presentation = ui_runtime
        .presentations
        .get(ui_runtime.presentation_id())
        .expect("the primary presentation is open");

    let mut withheld = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);
    frame(&ui_runtime, &mut withheld);
    assert_eq!(withheld.submit_calls, 1, "a scene was built and withheld");
    assert!(
        parked.try_take().is_none(),
        "the window never showed that scene"
    );
    assert_eq!(
        presentation.agent_layer_reads.borrow().len(),
        1,
        "the read is parked again for the next scene"
    );
    assert!(presentation.agent_layer_staged.borrow().is_empty());

    frame(&ui_runtime, &mut ScriptedSink::always_presents());
    parked
        .try_take()
        .expect("the retry showed a scene")
        .expect("the layers are read");
}

/// A reply is delivered to a receiver whose drop or trace is user code: one that panics must not
/// take the replies behind it with it.
#[test]
fn every_reply_is_attempted_and_the_first_panic_is_returned() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::super::agent_inspect::deliver_each;

    let mut seen = Vec::new();
    let panicked = deliver_each(vec![1, 2, 3], |item| {
        seen.push(item);
        assert_ne!(item, 1, "the first receiver panics");
        assert_ne!(item, 2, "and so does the second");
    })
    .expect("a panic was returned");
    assert_eq!(seen, [1, 2, 3], "every reply was attempted");
    let message = catch_unwind(AssertUnwindSafe(|| std::panic::resume_unwind(panicked)))
        .expect_err("it resumes");
    let message = message
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| message.downcast_ref::<&str>().map(ToString::to_string))
        .unwrap_or_default();
    assert!(message.contains("the first receiver panics"), "{message}");
}

/// A tracing subscriber, scoped to the current thread, that panics on the events whose fields
/// mention one of `needles`.
struct PanicsOnEvent(&'static [&'static str]);

impl tracing::Subscriber for PanicsOnEvent {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Mentions<'a>(&'a [&'static str], Option<&'static str>);
        impl tracing::field::Visit for Mentions<'_> {
            fn record_debug(&mut self, _: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                let text = format!("{value:?}");
                self.1 = self
                    .1
                    .or_else(|| self.0.iter().copied().find(|needle| text.contains(needle)));
            }
        }
        let mut mentions = Mentions(self.0, None);
        event.record(&mut mentions);
        if let Some(needle) = mentions.1 {
            panic!("a subscriber that cannot take this trace: {needle}");
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Runs `run` with [`PanicsOnEvent`] as this thread's subscriber. A second dispatcher is held
/// meanwhile: with only one in the process, tracing registers a callsite that another test thread
/// reaches first against that thread's (empty) default, and this subscriber would never see it.
fn with_panicking_subscriber<R>(needles: &'static [&'static str], run: impl FnOnce() -> R) -> R {
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    tracing::subscriber::with_default(PanicsOnEvent(needles), || {
        tracing::callsite::rebuild_interest_cache();
        run()
    })
}

/// Closing a window answers every job parked on it: a reply that panics (here, a dropped
/// receiver whose trace a subscriber refuses) must not leave the layer read behind it unanswered.
#[test]
fn a_reply_that_panics_while_a_window_closes_does_not_strand_its_layer_read() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use flui_protocol::{Condition, Locator, WaitParams};

    let mut ui_runtime = UiRuntime::for_test();
    let second = ui_runtime.install_second_presentation_for_test();
    let agent = ui_runtime
        .semantics_agent(second)
        .expect("the ui_runtime hosts the second presentation");
    let second_window = WindowId::from_u64(second.as_u64()).expect("a generation");
    // A wait nobody listens to any more: answering it traces, and the trace panics.
    drop(
        agent
            .wait(WaitParams::new(
                second_window,
                Condition::Appears(Locator::named("never")),
            ))
            .expect("the inbox has room"),
    );
    let _report = ui_runtime.drain_commands();
    let mut parked = park_layers(&ui_runtime, &agent, InspectParams::new(second_window));
    assert!(parked.try_take().is_none());

    let closed = with_panicking_subscriber(&["nobody waits for"], || {
        catch_unwind(AssertUnwindSafe(|| {
            ui_runtime.close_presentation_entered(second)
        }))
    });
    assert!(closed.is_err(), "the trace's panic continues");
    let error = parked
        .try_take()
        .expect("closing answers the layer read whatever an earlier reply did")
        .expect_err("the window is gone");
    assert_eq!(error, AgentError::PresentationGone);
}

/// A panic after the sink showed a scene (here, the trace that follows the commit) must not strand
/// the layer reads staged for it: they are answered before anything else that can panic.
#[test]
fn a_panic_after_the_scene_is_shown_does_not_strand_its_layer_read() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let (ui_runtime, agent) = shown();
    let mut parked = park_layers(&ui_runtime, &agent, InspectParams::new(window(&ui_runtime)));

    let unwound = with_panicking_subscriber(&["tree_revision_committed"], || {
        catch_unwind(AssertUnwindSafe(|| {
            frame(&ui_runtime, &mut ScriptedSink::always_presents());
        }))
    });
    assert!(unwound.is_err(), "the trace's panic continues");
    parked
        .try_take()
        .expect("the scene was shown, so the read is answered")
        .expect("the layers are read");
}

/// A sink that panics on submit takes the scene with it: the reads that were staged for it are
/// parked again for the next scene, not left staged or dropped.
#[test]
fn a_sink_that_panics_on_submit_leaves_the_layer_reads_parked_for_the_next_scene() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let (ui_runtime, agent) = shown();
    let mut parked = park_layers(&ui_runtime, &agent, InspectParams::new(window(&ui_runtime)));

    let mut panicking = ScriptedSink::new(|_, _| panic!("a sink that cannot submit"));
    let unwound = catch_unwind(AssertUnwindSafe(|| frame(&ui_runtime, &mut panicking)));
    assert!(unwound.is_err(), "the sink's panic continues");

    let presentation = ui_runtime
        .presentations
        .get(ui_runtime.presentation_id())
        .expect("the primary presentation is open");
    assert!(parked.try_take().is_none(), "no scene was shown");
    assert_eq!(presentation.agent_layer_reads.borrow().len(), 1);
    assert!(presentation.agent_layer_staged.borrow().is_empty());
}

/// Two things go wrong after a scene is shown: a layer read's receiver is gone and tracing its
/// reply panics, and then the commit's trace panics too. The first failure is the one that
/// resumes; the second does not replace it.
#[test]
fn the_first_panic_after_a_shown_scene_is_the_one_that_resumes() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let (ui_runtime, agent) = shown();
    // A read nobody waits for any more: answering it traces, and the trace panics.
    drop(park_layers(
        &ui_runtime,
        &agent,
        InspectParams::new(window(&ui_runtime)),
    ));

    let unwound =
        with_panicking_subscriber(&["nobody waits for", "tree_revision_committed"], || {
            catch_unwind(AssertUnwindSafe(|| {
                frame(&ui_runtime, &mut ScriptedSink::always_presents());
            }))
        })
        .expect_err("the traces' panics continue");
    let message = unwound
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| unwound.downcast_ref::<&str>().map(ToString::to_string))
        .unwrap_or_default();
    assert!(
        message.contains("nobody waits for"),
        "the reply's panic came first and is the one resumed: {message}"
    );
}
