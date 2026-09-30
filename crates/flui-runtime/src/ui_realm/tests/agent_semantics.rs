//! A semantics agent reads a presentation's committed tree as ADR-0080 wire
//! nodes and acts on its elements through the owner inbox (ADR-0095 §3):
//! the read is served at a drain, an action reaches the widget's own handler,
//! and every failure answers the agent with its code.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::AtomicU32;

use flui_protocol::{
    ActionName, ActionRequest, ElementId, ErrorCode, ReadQuery, Retry, Tree, outline,
};
use flui_semantics::{
    AccessibilityNodeId, SemanticsAction, SemanticsConfiguration, SemanticsNode, SemanticsOwner,
};
use flui_view::Signal;
use flui_widgets::{Column, RawButton, Text, column};

use super::*;

static_assertions::assert_impl_all!(SemanticsAgent: Clone, Send, Sync);
static_assertions::assert_impl_all!(AgentReply<Tree>: Send);

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

    fn create_state(&self) -> Self::State {
        CounterState {
            count: Signal::default(),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        self.count = ctx.signal(0);
    }

    fn build(&self, view: &Counter, ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        let count = self.count;
        let value = count.get(ctx);
        let presses = Arc::clone(&view.presses);
        let control = if view.hide_at.is_some_and(|at| value >= at) {
            Text::new("Done").into_view().boxed()
        } else {
            RawButton::new(Text::new("Increment"))
                .on_press(move |cx| {
                    presses.fetch_add(1, Ordering::Relaxed);
                    count.update(cx, |n| *n += 1)
                })
                .into_view()
                .boxed()
        };
        Column::new(column![Text::new(format!("Count: {value}")), control])
    }
}

