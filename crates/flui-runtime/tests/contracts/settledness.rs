//! Settledness has one owner, the host that runs the turns (ADR-0400 §3): the runtime answers
//! a parked wait from the list its host hands it after a turn, and from its own half alone, never
//! idle, where its host composes none.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use flui_platform::PlatformWindow;
use flui_runtime::presentation::PresentationWindow;
use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock};
use flui_protocol::method::{WaitFrames, WaitSettled, WaitUntil};
use flui_protocol::wait::{
    ClearedBy, Condition, FramesParams, SettleScope, SettledParams, UnsettledReason, WaitEnd,
    WaitOutcome, WaitParams,
};
use flui_protocol::{locator::Locator, tree::WindowId};
use flui_runtime::dev_agent::{AgentWindowSource, DevAgentHost};
use flui_runtime::pump::SampledClock;
use flui_runtime::settledness::{HostHalf, TURNS_AT_ONE_INSTANT, Unsettled};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_testing::host::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook};
use parking_lot::Mutex;

/// A sink that shows every frame.
struct Shown;

impl FrameSink for Shown {
    fn surface_size(&mut self) -> (u32, u32) {
        (40, 40)
    }

    fn submit(
        &mut self,
        _epoch: flui_foundation::FrameEpoch,
        _: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        SubmitVerdict::Presented
    }
}

/// A hook that serves and keeps nothing: the host vends the window itself.
struct Serving;

impl DevAgentHook for Serving {
    fn attach(&mut self) -> bool {
        true
    }

    fn window_opened(&mut self, _window: AgentWindow) {}
}

/// Keeps every window the host hands it.
struct Windows(Arc<Mutex<Vec<AgentWindow>>>);

impl DevAgentHook for Windows {
    fn attach(&mut self) -> bool {
        true
    }

    fn window_opened(&mut self, window: AgentWindow) {
        self.0.lock().push(window);
    }
}

/// What a host keeps while its runtime runs: the agent's hooks, and the window, which a
/// presentation holds only weakly.
struct Held(
    #[expect(dead_code, reason = "kept alive, never read")] DevAgentHost,
    #[expect(dead_code, reason = "kept alive, never read")] Rc<dyn PlatformWindow>,
);

/// An idle runtime on `clock`, built as a host that composes its half (`composes`) or not, run
/// until its half lists nothing, and the agent window of its presentation.
fn idle_runtime(clock: &ManualClock, composes: bool) -> (UiRuntime, Held, AgentWindow) {
    let fonts = flui_painting::FontCollection::new();
    let mut services = RuntimeHostServices::new(
        Arc::new(|| {}),
        Arc::new(AtomicBool::new(false)),
        &fonts,
        flui_scheduler::ClockSource::Manual(clock.clone()),
    );
    if composes {
        services = services.composes_host_half();
    }
    // The headless host's window answers what a runtime asks of its window (its gesture
    // geometry among it), so a still root leaves nothing armed.
    let platform_window: Rc<dyn PlatformWindow> = Rc::new(HeadlessWindow::new(40, 40));
    let window = PresentationWindow::new(Rc::clone(&platform_window), None)
        .with_gesture_profiles(flui_interaction::gesture::GestureSettings::default());
    let mut runtime = UiRuntime::try_new(window, 1.0, services).expect("a manual-clock runtime");
    runtime
        .attach_root_widget(&flui_widgets::SizedBox::square(10.0))
        .expect("the root mounts");
    let agent = DevAgentHost::attach(AgentWindowSource::dev_agent(), [Box::new(Serving) as _]);
    let window = agent
        .vend(&runtime, runtime.presentation_id())
        .expect("a serving hook is vended the window");
    // Run turns at each wake until nothing is listed, as a host's loop does.
    let mut half = Unsettled::new(clock.now());
    for _ in 0..16 {
        let next = half.wake().map_or(Duration::from_millis(16), |wake| {
            wake.saturating_duration_since(clock.now())
        });
        clock.advance(next);
        let _ = runtime.pump(&mut SampledClock(clock.now()), &mut Shown);
        half = Unsettled::new(clock.now());
        runtime.unsettled(clock.now(), &mut half);
        if half.is_empty() {
            break;
        }
    }
    assert!(
        half.is_empty(),
        "a still root leaves the runtime idle within a few frames: {:?}",
        half.causes().collect::<Vec<_>>()
    );
    (runtime, Held(agent, platform_window), window)
}

