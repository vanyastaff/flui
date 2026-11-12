//! A semantics agent reads a presentation's committed tree as ADR-0080 wire
//! nodes and acts on its elements through the owner inbox (ADR-0095 §3):
//! the read is served at a drain, an action reaches the widget's own handler,
//! and every failure answers the agent with its code.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::AtomicU32;

use flui_protocol::method;
use flui_protocol::{
    ActParams, ActionName, EffectKind, ElementId, ElementRef, ErrorCode, ErrorObject, HandleKind,
    ReadParams, ReadQuery, Retry, Tree, outline,
};
use flui_semantics::{
    AccessibilityNodeId, SemanticsAction, SemanticsConfiguration, SemanticsNode, SemanticsOwner,
    SemanticsRole,
};
use flui_view::Signal;
use flui_view::dev_agent::{AgentAnswer, AgentWindow};
use flui_widgets::{Column, RawButton, Text};

use super::*;

/// A count, its text, and a button that increments it. Once `hide_at` is
/// reached the button is replaced by a text, taking its element out of the
/// tree.
#[derive(Clone)]
struct Counter {
    presses: Arc<AtomicU32>,
    hide_at: Option<u32>,
}

struct CounterState {
    count: Signal<u32>,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        CounterState {
            count: ctx.signal(0),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, view: &Counter, ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        let count = self.count;
        let value = count.get(ctx);
        let presses = Arc::clone(&view.presses);
        let control = if view.hide_at.is_some_and(|at| value >= at) {
            Text::new("Done").into_view().boxed()
        } else {
            RawButton::new(Text::new("Increment"))
                .on_pressed(move |ctx| {
                    presses.fetch_add(1, Ordering::Relaxed);
                    count.update(ctx, |n| *n += 1)
                })
                .into_view()
                .boxed()
        };
        Column::new((Text::new(format!("Count: {value}")), control))
    }
}

impl flui_view::View for Counter {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

pub(super) fn counter(ui_runtime: &UiRuntime, hide_at: Option<u32>) -> Arc<AtomicU32> {
    let presses = Arc::new(AtomicU32::new(0));
    // Inside the ui_runtime's entry, as a host attaches: the button's semantics
    // action lives in the ui_runtime's interaction lane.
    ui_runtime
        .enter(|ui_runtime| {
            ui_runtime.attach_root_widget_to_for_test(
                ui_runtime.presentation_id(),
                &Counter {
                    presses: Arc::clone(&presses),
                    hide_at,
                },
            )
        })
        .expect("the counter attaches");
    presses
}

/// One whole frame, post-frame callbacks included: the frame step a pump
/// runs after its drain, without the drain, inside the UI runtime's entry as a
/// pump runs it.
pub(super) fn frame(ui_runtime: &UiRuntime, sink: &mut ScriptedSink) {
    let now = flui_scheduler::Instant::now();
    let _presented = ui_runtime.enter(|ui_runtime| {
        ui_runtime.drive_frame(
            now,
            flui_scheduler::IdleDeadline::far_future(now),
            || {},
            || ui_runtime.render_frame(sink),
        )
    });
}

/// Serves the inbox through the public drain, deliberately without entering
/// the UI runtime first: the drain enters it itself, so an act still reaches a
/// handler that lives in the UI runtime's interaction lane. Takes the answer the
/// drain produced.
#[expect(
    clippy::result_large_err,
    reason = "the protocol's error object is the answer under test"
)]
pub(super) fn answer<T>(
    ui_runtime: &UiRuntime,
    mut reply: AgentAnswer<T>,
) -> Result<T, ErrorObject> {
    let _report = ui_runtime.drain_commands();
    reply
        .try_take()
        .expect("the drain answers every request it serves")
}

/// The parameters of a read of `window` scoped by `query`.
pub(super) fn read_params(window: &AgentWindow, query: ReadQuery) -> ReadParams {
    ReadParams::new(window.id()).query(query)
}

/// The parameters of `action` on the element `element` of `window`.
pub(super) fn act_params(
    window: &AgentWindow,
    element: ElementId,
    action: ActionName,
) -> ActParams {
    ActParams::new(ElementRef::new(window.id(), element), action)
}