impl flui_view::View for Counter {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

fn counter(realm: &UiRealm, hide_at: Option<u32>) -> Arc<AtomicU32> {
    let presses = Arc::new(AtomicU32::new(0));
    // Inside the realm's entry, as a host attaches: the button's semantics
    // action lives in the realm's interaction lane.
    realm
        .enter(|realm| {
            realm.attach_root_widget_to_for_test(
                realm.presentation_id(),
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
/// runs after its drain, without the drain, inside the realm's entry as a
/// pump runs it.
fn frame(realm: &UiRealm, sink: &mut ScriptedSink) {
    let now = flui_scheduler::Instant::now();
    let _presented = realm.enter(|realm| {
        realm.drive_frame(now, flui_scheduler::IdleDeadline::far_future(now), || {
            realm.render_frame(sink)
        })
    });
}

/// Serves the inbox through the public drain, deliberately without entering
/// the realm first: the drain enters it itself, so an act still reaches a
/// handler that lives in the realm's interaction lane. Takes the answer the
/// drain produced.
fn answer<T>(realm: &UiRealm, mut reply: AgentReply<T>) -> Result<T, AgentError> {
    let _report = realm.drain_commands();
    reply
        .try_take()
        .expect("the drain answers every request it serves")
}

fn read(realm: &UiRealm, agent: &SemanticsAgent) -> Tree {
    let reply = agent.read(ReadQuery::new()).expect("the inbox has room");
    answer(realm, reply).expect("the committed tree reads")
}

fn find(tree: &Tree, name: &str) -> flui_protocol::Node {
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
    realm: &UiRealm,
    render_id: RenderId,
    configure: impl FnOnce(&mut SemanticsConfiguration),
) {
    let mut node = SemanticsNode::new().with_source_render_id(render_id);
    configure(node.config_mut());
    let mut owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = owner.insert(node);
    owner.set_root(Some(root));
    realm
        .pipeline_for_test()
        .with_mut(|pipeline| pipeline.set_semantics_owner(Some(owner)));
}

#[test]
fn an_agent_reads_a_widget_tree_as_wire_nodes_that_round_trip_through_json() {
    let realm = UiRealm::for_test();
    let _presses = counter(&realm, None);
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&realm, &mut sink);

    let tree = read(&realm, &agent);
    let window = realm.presentation_id().as_u64();
    let button = find(&tree, "Increment");
    let root = &tree.roots[0];
    assert_eq!(
        outline(&tree.roots),
        format!(
            "- window [ref={}] [window=w{window}]\n  - label \"Count: 0\" [ref={}]\n  - button \
             \"Increment\" [ref={}] [actions=invoke]\n",
            root.id, root.children[0].id, button.id
        )
    );
    assert_eq!(root.window.map(flui_protocol::WindowId::get), Some(window));
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
    let realm = UiRealm::for_test();
    let presses = counter(&realm, None);
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&realm, &mut sink);
    let button = find(&read(&realm, &agent), "Increment").id;

    let reply = agent
        .act(ActionRequest::new(button, ActionName::Invoke))
        .expect("the inbox has room");
    assert_eq!(answer(&realm, reply), Ok(()));
    assert_eq!(
        presses.load(Ordering::Relaxed),
        0,
        "the detector delivers a semantics tap on the next frame, not in the drain"
    );
    frame(&realm, &mut sink);
    assert_eq!(presses.load(Ordering::Relaxed), 1);
    frame(&realm, &mut sink);

    let tree = read(&realm, &agent);
    find(&tree, "Count: 1");
    assert_eq!(
        find(&tree, "Increment").id,
        button,
        "the button keeps its handle"
    );
}

#[test]
fn a_read_before_the_first_semantics_frame_is_busy_and_that_frame_was_requested() {
    let realm = UiRealm::for_test();
    let _presses = counter(&realm, None);
    let mut sink = ScriptedSink::always_presents();
    frame(&realm, &mut sink);
    let pipeline = realm.pipeline_for_test();
    assert!(
        pipeline.with(|p| p.semantics_owner().is_none()),
        "premise: nothing collects semantics yet"
    );
    let _ = realm.presentations.primary().take_redraw_pending();

    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    assert!(
        realm.presentations.primary().take_redraw_pending(),
        "vending an agent requests the frame that builds its tree"
    );
    realm.presentations.primary().mark_redraw_pending();

    let early = agent.read(ReadQuery::new()).expect("the inbox has room");
    let early = answer(&realm, early);
    assert_eq!(early, Err(AgentError::NoTreeYet));
    let error = early.expect_err("checked above");
    assert_eq!(
        (error.code(), error.retry()),
        (ErrorCode::Busy, Retry::Soon)
    );

    frame(&realm, &mut sink);
    find(&read(&realm, &agent), "Increment");
}

#[test]
fn dropping_the_last_agent_stops_semantics_collection() {
    let realm = UiRealm::for_test();
    let _presses = counter(&realm, None);
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    let clone = agent.clone();
    let mut sink = ScriptedSink::always_presents();
    frame(&realm, &mut sink);
    let pipeline = realm.pipeline_for_test();
    assert!(pipeline.with(|p| p.semantics_owner().is_some()));

    drop(agent);
    realm.request_redraw();
    frame(&realm, &mut sink);
    assert!(
        pipeline.with(|p| p.semantics_owner().is_some()),
        "a live clone keeps collection on"
    );

    let unanswered = clone.read(ReadQuery::new()).expect("the inbox has room");
    drop(clone);
    realm.request_redraw();
    frame(&realm, &mut sink);
    assert!(
        pipeline.with(|p| p.semantics_owner().is_none()),
        "the last clone gone, the next frame stops collecting, though a reply is unanswered"
    );
    drop(unanswered);
}

#[test]
fn an_action_on_a_node_that_left_the_tree_is_gone_not_retargeted() {
    let realm = UiRealm::for_test();
    let presses = counter(&realm, Some(1));
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&realm, &mut sink);
    let button = find(&read(&realm, &agent), "Increment").id;

    let first = agent
        .act(ActionRequest::new(button, ActionName::Invoke))
        .expect("the inbox has room");
    assert_eq!(answer(&realm, first), Ok(()));
    frame(&realm, &mut sink);
    frame(&realm, &mut sink);
    find(&read(&realm, &agent), "Done");

    let again = agent
        .act(ActionRequest::new(button, ActionName::Invoke))
        .expect("the inbox has room");
    let again = answer(&realm, again);
    assert_eq!(
        again,
        Err(AgentError::NodeNotFound {
            element: button,
            issued: true
        })
    );
    assert_eq!(again.map_err(|e| e.code()), Err(ErrorCode::Gone));
    frame(&realm, &mut sink);
    assert_eq!(
        presses.load(Ordering::Relaxed),
        1,
        "nothing else was pressed"
    );

    let never = ElementId::from_u64(u64::MAX).expect("non-zero");
    let unknown = agent
        .act(ActionRequest::new(never, ActionName::Invoke))
        .expect("the inbox has room");
    assert_eq!(
        answer(&realm, unknown).map_err(|e| e.code()),
        Err(ErrorCode::UnknownHandle),
        "a handle no read reported was never issued"
    );

    // A read scoped to either handle answers as an action on it does.
    let scoped_gone = agent
        .read(ReadQuery::new().with_root(button))
        .expect("the inbox has room");
    assert_eq!(
        answer(&realm, scoped_gone).map_err(|e| e.code()),
        Err(ErrorCode::Gone)
    );
    let scoped_unknown = agent
        .read(ReadQuery::new().with_root(never))
        .expect("the inbox has room");
    assert_eq!(
        answer(&realm, scoped_unknown).map_err(|e| e.code()),
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
            std::num::NonZeroU32::new(generation).expect("test generations are non-zero"),
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
    let realm = UiRealm::for_test();
    let render_id = RenderId::new(1);
    let ran = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&ran);
    install_node(&realm, render_id, move |config| {
        config.set_button(true);
        config.set_label("Press");
        config.add_action(
            SemanticsAction::Tap,
            Arc::new(move |_, _| {
                counter.fetch_add(1, Ordering::Relaxed);
            }),
        );
    });
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    let button = find(&read(&realm, &agent), "Press").id;

    let mut act = agent
        .act(ActionRequest::new(button, ActionName::Invoke))
        .expect("the inbox has room");
    let pipeline = realm.pipeline_for_test();
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        pipeline.with_mut(|_| realm.drain_commands())
    }));
    assert!(
        unwound.is_err(),
        "resolving against a borrowed pipeline panics"
    );
    let failed = act
        .try_take()
        .expect("the reply was answered before unwinding");
    assert_eq!(failed, Err(AgentError::ResolvePanicked { element: button }));
    let error = failed.expect_err("checked above");
    assert!(!error.may_have_run());
    assert_eq!(error.code(), ErrorCode::Platform);
    assert_eq!(ran.load(Ordering::Relaxed), 0, "no handler ran");
}

