//! An agent's waits (ADR-0238 §7, R18): a condition the owner holds out for without polling,
//! answered at the frame that makes it hold or at its deadline, refused past the window's cap,
//! and answered `gone` when the window closes first.

use std::time::Duration;

use flui_foundation::ManualClock;
use flui_protocol::{
    ActionName, ActionRequest, Condition, ErrorCode, Locator, Retry, UnsettledReason, WaitOutcome,
    WaitParams, WindowId,
};

use super::agent_semantics::{counter, find, frame, read};
use super::pump_transaction::manual_clock_ui_runtime;
use super::*;

static_assertions::assert_impl_all!(AgentReply<WaitOutcome>: Send);

fn window(ui_runtime: &UiRuntime) -> WindowId {
    WindowId::from_u64(ui_runtime.presentation_id().as_u64())
        .expect("a presentation id packs a non-zero generation")
}

/// A counter on a manual clock, one frame in, and the agent that waits on it.
fn shown() -> (UiRuntime, ManualClock, SemanticsAgent, ScriptedSink) {
    let clock = ManualClock::new();
    let ui_runtime = manual_clock_ui_runtime(&clock);
    let _presses = counter(&ui_runtime, None);
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    (ui_runtime, clock, agent, sink)
}

fn wait(
    ui_runtime: &UiRuntime,
    agent: &SemanticsAgent,
    condition: Condition,
    timeout_ms: Option<u64>,
) -> AgentReply<WaitOutcome> {
    let mut params = WaitParams::new(window(ui_runtime), condition);
    params.timeout_ms = timeout_ms;
    agent.wait(params).expect("the inbox has room")
}

fn appears(name: &str) -> Condition {
    Condition::Appears(Locator::named(name))
}

#[test]
fn a_wait_is_satisfied_at_the_frame_its_condition_first_holds() {
    let (ui_runtime, _clock, agent, mut sink) = shown();
    let mut reply = wait(&ui_runtime, &agent, appears("Count: 1"), None);
    let _report = ui_runtime.drain_commands();
    assert!(
        reply.try_take().is_none(),
        "the condition does not hold yet, so the wait is parked"
    );

    let button = find(&read(&ui_runtime, &agent), "Increment").id;
    let tap = agent
        .act(ActionRequest::new(button, ActionName::Invoke))
        .expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    drop(tap);
    assert!(
        reply.try_take().is_none(),
        "the tap changed the app, but the tree the wait sees is the committed one"
    );

    frame(&ui_runtime, &mut sink);
    let outcome = reply
        .try_take()
        .expect("the frame that committed the new count answered")
        .expect("the wait ran");
    assert!(outcome.satisfied);
    assert_eq!(outcome.frames, 1);
    let names: Vec<_> = outcome
        .observation
        .matched
        .iter()
        .filter_map(|node| node.name.as_deref())
        .collect();
    assert_eq!(names, ["Count: 1"]);
}

#[test]
fn a_condition_that_holds_already_answers_at_the_drain() {
    let (ui_runtime, _clock, agent, _sink) = shown();
    let mut reply = wait(&ui_runtime, &agent, appears("Increment"), None);
    let _report = ui_runtime.drain_commands();

    let outcome = reply
        .try_take()
        .expect("nothing to wait for")
        .expect("the wait ran");
    assert!(outcome.satisfied);
    assert_eq!(outcome.frames, 0);
}

#[test]
fn a_frame_count_keeps_frames_coming_and_counts_the_ones_after_the_request() {
    let (ui_runtime, _clock, agent, mut sink) = shown();
    let mut reply = wait(&ui_runtime, &agent, Condition::Frames(2), None);
    let _report = ui_runtime.drain_commands();
    assert!(reply.try_take().is_none());
    assert!(
        ui_runtime.needs_redraw(),
        "an idle window would never present the frames the wait counts"
    );

    frame(&ui_runtime, &mut sink);
    assert!(reply.try_take().is_none(), "one frame of two");
    assert!(
        ui_runtime.needs_redraw(),
        "the wait asks for the next frame itself"
    );
    frame(&ui_runtime, &mut sink);
    let outcome = reply
        .try_take()
        .expect("the second frame answered")
        .expect("the wait ran");
    assert!(outcome.satisfied);
    assert_eq!(outcome.frames, 2);
}

#[test]
fn a_wait_that_runs_out_answers_unsatisfied_at_its_deadline_without_a_frame() {
    let (ui_runtime, clock, agent, _sink) = shown();
    let idle_wake = ui_runtime.next_wake();
    let mut reply = wait(&ui_runtime, &agent, appears("Missing"), Some(50));
    let _report = ui_runtime.drain_commands();
    assert!(reply.try_take().is_none());
    assert!(
        ui_runtime.next_wake() < idle_wake,
        "the deadline is a wake before the idle one, so a window that draws nothing still times out"
    );

    clock.advance(Duration::from_millis(30));
    let _ = ui_runtime.drain_owner_inbox();
    assert!(reply.try_take().is_none(), "20 ms are left");

    clock.advance(Duration::from_millis(30));
    let _ = ui_runtime.drain_owner_inbox();
    let outcome = reply
        .try_take()
        .expect("the wake at the deadline answered")
        .expect("running out is an answer, not an error");
    assert!(!outcome.satisfied);
    assert_eq!(outcome.frames, 0);
    assert_eq!(outcome.observation.count, 0);
    assert_eq!(
        ui_runtime.next_wake(),
        idle_wake,
        "an answered wait leaves no wake behind"
    );
}