pub(super) fn read(ui_runtime: &UiRuntime, window: &AgentWindow) -> Tree {
    let reply = window
        .call::<method::SemanticsRead>(ReadParams::new(window.id()))
        .expect("the inbox has room");
    answer(ui_runtime, reply).expect("the committed tree reads")
}

pub(super) fn find(tree: &Tree, name: &str) -> flui_protocol::Node {
    fn walk(nodes: &[flui_protocol::Node], name: &str) -> Option<flui_protocol::Node> {
        nodes.iter().find_map(|node| {
            (node.name.as_deref() == Some(name))
                .then(|| node.clone())
                .or_else(|| walk(&node.children, name))
        })
    }
    walk(&tree.roots, name).unwrap_or_else(|| panic!("no `{name}` in\n{}", outline(&tree.roots)))
}

fn element(render_id: RenderId) -> ElementId {
    ElementId::from_u64(AccessibilityNodeId::from(render_id).as_u64())
        .expect("an accessibility id is non-zero")
}

/// Installs a hand-built semantics tree of one node on the primary
/// presentation, as an adapter-driven test does.
fn install_node(
    ui_runtime: &UiRuntime,
    render_id: RenderId,
    configure: impl FnOnce(&mut SemanticsConfiguration),
) {
    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    configure(node.config_mut());
    let mut owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = owner.insert(node);
    owner.set_root(Some(root));
    ui_runtime
        .pipeline_for_test()
        .with_mut(|pipeline| pipeline.set_semantics_owner(Some(owner)));
}

#[test]
fn an_agent_reads_a_widget_tree_as_wire_nodes_that_round_trip_through_json() {
    let ui_runtime = UiRuntime::for_test();
    let _presses = counter(&ui_runtime, None);
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);

    let tree = read(&ui_runtime, &window);
    let window_number = ui_runtime.presentation_id().as_u64();
    let button = find(&tree, "Increment");
    let root = &tree.roots[0];
    assert_eq!(
        outline(&tree.roots),
        format!(
            "- window [ref={}] [window=w{window_number}]\n  - label \"Count: 0\" [ref={}]\n  - button \
             \"Increment\" [ref={}] [actions=invoke,focus]\n",
            root.id, root.children[0].id, button.id
        )
    );
    assert_eq!(
        root.window.map(flui_protocol::WindowId::get),
        Some(window_number)
    );
    assert_eq!(button.rect, None, "no screen rect is claimed in process");
    let rect = button.surface_rect.expect("a laid-out button has bounds");
    assert!(
        rect.width > 0 && rect.height > 0 && rect.y > 0,
        "the button's surface rect sits below the count text: {rect:?}"
    );

    let json = serde_json::to_string(&tree).expect("a tree serializes");
    let back: Tree = serde_json::from_str(&json).expect("a tree deserializes");
    assert_eq!(back, tree);
}

#[test]
fn an_agent_tap_reaches_the_widgets_handler_and_writes_its_signal() {
    let ui_runtime = UiRuntime::for_test();
    let presses = counter(&ui_runtime, None);
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    let button = find(&read(&ui_runtime, &window), "Increment").id;

    let reply = window
        .call::<method::SemanticsAct>(act_params(&window, button, ActionName::Invoke))
        .expect("the inbox has room");
    let outcome = answer(&ui_runtime, reply).expect("the tap runs");
    assert_eq!(outcome.effect.kind, EffectKind::Ran);
    assert_eq!(
        presses.load(Ordering::Relaxed),
        1,
        "the detector runs a semantics tap in the drain that answers it"
    );
    frame(&ui_runtime, &mut sink);
    assert_eq!(
        presses.load(Ordering::Relaxed),
        1,
        "no frame replays the tap"
    );

    let tree = read(&ui_runtime, &window);
    find(&tree, "Count: 1");
    assert_eq!(
        find(&tree, "Increment").id,
        button,
        "the button keeps its handle"
    );
}

