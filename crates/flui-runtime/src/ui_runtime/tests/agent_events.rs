//! An agent's event streams (ADR-0238 §2, R19): what a window emits to its subscribers and to the
//! waits parked on it, the bound on each subscriber's queue, the cap on subscriptions, and the end
//! of every stream when the window closes.

use flui_foundation::ManualClock;
use flui_protocol::{
    ActionName, ActionRequest, Condition, ErrorCode, Event, EventFilter, EventKind, Retry,
    WaitOutcome, WaitParams, WindowId,
};

use super::agent_semantics::{counter, find, frame, read};
use super::pump_transaction::manual_clock_ui_runtime;
use super::*;

static_assertions::assert_impl_all!(AgentReply<EventStream>: Send);
static_assertions::assert_impl_all!(EventStream: Send);

fn window(ui_runtime: &UiRuntime) -> WindowId {
    WindowId::from_u64(ui_runtime.presentation_id().as_u64())
        .expect("a presentation id packs a non-zero generation")
}

/// A counter on a manual clock, one frame in, and the agent that listens to it.
fn shown() -> (UiRuntime, SemanticsAgent, ScriptedSink) {
    let clock = ManualClock::new();
    let ui_runtime = manual_clock_ui_runtime(&clock);
    let _presses = counter(&ui_runtime, None);
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    (ui_runtime, agent, sink)
}

fn subscribe(
    ui_runtime: &UiRuntime,
    agent: &SemanticsAgent,
    kinds: &[EventKind],
) -> Result<EventStream, AgentError> {
    let filters = kinds.iter().copied().map(EventFilter::new).collect();
    let mut reply = agent.subscribe(filters).expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    reply.try_take().expect("the owner answered at the drain")
}

/// Taps the counter's button, so the next frame publishes a new tree.
fn tap(ui_runtime: &UiRuntime, agent: &SemanticsAgent) {
    let button = find(&read(ui_runtime, agent), "Increment").id;
    let tap = agent
        .act(ActionRequest::new(button, ActionName::Invoke))
        .expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    drop(tap);
}

fn drain(stream: &mut EventStream) -> Vec<Event> {
    std::iter::from_fn(|| stream.try_next()).collect()
}

fn kinds(events: &[Event]) -> Vec<EventKind> {
    events.iter().map(Event::kind).collect()
}

#[test]
fn a_subscriber_hears_a_frame_then_the_tree_it_published() {
    let (ui_runtime, agent, mut sink) = shown();
    let mut stream = subscribe(
        &ui_runtime,
        &agent,
        &[EventKind::Frame, EventKind::Semantics],
    )
    .expect("the window admits it");
    assert!(
        drain(&mut stream).is_empty(),
        "what happened before the subscription is not replayed"
    );

    tap(&ui_runtime, &agent);
    frame(&ui_runtime, &mut sink);
    let events = drain(&mut stream);
    assert_eq!(kinds(&events), [EventKind::Frame, EventKind::Semantics]);
    let Event::Frame(frame_event) = &events[0] else {
        panic!("a frame event first: {events:?}");
    };
    assert_eq!(frame_event.window, window(&ui_runtime));
    assert!(frame_event.presented);
    let Event::Semantics(published) = &events[1] else {
        panic!("a semantics event second: {events:?}");
    };
    assert_eq!(published.window, window(&ui_runtime));

    frame(&ui_runtime, &mut sink);
    assert!(
        drain(&mut stream).is_empty(),
        "a window with nothing to draw presents no frame, so it emits none"
    );
}

#[test]
fn a_filter_selects_the_kinds_and_the_window_it_names() {
    let (ui_runtime, agent, mut sink) = shown();
    let mut trees =
        subscribe(&ui_runtime, &agent, &[EventKind::Semantics]).expect("the window admits it");
    let mut other = {
        let mut filter = EventFilter::new(EventKind::Frame);
        filter.window =
            Some(WindowId::from_u64(window(&ui_runtime).get() + 1).expect("another window number"));
        let mut reply = agent.subscribe(vec![filter]).expect("the inbox has room");
        let _report = ui_runtime.drain_commands();
        reply
            .try_take()
            .expect("the owner answered")
            .expect("the window admits it")
    };

    tap(&ui_runtime, &agent);
    frame(&ui_runtime, &mut sink);
    assert_eq!(kinds(&drain(&mut trees)), [EventKind::Semantics]);
    assert!(
        drain(&mut other).is_empty(),
        "another window's frames do not reach this window's stream"
    );
}