fn wait_until(
    window: &AgentWindow,
    condition: Condition,
    timeout_ms: u64,
) -> AgentAnswer<WaitOutcome> {
    window
        .call::<WaitUntil>(WaitParams::new(window.id(), condition).timeout_ms(timeout_ms))
        .expect("the inbox has room")
}

/// `wait.settled` on `window`, holding out at most `timeout_ms`; the protocol gives the
/// parameters no constructor, so they are read as a client writes them.
fn settled_params(window: WindowId, timeout_ms: u64) -> SettledParams {
    serde_json::from_value(serde_json::json!({ "window": window, "timeout_ms": timeout_ms }))
        .expect("the parameters a client writes")
}

/// `wait.frames` on `window` for `frames` frames, holding out at most `timeout_ms`.
fn frames_params(window: WindowId, frames: u32, timeout_ms: u64) -> FramesParams {
    serde_json::from_value(serde_json::json!({
        "window": window,
        "frames": frames,
        "timeout_ms": timeout_ms,
    }))
    .expect("the parameters a client writes")
}

fn missing() -> Condition {
    Condition::Appears(Locator::named("Missing"))
}

/// A false wait on an idle application: a host that composes its half and lists nothing but
/// what the test discharges ends it idle at once; the same state answered from the runtime's half
/// alone (`Absent`), or with a cause something outside answers, keeps it parked to its deadline,
/// where it ends unsatisfied with the scope it counted.
///
/// Fails if an `Absent` half or an `Outside` cause counted toward the idle end: either wait ends
/// idle at the first answer.
#[test]
fn only_a_complete_half_of_test_causes_ends_a_wait_idle() {
    let clock = ManualClock::new();
    let (mut runtime, _agent, window) = idle_runtime(&clock, true);
    let now = clock.now();
    let mut held = Unsettled::new(now);
    held.test(UnsettledReason::Background, None);
    let mut outside = Unsettled::new(now);
    outside.outside(UnsettledReason::Background, None);

    struct Row {
        name: &'static str,
        half: Option<Unsettled>,
        idle: bool,
    }
    let rows = [
        Row {
            name: "complete, nothing listed",
            half: Some(Unsettled::new(now)),
            idle: true,
        },
        Row {
            name: "complete, held work",
            half: Some(held),
            idle: true,
        },
        Row {
            name: "complete, a reply outside",
            half: Some(outside),
            idle: false,
        },
        Row {
            name: "absent",
            half: None,
            idle: false,
        },
    ];
    for row in rows {
        let mut reply = wait_until(&window, missing(), 50);
        let _ = runtime.drain_owner_inbox();
        assert!(
            reply.try_take().is_none(),
            "{}: a composing host's turn answers nothing",
            row.name
        );
        let answer = |runtime: &mut UiRuntime| match &row.half {
            Some(half) => runtime.answer_settled(clock.now(), HostHalf::Complete(half)),
            None => runtime.answer_settled(clock.now(), HostHalf::Absent),
        };
        answer(&mut runtime);
        let scope = if row.half.is_some() {
            SettleScope::Complete
        } else {
            SettleScope::RuntimeOnly
        };
        if row.idle {
            let outcome = reply
                .try_take()
                .expect("an idle end answers at once")
                .expect("an answer");
            assert_eq!(outcome.end, WaitEnd::Idle, "{}", row.name);
            assert_eq!(outcome.scope, scope, "{}", row.name);
            continue;
        }
        assert!(
            reply.try_take().is_none(),
            "{}: never idle, the wait holds out",
            row.name
        );
        clock.advance(Duration::from_millis(50));
        answer(&mut runtime);
        let outcome = reply
            .try_take()
            .expect("its deadline answers")
            .expect("an answer");
        assert_eq!(outcome.end, WaitEnd::Unsatisfied, "{}", row.name);
        assert_eq!(outcome.scope, scope, "{}", row.name);
    }
}