#[test]
fn a_read_before_the_first_semantics_frame_is_busy_and_that_frame_was_requested() {
    let ui_runtime = UiRuntime::for_test();
    let _presses = counter(&ui_runtime, None);
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    let pipeline = ui_runtime.pipeline_for_test();
    assert!(
        pipeline.with(|p| p.semantics_owner().is_none()),
        "premise: nothing collects semantics yet"
    );
    let _ = ui_runtime.presentations.primary().take_redraw_pending();

    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    assert!(
        ui_runtime.presentations.primary().take_redraw_pending(),
        "vending an agent requests the frame that builds its tree"
    );
    ui_runtime.presentations.primary().mark_redraw_pending();

    let early = window
        .call::<method::SemanticsRead>(ReadParams::new(window.id()))
        .expect("the inbox has room");
    let error = answer(&ui_runtime, early).expect_err("no tree is committed yet");
    assert_eq!((error.code, error.retry), (ErrorCode::Busy, Retry::Soon));
    assert!(
        error.message.contains("no semantics tree"),
        "busy because no tree is committed, not because the inbox is full: {}",
        error.message
    );

    frame(&ui_runtime, &mut sink);
    find(&read(&ui_runtime, &window), "Increment");
}

#[test]
fn dropping_the_last_agent_stops_semantics_collection() {
    let ui_runtime = UiRuntime::for_test();
    let _presses = counter(&ui_runtime, None);
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let clone = window.clone();
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    let pipeline = ui_runtime.pipeline_for_test();
    assert!(pipeline.with(|p| p.semantics_owner().is_some()));

    drop(window);
    ui_runtime.request_redraw();
    frame(&ui_runtime, &mut sink);
    assert!(
        pipeline.with(|p| p.semantics_owner().is_some()),
        "a live clone keeps collection on"
    );

    let unanswered = clone
        .call::<method::SemanticsRead>(ReadParams::new(clone.id()))
        .expect("the inbox has room");
    drop(clone);
    ui_runtime.request_redraw();
    frame(&ui_runtime, &mut sink);
    assert!(
        pipeline.with(|p| p.semantics_owner().is_none()),
        "the last clone gone, the next frame stops collecting, though a reply is unanswered"
    );
    drop(unanswered);
}

#[test]
fn an_action_on_a_node_that_left_the_tree_is_gone_not_retargeted() {
    let ui_runtime = UiRuntime::for_test();
    let presses = counter(&ui_runtime, Some(1));
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    let button = find(&read(&ui_runtime, &window), "Increment").id;

    let first = window
        .call::<method::SemanticsAct>(act_params(&window, button, ActionName::Invoke))
        .expect("the inbox has room");
    let outcome = answer(&ui_runtime, first).expect("the tap runs");
    assert_eq!(outcome.effect.kind, EffectKind::Ran);
    frame(&ui_runtime, &mut sink);
    frame(&ui_runtime, &mut sink);
    find(&read(&ui_runtime, &window), "Done");

    let again = window
        .call::<method::SemanticsAct>(act_params(&window, button, ActionName::Invoke))
        .expect("the inbox has room");
    let again = answer(&ui_runtime, again).expect_err("the button left the tree");
    assert_eq!(
        (again.code, again.kind, again.handle.as_deref()),
        (
            ErrorCode::Gone,
            Some(HandleKind::Element),
            Some(button.to_string().as_str())
        )
    );
    frame(&ui_runtime, &mut sink);
    assert_eq!(
        presses.load(Ordering::Relaxed),
        1,
        "nothing else was pressed"
    );

    let never = ElementId::from_u64(u64::MAX).expect("non-zero");
    let unknown = window
        .call::<method::SemanticsAct>(act_params(&window, never, ActionName::Invoke))
        .expect("the inbox has room");
    assert_eq!(
        answer(&ui_runtime, unknown).map_err(|e| e.code),
        Err(ErrorCode::UnknownHandle),
        "a handle no read reported was never issued"
    );

    // A read scoped to either handle answers as an action on it does.
    let scoped_gone = window
        .call::<method::SemanticsRead>(read_params(&window, ReadQuery::new().with_root(button)))
        .expect("the inbox has room");
    assert_eq!(
        answer(&ui_runtime, scoped_gone).map_err(|e| e.code),
        Err(ErrorCode::Gone)
    );
    let scoped_unknown = window
        .call::<method::SemanticsRead>(read_params(&window, ReadQuery::new().with_root(never)))
        .expect("the inbox has room");
    assert_eq!(
        answer(&ui_runtime, scoped_unknown).map_err(|e| e.code),
        Err(ErrorCode::UnknownHandle)
    );
}