#[test]
fn a_timeout_names_why_the_window_had_not_settled() {
    let clock = ManualClock::new();
    let ui_runtime = manual_clock_ui_runtime(&clock);
    let _presses = counter(&ui_runtime, None);
    let agent = ui_runtime
        .semantics_agent(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    // No frame has run: the attach asked for one.
    let mut reply = wait(
        &ui_runtime,
        &agent,
        Condition::All(vec![Condition::Settled, appears("Missing")]),
        Some(0),
    );
    let _report = ui_runtime.drain_commands();

    let outcome = reply
        .try_take()
        .expect("a zero timeout is up at once")
        .expect("running out is an answer");
    assert!(!outcome.satisfied);
    let reasons: Vec<_> = outcome.unsettled.iter().map(|cause| cause.reason).collect();
    assert!(
        reasons.contains(&UnsettledReason::NeedsRedraw),
        "a frame was still wanted: {reasons:?}"
    );
}

#[test]
fn a_parked_wait_is_not_a_reason_the_window_has_not_settled() {
    let (ui_runtime, _clock, agent, _sink) = shown();
    let now = ui_runtime
        .presentations
        .get(ui_runtime.presentation_id())
        .expect("the primary presentation is hosted")
        .clock()
        .now();
    let before = ui_runtime.unsettled_causes(now);

    let _parked = wait(&ui_runtime, &agent, appears("Missing"), Some(10));
    let _report = ui_runtime.drain_commands();

    assert!(
        ui_runtime.next_wake() < ui_runtime.next_wake_excluding_agent_waits(),
        "the parked wait's deadline is the earliest wake"
    );
    assert_eq!(
        ui_runtime.unsettled_causes(now),
        before,
        "what a wait itself holds out for is not why the window is busy"
    );
}

#[test]
fn a_window_admits_a_bounded_number_of_waits_and_refuses_the_next_with_a_typed_error() {
    let (ui_runtime, _clock, agent, _sink) = shown();
    // The inbox holds 256 commands, so the waits go in two waves.
    let mut parked = Vec::new();
    for _ in 0..2 {
        parked.extend((0..128).map(|_| wait(&ui_runtime, &agent, appears("Missing"), None)));
        let _report = ui_runtime.drain_commands();
    }
    let mut refused = wait(&ui_runtime, &agent, appears("Missing"), None);
    let _report = ui_runtime.drain_commands();

    let error = refused
        .try_take()
        .expect("the wait past the cap is answered at once")
        .expect_err("it is refused, not parked or taking an older one's place");
    assert_eq!(error, AgentError::TooManyWaits);
    assert_eq!(error.code(), ErrorCode::Busy);
    assert_eq!(error.retry(), Retry::Soon);
    assert!(
        parked.iter_mut().all(|reply| reply.try_take().is_none()),
        "the waits already admitted are untouched"
    );
}

#[test]
fn a_window_that_closes_first_answers_its_waits_gone() {
    let mut ui_runtime = UiRuntime::for_test();
    let second = ui_runtime.install_second_presentation_for_test();
    let agent = ui_runtime
        .semantics_agent(second)
        .expect("the ui_runtime hosts the second presentation");
    let window =
        WindowId::from_u64(second.as_u64()).expect("a presentation id packs a non-zero generation");
    let mut reply = agent
        .wait(WaitParams::new(window, Condition::Frames(5)))
        .expect("the inbox has room");
    let _report = ui_runtime.drain_commands();
    assert!(reply.try_take().is_none(), "the wait is parked");

    assert!(ui_runtime.close_presentation_entered(second));
    let error = reply
        .try_take()
        .expect("closing answered what it held")
        .expect_err("the window is gone");
    assert_eq!(error, AgentError::PresentationGone);
    assert_eq!(error.code(), ErrorCode::Gone);
}

#[test]
fn a_timer_the_window_waits_on_is_named_with_when_it_is_due() {
    let (ui_runtime, _clock, agent, _sink) = shown();
    // The test window cannot answer the native gesture-geometry query, so the owner keeps
    // its retry as a timer: a window with one is not settled.
    let mut reply = wait(&ui_runtime, &agent, Condition::Settled, Some(0));
    let _report = ui_runtime.drain_commands();

    let outcome = reply
        .try_take()
        .expect("a zero timeout is up at once")
        .expect("running out is an answer");
    assert!(!outcome.satisfied);
    let timer = outcome
        .unsettled
        .iter()
        .find(|cause| cause.reason == UnsettledReason::Timer)
        .unwrap_or_else(|| panic!("a timer is pending: {:?}", outcome.unsettled));
    assert!(
        timer.due_in_ms.is_some(),
        "when it is due is part of the cause: {timer:?}"
    );
}