/// A runtime whose host composes no half answers its settled waits itself at the end of each
/// turn, from its own half: never idle, and saying so.
///
/// Fails against a runtime that answers nothing where its host composes no half (the wait is
/// never answered), or that answers it with the host's scope.
#[test]
fn a_runtime_whose_host_composes_no_half_answers_runtime_only_and_never_idle() {
    let clock = ManualClock::new();
    let (runtime, _agent, window) = idle_runtime(&clock, false);
    let mut settled = window
        .call::<WaitSettled>(settled_params(window.id(), 1_000))
        .expect("the inbox has room");
    let mut false_wait = wait_until(&window, missing(), 30);
    let _ = runtime.drain_owner_inbox();

    let outcome = settled
        .try_take()
        .expect("its turn answered it")
        .expect("an answer");
    assert_eq!(outcome.end, WaitEnd::Satisfied);
    assert_eq!(outcome.scope, SettleScope::RuntimeOnly);
    assert!(
        false_wait.try_take().is_none(),
        "an idle runtime alone is no idle end"
    );

    clock.advance(Duration::from_millis(30));
    let _ = runtime.drain_owner_inbox();
    let outcome = false_wait
        .try_take()
        .expect("its deadline answered")
        .expect("an answer");
    assert_eq!(outcome.end, WaitEnd::Unsatisfied);
    assert_eq!(outcome.scope, SettleScope::RuntimeOnly);
}

/// A timer the host lists as due at the instant it is served, after every turn, keeps the clock
/// from moving: the wait ends unsatisfied after the turn bound, naming the timer.
///
/// Fails without the bound: the wait stays parked however many turns run at one instant.
#[test]
fn a_timer_that_re_arms_at_the_same_instant_ends_the_wait_after_the_turn_bound() {
    let clock = ManualClock::new();
    let (mut runtime, _agent, window) = idle_runtime(&clock, true);
    let mut reply = wait_until(&window, missing(), 60_000);
    let _ = runtime.drain_owner_inbox();
    let now = clock.now();
    // The timer is due at this instant; the scenario's step is not.
    let mut listed = Unsettled::new(now);
    listed.deadline(UnsettledReason::Timer, None, now);
    listed.deadline(
        UnsettledReason::ScenarioStep,
        None,
        now + Duration::from_secs(1),
    );

    for _ in 1..TURNS_AT_ONE_INSTANT {
        runtime.answer_settled(now, HostHalf::Complete(&listed));
    }
    assert!(
        reply.try_take().is_none(),
        "one turn short of the bound, the wait holds out"
    );
    runtime.answer_settled(now, HostHalf::Complete(&listed));
    let outcome = reply
        .try_take()
        .expect("the bound answered")
        .expect("an answer");
    assert_eq!(outcome.end, WaitEnd::Unsatisfied);
    let named: Vec<_> = outcome
        .unsettled
        .iter()
        .map(|cause| (cause.reason, cause.cleared_by))
        .collect();
    assert_eq!(
        named,
        [(UnsettledReason::Timer, ClearedBy::Deadline)],
        "the cause due at that instant, not the later step"
    );
}

/// One served answer per method the runtime answers since it owns settledness: `wait.settled`
/// and `wait.frames`, on the headless host, which composes its half.
///
/// Fails while either method is `not_supported`, or against a host whose answers count the
/// runtime's half alone.
#[test]
fn the_headless_host_answers_wait_settled_and_wait_frames_with_both_halves() {
    let windows = Arc::new(Mutex::new(Vec::new()));
    let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
    let mut host = HeadlessHost::new(HeadlessWindow::new(40, 40)).with_dev_agent(&mut agent);
    host.attach(&flui_widgets::SizedBox::square(10.0))
        .expect("the root mounts");
    host.pump_until_settled(Duration::from_millis(16), 8)
        .expect("a still root settles");
    let window = windows
        .lock()
        .pop()
        .expect("the host handed the agent its window");

    let mut settled = window
        .call::<WaitSettled>(settled_params(window.id(), 1_000))
        .expect("the inbox has room");
    let outcome = host
        .run_until_answered(
            &mut settled,
            Duration::from_millis(16),
            Duration::from_secs(1),
        )
        .expect("answered")
        .expect("served");
    assert_eq!(outcome.end, WaitEnd::Satisfied);
    assert_eq!(outcome.scope, SettleScope::Complete);

    let mut frames = window
        .call::<WaitFrames>(frames_params(window.id(), 2, 1_000))
        .expect("the inbox has room");
    let outcome = host
        .run_until_answered(
            &mut frames,
            Duration::from_millis(16),
            Duration::from_secs(1),
        )
        .expect("answered")
        .expect("served");
    assert_eq!(outcome.end, WaitEnd::Satisfied);
    assert_eq!(outcome.frames, 2, "the frames presented after the request");
    assert_eq!(outcome.scope, SettleScope::Complete);
}