/// The record that tells `gone` from `unknown_handle` knows exactly which
/// generations of a slot this agent's reads reported, and grows with the
/// render slots a presentation uses and the separate runs of generations
/// reported at each, not with every element that came and went.
#[test]
fn the_record_of_issued_handles_is_exact_and_bounded_by_render_slots() {
    let at = |slot: u32, generation: u32| {
        element(RenderId::new_gen(
            slot,
            flui_foundation::generation::Generation::try_from(
                std::num::NonZeroU32::new(generation).expect("test generations are non-zero"),
            )
            .expect("a test generation leaves the reserved bit clear"),
        ))
    };
    let mut issued = super::super::agent::IssuedHandles::default();

    // The first read sees generation 10: 1 to 9 existed, but no read
    // reported them to this agent.
    issued.record(at(3, 10));
    assert!(issued.was_issued(at(3, 10)), "a reported generation");
    assert!(
        !issued.was_issued(at(3, 9)),
        "an older generation no read reported was never issued"
    );
    // A later read sees generation 12: 11 came and went between the reads.
    issued.record(at(3, 12));
    assert!(issued.was_issued(at(3, 10)), "reported, then removed: gone");
    assert!(
        !issued.was_issued(at(3, 11)),
        "a generation between two reads that neither reported"
    );
    assert!(!issued.was_issued(at(3, 13)), "a newer generation");

    // Every generation reported, one read each: one run, one slot.
    for generation in 1..=10_000 {
        issued.record(at(7, generation));
    }
    assert_eq!(issued.slots(), 2);
    assert_eq!(issued.runs(), 3, "two runs at slot 3, one at slot 7");
    assert!(issued.was_issued(at(7, 1)));
    assert!(issued.was_issued(at(7, 10_000)));
    assert!(
        !issued.was_issued(at(7, 10_001)),
        "a newer generation than any read reported was not"
    );
    assert!(!issued.was_issued(at(8, 1)), "nor a slot no read reported");

    // Every other generation reported: the runs a slot keeps are bounded,
    // and past the bound its oldest gaps count as issued.
    for generation in (1..=10_000).step_by(2) {
        issued.record(at(9, generation));
    }
    assert!(issued.runs() <= 3 + super::super::agent::IssuedHandles::MAX_RUNS);
    assert!(issued.was_issued(at(9, 9_999)));
    assert!(!issued.was_issued(at(9, 9_998)), "a recent gap stays exact");
    assert!(
        issued.was_issued(at(9, 2)),
        "an old gap past the bound counts as issued"
    );
    assert!(!issued.was_issued(at(9, 10_000)));
}

/// A panic before the handler is reached (here, the pipeline already borrowed
/// when the owner resolves the action) is not reported as the handler's: no
/// part of the action ran.
#[test]
fn a_panic_while_resolving_is_not_reported_as_the_handlers() {
    let ui_runtime = UiRuntime::for_test();
    let render_id = RenderId::new(1);
    let ran = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&ran);
    install_node(&ui_runtime, render_id, move |config| {
        config.set_role(SemanticsRole::Button);
        config.set_label("Press");
        config.add_action(
            SemanticsAction::Click,
            std::rc::Rc::new(move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
            }),
        );
    });
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let button = find(&read(&ui_runtime, &window), "Press").id;

    let mut act = window
        .call::<method::SemanticsAct>(act_params(&window, button, ActionName::Invoke))
        .expect("the inbox has room");
    let pipeline = ui_runtime.pipeline_for_test();
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        pipeline.with_mut(|_| ui_runtime.drain_commands())
    }));
    assert!(
        unwound.is_err(),
        "resolving against a borrowed pipeline panics"
    );
    let failed = act
        .try_take()
        .expect("the reply was answered before unwinding");
    let error = failed.expect_err("the panic is the answer");
    assert!(
        error.message.contains("resolving the action"),
        "the failure is the resolving's, not the handler's: {}",
        error.message
    );
    assert_eq!(error.effect, None, "no part of the action ran");
    assert_eq!(error.code, ErrorCode::Platform);
    assert_eq!(ran.load(Ordering::Relaxed), 0, "no handler ran");
}