#[test]
fn a_subscription_names_at_least_one_event() {
    let (ui_runtime, agent, _sink) = shown();
    let error = subscribe(&ui_runtime, &agent, &[]).expect_err("nothing to stream");
    assert_eq!(error.code(), ErrorCode::InvalidArgument);
}

#[test]
fn a_reader_that_falls_behind_is_told_how_many_events_it_missed() {
    let (ui_runtime, agent, mut sink) = shown();
    let mut stream =
        subscribe(&ui_runtime, &agent, &[EventKind::Frame]).expect("the window admits it");
    for _ in 0..300 {
        tap(&ui_runtime, &agent);
        frame(&ui_runtime, &mut sink);
    }

    let queued = drain(&mut stream);
    assert_eq!(queued.len(), 256, "the queue holds 256 events");
    assert!(queued.iter().all(|event| event.kind() == EventKind::Frame));

    tap(&ui_runtime, &agent);
    frame(&ui_runtime, &mut sink);
    let next = drain(&mut stream);
    assert_eq!(
        kinds(&next),
        [EventKind::Gap, EventKind::Frame],
        "the next event that fits comes after the gap"
    );
    let Event::Gap(gap) = &next[0] else {
        panic!("a gap first: {next:?}");
    };
    assert_eq!(gap.missed, 44, "300 frames, 256 of them queued");
    let Event::Frame(last) = &next[1] else {
        panic!("a frame second: {next:?}");
    };
    assert_eq!(
        last.frame,
        301 + 1,
        "the first frame, 300 more, and this one"
    );
}

#[test]
fn a_window_admits_a_bounded_number_of_subscriptions_and_a_dropped_stream_frees_its_place() {
    let (ui_runtime, agent, _sink) = shown();
    let mut streams: Vec<_> = (0..64)
        .map(|_| subscribe(&ui_runtime, &agent, &[EventKind::Frame]).expect("within the cap"))
        .collect();

    let error =
        subscribe(&ui_runtime, &agent, &[EventKind::Frame]).expect_err("the 65th is past the cap");
    assert_eq!(error, AgentError::TooManySubscriptions);
    assert_eq!(error.code(), ErrorCode::Busy);
    assert_eq!(error.retry(), Retry::Soon);

    drop(streams.pop());
    let again = subscribe(&ui_runtime, &agent, &[EventKind::Frame]);
    assert!(again.is_ok(), "a dropped stream frees its place: {again:?}");
}

#[test]
fn a_closing_window_ends_every_stream_after_a_last_window_event() {
    let mut ui_runtime = UiRuntime::for_test();
    let second = ui_runtime.install_second_presentation_for_test();
    let agent = ui_runtime
        .semantics_agent(second)
        .expect("the ui_runtime hosts the second presentation");
    let mut reply = agent
        .subscribe(vec![EventFilter::new(EventKind::Window)])
        .expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    let mut stream = reply
        .try_take()
        .expect("the owner answered at the drain")
        .expect("the window admits it");
    assert!(!stream.is_finished());

    assert!(ui_runtime.close_presentation_entered(second));
    let events = drain(&mut stream);
    assert_eq!(kinds(&events), [EventKind::Window]);
    let Event::Window(closed) = &events[0] else {
        panic!("a window event: {events:?}");
    };
    assert!(!closed.opened);
    assert!(stream.is_finished(), "nothing follows the close");
}

#[test]
fn a_wait_on_an_event_is_satisfied_by_the_next_one_the_window_emits() {
    let (ui_runtime, agent, mut sink) = shown();
    let wait = |condition: Condition| {
        let mut params = WaitParams::new(window(&ui_runtime), condition);
        params.timeout_ms = Some(60_000);
        let reply = agent.wait(params).expect("the inbox has room");
        let _report = ui_runtime.drain_commands();
        reply
    };
    let mut on_tree = wait(Condition::Event(EventFilter::new(EventKind::Semantics)));
    let mut on_close = wait(Condition::Event(EventFilter::new(EventKind::Window)));
    assert!(
        on_tree.try_take().is_none(),
        "the tree the first frame published came before the wait"
    );

    frame(&ui_runtime, &mut sink);
    assert!(
        on_tree.try_take().is_none(),
        "a frame that changed nothing publishes no revision"
    );

    tap(&ui_runtime, &agent);
    frame(&ui_runtime, &mut sink);
    let outcome: WaitOutcome = on_tree
        .try_take()
        .expect("the frame that published the new tree answered")
        .expect("the wait ran");
    assert!(outcome.satisfied);
    assert!(
        on_close.try_take().is_none(),
        "the window has not closed, so the wait on it holds out"
    );
}