#[test]
fn an_agent_for_a_closed_presentation_answers_gone() {
    let mut realm = UiRealm::for_test();
    let second = realm.install_second_presentation_for_test();
    let agent = realm
        .semantics_agent(second)
        .expect("the realm hosts the second presentation");
    let dev_window = realm
        .dev_agent_window(second)
        .expect("the realm hosts the second presentation");
    let again = realm
        .dev_agent_window(second)
        .expect("the realm hosts the second presentation");
    assert_eq!(dev_window.id(), again.id());
    assert!(dev_window.is_open());
    // A call on another thread that upgraded the window's port and is still
    // enqueueing when the presentation closes.
    let in_flight = {
        let state = realm
            .presentations
            .get(second)
            .expect("the realm hosts the second presentation");
        let slot = state.dev_agent.borrow();
        Arc::clone(&slot.as_ref().expect("a window was vended").agent)
    };
    assert!(realm.close_presentation_entered(second));
    assert!(realm.semantics_agent(second).is_none());
    assert!(realm.dev_agent_window(second).is_none());
    assert!(
        !again.is_open(),
        "the development agent went with its presentation"
    );
    let fault = dev_window
        .read(ReadQuery::new())
        .expect_err("a closed window answers at once");
    assert_eq!(
        (fault.code(), fault.kind()),
        (
            ErrorCode::Gone,
            Some(flui_view::dev_agent::HandleKind::Window)
        )
    );
    assert!(
        !dev_window.is_open(),
        "closed, though the in-flight call still holds the port"
    );
    let element = ElementId::from_u64(1).expect("non-zero");
    let fault = dev_window
        .act(ActionRequest::new(element, ActionName::Invoke))
        .expect_err("an action on a closed window answers at once");
    assert_eq!(fault.code(), ErrorCode::Gone);
    // The in-flight call, past the window's own check, reaches the port only
    // now: the port admits nothing either.
    {
        use flui_view::__runtime::AgentPort as _;
        let read = in_flight
            .read(ReadQuery::new())
            .expect_err("a closed port admits no read");
        let act = in_flight
            .act(ActionRequest::new(element, ActionName::Invoke))
            .expect_err("a closed port admits no action");
        for fault in [read, act] {
            assert_eq!(
                (fault.code(), fault.kind()),
                (
                    ErrorCode::Gone,
                    Some(flui_view::dev_agent::HandleKind::Window)
                )
            );
        }
        let report = realm.drain_commands();
        assert_eq!(
            (report.invoked, report.dropped_stale),
            (0, 0),
            "nothing was enqueued"
        );
    }
    drop(in_flight);

    let reply = agent.read(ReadQuery::new()).expect("the inbox has room");
    let report = realm.drain_commands();
    assert_eq!(report.dropped_stale, 1);
    let mut reply = reply;
    let gone = reply.try_take().expect("answered");
    assert_eq!(gone, Err(AgentError::PresentationGone));
    let error = gone.expect_err("checked above");
    assert_eq!(
        (error.code(), error.handle_kind()),
        (ErrorCode::Gone, Some("window"))
    );
}