#[test]
fn an_agent_for_a_closed_presentation_answers_gone() {
    let mut ui_runtime = UiRuntime::for_test();
    let second = ui_runtime.install_second_presentation_for_test();
    let window = ui_runtime
        .dev_agent_window(second)
        .expect("the ui_runtime hosts the second presentation");
    let again = ui_runtime
        .dev_agent_window(second)
        .expect("the ui_runtime hosts the second presentation");
    assert_eq!(window.id(), again.id());
    assert!(window.is_open());
    // A request admitted before the presentation closes, still in the inbox.
    let mut queued = window
        .call::<method::SemanticsRead>(ReadParams::new(window.id()))
        .expect("the inbox has room");
    assert!(ui_runtime.close_presentation_entered(second));
    assert!(ui_runtime.dev_agent_window(second).is_none());
    assert!(
        !again.is_open(),
        "the development agent went with its presentation"
    );
    let fault = window
        .call::<method::SemanticsRead>(ReadParams::new(window.id()))
        .expect_err("a closed window answers at once");
    assert_eq!(
        (fault.code, fault.kind),
        (ErrorCode::Gone, Some(HandleKind::Window))
    );
    assert!(!window.is_open());
    let element = ElementId::from_u64(1).expect("non-zero");
    let fault = window
        .call::<method::SemanticsAct>(act_params(&window, element, ActionName::Invoke))
        .expect_err("an action on a closed window answers at once");
    assert_eq!(fault.code, ErrorCode::Gone);

    let report = ui_runtime.drain_commands();
    assert_eq!(
        (report.invoked, report.dropped_stale),
        (0, 1),
        "only the request admitted before the close is in the inbox"
    );
    let gone = queued.try_take().expect("answered");
    let error = gone.expect_err("the window closed before the owner served it");
    assert_eq!(
        (error.code, error.kind, error.handle.as_deref()),
        (
            ErrorCode::Gone,
            Some(HandleKind::Window),
            Some(window.id().to_string().as_str())
        )
    );
}

#[test]
fn a_full_inbox_answers_busy() {
    let ui_runtime =
        new_runtime_with_capacity(2, Arc::new(|| {})).expect("runtime with a tiny inbox");
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let _first = window
        .call::<method::SemanticsRead>(ReadParams::new(window.id()))
        .expect("first fits");
    let _second = window
        .call::<method::SemanticsRead>(ReadParams::new(window.id()))
        .expect("second fits");
    let full = window
        .call::<method::SemanticsRead>(ReadParams::new(window.id()))
        .expect_err("the inbox is full");
    assert_eq!((full.code, full.retry), (ErrorCode::Busy, Retry::Soon));
    assert!(
        full.message.contains("inbox is full"),
        "busy because the inbox is full: {}",
        full.message
    );
}