#[test]
fn a_reader_that_fell_behind_still_sees_the_window_close() {
    let mut ui_runtime = UiRuntime::for_test();
    let second = ui_runtime.install_second_presentation_for_test();
    let agent = ui_runtime
        .semantics_agent(second)
        .expect("the ui_runtime hosts the second presentation");
    let mut stream = subscribe_to(&ui_runtime, &agent, &[EventKind::Frame, EventKind::Window]);
    let state = ui_runtime
        .presentations
        .get(second)
        .expect("the second presentation is open");
    let second_window = WindowId::from_u64(second.as_u64()).expect("a generation");
    for number in 1..=300 {
        state.emit_agent_event(Event::Frame(flui_protocol::FrameEvent::new(
            second_window,
            number,
            true,
        )));
    }

    assert!(ui_runtime.close_presentation_entered(second));
    let events = drain(&mut stream);
    let tail = &events[events.len() - 2..];
    assert!(
        matches!(tail, [Event::Gap(_), Event::Window(closed)] if !closed.opened),
        "the gap, then the close: {tail:?}"
    );
    assert!(stream.is_finished());
}

#[test]
fn a_window_that_has_published_no_tree_reports_none() {
    let clock = ManualClock::new();
    let ui_runtime = manual_clock_ui_runtime(&clock);
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut stream = subscribe_to(&ui_runtime, &agent, &[EventKind::Semantics]);

    frame(&ui_runtime, &mut ScriptedSink::always_presents());
    assert!(
        drain(&mut stream).is_empty(),
        "an owner with no root has published nothing"
    );
}

#[test]
fn a_subscription_to_semantics_keeps_the_window_collecting_them() {
    let (ui_runtime, agent, mut sink) = shown();
    let stream = subscribe_to(&ui_runtime, &agent, &[EventKind::Semantics]);
    drop(agent);
    let collecting = || {
        ui_runtime
            .presentations
            .get(ui_runtime.presentation_id())
            .expect("the primary presentation is open")
            .pipeline()
            .with(flui_rendering::PipelineOwner::semantics_enabled)
    };

    frame(&ui_runtime, &mut sink);
    assert!(collecting(), "the stream holds the window collecting");
    drop(stream);
    // The hub forgets a dropped stream when it next publishes, and the window stops collecting
    // at the frame after.
    ui_runtime.publish_agent_events();
    frame(&ui_runtime, &mut sink);
    assert!(!collecting(), "a dropped stream no longer holds it");
}

#[test]
fn a_frame_that_unwound_in_a_callback_is_still_reported() {
    let (ui_runtime, agent, mut sink) = shown();
    let mut stream = subscribe_to(&ui_runtime, &agent, &[EventKind::Frame]);
    tap(&ui_runtime, &agent);
    ui_runtime
        .scheduler()
        .add_post_frame_callback(Box::new(|_| panic!("a post-frame callback panics")));

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        frame(&ui_runtime, &mut sink);
    }));
    assert!(unwound.is_err(), "the callback's panic leaves the frame");
    let events = drain(&mut stream);
    assert_eq!(
        kinds(&events),
        [EventKind::Frame],
        "the frame was presented"
    );
}

fn subscribe_to(
    ui_runtime: &UiRuntime,
    agent: &SemanticsAgent,
    kinds: &[EventKind],
) -> EventStream {
    subscribe(ui_runtime, agent, kinds).expect("the window admits it")
}

#[test]
fn a_frame_that_unwound_before_the_pipeline_counts_for_no_wait() {
    let (ui_runtime, agent, mut sink) = shown();
    let mut params = WaitParams::new(window(&ui_runtime), Condition::Frames(1));
    params.timeout_ms = Some(60_000);
    let mut parked = agent.wait(params).expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    assert!(parked.try_take().is_none(), "no frame has come since");
    ui_runtime
        .scheduler()
        .schedule_frame_callback(Box::new(|_| panic!("a transient callback panics")));

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        frame(&ui_runtime, &mut sink);
    }));
    assert!(unwound.is_err(), "the callback's panic leaves the frame");
    assert!(
        parked.try_take().is_none(),
        "no frame was rendered, so the wait stays parked"
    );
}