#[test]
fn a_full_inbox_answers_busy() {
    let realm = new_runtime_with_capacity(2, Arc::new(|| {})).expect("runtime with a tiny inbox");
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    let _first = agent.read(ReadQuery::new()).expect("first fits");
    let _second = agent.read(ReadQuery::new()).expect("second fits");
    let full = agent.read(ReadQuery::new()).expect_err("the inbox is full");
    assert_eq!(full, AgentError::InboxFull);
    assert_eq!((full.code(), full.retry()), (ErrorCode::Busy, Retry::Soon));
}

/// A handler's panic is contained in the right order: the act reply fails
/// first, the queued read behind it is left for the next drain (and the owner
/// is woken for it), and the handler's own panic is the one that escapes.
#[test]
fn a_panicking_handler_fails_its_act_reply_first_and_the_queued_read_is_served_next_drain() {
    let (wake, wakes) = counting_wake();
    let realm = new_runtime(wake).expect("runtime");
    let render_id = RenderId::new(1);
    install_node(&realm, render_id, |config| {
        config.set_button(true);
        config.set_label("Boom");
        config.add_action(
            SemanticsAction::Tap,
            Arc::new(|_action, _arguments| panic!("handler boom")),
        );
    });
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    let button = find(&read(&realm, &agent), "Boom").id;
    assert_eq!(button, element(render_id));

    let mut act = agent
        .act(ActionRequest::new(button, ActionName::Invoke))
        .expect("the inbox has room");
    let mut queued = agent.read(ReadQuery::new()).expect("the inbox has room");
    let wakes_before = wakes.load(Ordering::Relaxed);

    let unwound = catch_unwind(AssertUnwindSafe(|| realm.drain_commands()))
        .expect_err("the handler's panic escapes the drain");
    assert_eq!(
        unwound.downcast_ref::<&str>().copied(),
        Some("handler boom"),
        "the handler's panic is the one resumed"
    );
    let failed = act
        .try_take()
        .expect("the act reply was answered before unwinding");
    assert_eq!(failed, Err(AgentError::HandlerPanicked { element: button }));
    assert!(failed.expect_err("checked above").may_have_run());
    assert!(
        queued.try_take().is_none(),
        "the read waits for the next drain"
    );
    assert!(
        wakes.load(Ordering::Relaxed) > wakes_before,
        "the queued read got a future owner turn"
    );

    let served = answer(&realm, queued);
    assert_eq!(served.map(|tree| tree.count), Ok(1));
}

#[test]
fn a_dropped_reply_receiver_does_not_fail_the_drain() {
    let realm = UiRealm::for_test();
    let render_id = RenderId::new(1);
    install_node(&realm, render_id, |config| {
        config.set_button(true);
        config.add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
    });
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    drop(agent.read(ReadQuery::new()).expect("the inbox has room"));
    drop(
        agent
            .act(ActionRequest::new(element(render_id), ActionName::Invoke))
            .expect("the inbox has room"),
    );
    let report = realm.drain_commands();
    assert_eq!((report.invoked, report.dropped_stale), (2, 0));
}

/// An answer nobody waits for is traced by element and code alone: a label a
/// read found and a value an action carried stay out of the log.
#[test]
fn agent_traces_carry_no_label_or_value() {
    const LABEL: &str = "secret-label-7f3a";
    const VALUE: &str = "secret-value-91c2";
    let realm = UiRealm::for_test();
    let render_id = RenderId::new(1);
    install_node(&realm, render_id, |config| {
        config.set_text_field(true);
        config.set_label(LABEL);
        config.add_action(SemanticsAction::SetText, Arc::new(|_, _| {}));
    });
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("the realm hosts its primary presentation");
    let target = element(render_id);
    drop(agent.read(ReadQuery::new()).expect("the inbox has room"));
    drop(
        agent
            .act(ActionRequest::set_value(target, VALUE))
            .expect("the inbox has room"),
    );
    drop(
        agent
            .act(ActionRequest::new(target, ActionName::Toggle))
            .expect("the inbox has room"),
    );

    let (_report, log) = flui_testing::log_capture::capture(|| realm.drain_commands());
    assert_eq!(
        log.count_containing("dropping a semantics agent reply nobody waits for"),
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
    assert!(
        !format!(
            "{:?}",
            UiCommand::SemanticsAgentAction {
                presentation_id: realm.presentation_id(),
                request: ActionRequest::set_value(target, VALUE),
                issued: true,
                reply: crossbeam_channel::bounded(1).0,
            }
        )
        .contains(VALUE),
        "a queued action's Debug leaves its value out"
    );
}