/// A handler's panic is contained in the right order: the act reply fails
/// first, the queued read behind it is left for the next drain (and the owner
/// is woken for it), and the handler's own panic is the one that escapes.
#[test]
fn a_panicking_handler_fails_its_act_reply_first_and_the_queued_read_is_served_next_drain() {
    let (wake, wakes) = counting_wake();
    let ui_runtime = new_runtime(wake).expect("runtime");
    let render_id = RenderId::new(1);
    install_node(&ui_runtime, render_id, |config| {
        config.set_role(SemanticsRole::Button);
        config.set_label("Boom");
        config.add_action(
            SemanticsAction::Click,
            std::rc::Rc::new(|_call| panic!("handler boom")),
        );
    });
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let button = find(&read(&ui_runtime, &window), "Boom").id;
    assert_eq!(button, element(render_id));

    let mut act = window
        .call::<method::SemanticsAct>(act_params(&window, button, ActionName::Invoke))
        .expect("the inbox has room");
    let mut queued = window
        .call::<method::SemanticsRead>(ReadParams::new(window.id()))
        .expect("the inbox has room");
    let wakes_before = wakes.load(Ordering::Relaxed);

    let unwound = catch_unwind(AssertUnwindSafe(|| ui_runtime.drain_commands()))
        .expect_err("the handler's panic escapes the drain");
    assert_eq!(
        unwound.downcast_ref::<&str>().copied(),
        Some("handler boom"),
        "the handler's panic is the one resumed"
    );
    let failed = act
        .try_take()
        .expect("the act reply was answered before unwinding");
    let error = failed.expect_err("the handler's panic is the answer");
    assert_eq!(error.code, ErrorCode::Platform);
    assert_eq!(
        error.effect.map(|effect| effect.kind),
        Some(EffectKind::MayHaveRun),
        "the handler may have run in part"
    );
    assert!(
        queued.try_take().is_none(),
        "the read waits for the next drain"
    );
    assert!(
        wakes.load(Ordering::Relaxed) > wakes_before,
        "the queued read got a future owner turn"
    );

    let served = answer(&ui_runtime, queued);
    assert_eq!(served.map(|tree| tree.count), Ok(1));
}

#[test]
fn a_dropped_reply_receiver_does_not_fail_the_drain() {
    let ui_runtime = UiRuntime::for_test();
    let render_id = RenderId::new(1);
    install_node(&ui_runtime, render_id, |config| {
        config.set_role(SemanticsRole::Button);
        config.add_action(SemanticsAction::Click, std::rc::Rc::new(|_| {}));
    });
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    drop(
        window
            .call::<method::SemanticsRead>(ReadParams::new(window.id()))
            .expect("the inbox has room"),
    );
    drop(
        window
            .call::<method::SemanticsAct>(act_params(
                &window,
                element(render_id),
                ActionName::Invoke,
            ))
            .expect("the inbox has room"),
    );
    let report = ui_runtime.drain_commands();
    assert_eq!((report.invoked, report.dropped_stale), (2, 0));
}

/// An answer nobody waits for is traced by element and code alone: a label a
/// read found and a value an action carried stay out of the log.
#[test]
fn agent_traces_carry_no_label_or_value() {
    const LABEL: &str = "secret-label-7f3a";
    const VALUE: &str = "secret-value-91c2";
    let ui_runtime = UiRuntime::for_test();
    let render_id = RenderId::new(1);
    install_node(&ui_runtime, render_id, |config| {
        config.set_role(SemanticsRole::TextInput);
        config.set_label(LABEL);
        config.add_action(SemanticsAction::SetValue, std::rc::Rc::new(|_| {}));
    });
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let target = element(render_id);
    drop(
        window
            .call::<method::SemanticsRead>(ReadParams::new(window.id()))
            .expect("the inbox has room"),
    );
    drop(
        window
            .call::<method::SemanticsAct>(ActParams::set_value(
                ElementRef::new(window.id(), target),
                VALUE,
            ))
            .expect("the inbox has room"),
    );
    drop(
        window
            .call::<method::SemanticsAct>(act_params(&window, target, ActionName::Toggle))
            .expect("the inbox has room"),
    );

    let (_report, log) = flui_testing::log_capture::capture(|| ui_runtime.drain_commands());
    assert_eq!(
        log.count_containing("dropping a development agent reply nobody waits for"),
        3,
        "vacuous-pass guard: every dropped answer is traced\n{log}"
    );
    assert!(
        log.contains("action_unsupported"),
        "the code is traced\n{log}"
    );
    assert!(
        log.contains(&target.get().to_string()),
        "the element is traced\n{log}"
    );
    assert!(!log.contains(LABEL), "a label reached the log\n{log}");
    assert!(!log.contains(VALUE), "a value reached the log\n{log}");
}
