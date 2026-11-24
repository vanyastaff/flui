//! Settledness has one owner, the host that runs the turns (ADR-0400 §3): the runtime answers
//! a parked wait from the list its host hands it after a turn, and from its own half alone, never
//! idle, where its host composes none.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use flui_foundation::panic::payload_text;
use flui_foundation::testing::panicking_waker;
use flui_foundation::{ManualClock, MonotonicClock};
use flui_platform::PlatformWindow;
use flui_protocol::method::{WaitFrames, WaitSettled, WaitUntil};
use flui_protocol::wait::{
    ClearedBy, Condition, FramesParams, SettleScope, SettledParams, UnsettledReason, WaitEnd,
    WaitOutcome, WaitParams,
};
use flui_protocol::{locator::Locator, tree::WindowId};
use flui_runtime::dev_agent::{AgentWindowSource, DevAgentHost};
use flui_runtime::owner::{OwnerEffects, OwnerHost, RecoveryState, RuntimeOperation};
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::pump::SampledClock;
use flui_runtime::settledness::{HostHalf, TURNS_AT_ONE_INSTANT, Unsettled};
use flui_runtime::sink::{FrameSettlement, FrameSink, Settled, SubmitVerdict};
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

/// A runtime on `clock` with a still root mounted and no frame run yet, built as a host that
/// composes its half (`composes`) or not, and the agent window of its presentation.
fn mounted_runtime(clock: &ManualClock, composes: bool) -> (UiRuntime, Held, AgentWindow) {
    mounted_runtime_waking(clock, composes, Arc::new(|| {}))
}

/// [`mounted_runtime`] whose host wake is `wake`.
fn mounted_runtime_waking(
    clock: &ManualClock,
    composes: bool,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> (UiRuntime, Held, AgentWindow) {
    let fonts = flui_painting::FontCollection::new();
    let mut services = RuntimeHostServices::new(
        wake,
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
    let runtime = UiRuntime::try_new(window, 1.0, services).expect("a manual-clock runtime");
    runtime
        .attach_root_widget(&flui_widgets::SizedBox::square(10.0))
        .expect("the root mounts");
    let agent = DevAgentHost::attach(AgentWindowSource::dev_agent(), [Box::new(Serving) as _]);
    let window = agent
        .vend(&runtime, runtime.presentation_id())
        .expect("a serving hook is vended the window");
    (runtime, Held(agent, platform_window), window)
}

/// [`mounted_runtime`], run until its half lists nothing.
fn idle_runtime(clock: &ManualClock, composes: bool) -> (UiRuntime, Held, AgentWindow) {
    run_until_idle(clock, mounted_runtime(clock, composes))
}

/// Runs `mounted`'s turns at each wake until its half lists nothing.
fn run_until_idle(
    clock: &ManualClock,
    mounted: (UiRuntime, Held, AgentWindow),
) -> (UiRuntime, Held, AgentWindow) {
    let (mut runtime, held, window) = mounted;
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
    (runtime, held, window)
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
/// from moving: the wait ends unsatisfied after the turn bound, naming the timer. The bound
/// counts passes at one instant: a pass at a later instant counts from one again, so passes
/// spread over time never end a wait.
///
/// Fails without the bound (the wait stays parked however many turns run at one instant), or
/// with a count that never restarts (the pass at the later instant ends the wait).
#[test]
fn a_timer_that_re_arms_at_the_same_instant_ends_the_wait_after_the_turn_bound() {
    struct Row {
        name: &'static str,
        /// Whether the clock moves before the pass that would reach the bound.
        moves: bool,
    }
    // The timer is due at `now`; the scenario's step is not.
    let listed_at = |now| {
        let mut listed = Unsettled::new(now);
        listed.deadline(UnsettledReason::Timer, None, now);
        listed.deadline(
            UnsettledReason::ScenarioStep,
            None,
            now + Duration::from_secs(1),
        );
        listed
    };
    let rows = [
        Row {
            name: "every pass at one instant",
            moves: false,
        },
        Row {
            name: "the last pass at a later instant",
            moves: true,
        },
    ];
    for row in rows {
        let clock = ManualClock::new();
        let (mut runtime, _agent, window) = idle_runtime(&clock, true);
        let mut reply = wait_until(&window, missing(), 60_000);
        let _ = runtime.drain_owner_inbox();
        let now = clock.now();
        let listed = listed_at(now);
        for _ in 1..TURNS_AT_ONE_INSTANT {
            runtime.answer_settled(now, HostHalf::Complete(&listed));
        }
        assert!(
            reply.try_take().is_none(),
            "{}: one turn short of the bound, the wait holds out",
            row.name
        );
        if row.moves {
            clock.advance(Duration::from_millis(1));
            let later = clock.now();
            runtime.answer_settled(later, HostHalf::Complete(&listed_at(later)));
            assert!(
                reply.try_take().is_none(),
                "{}: a pass at a later instant counts from one again",
                row.name
            );
            continue;
        }
        runtime.answer_settled(now, HostHalf::Complete(&listed));
        let outcome = reply
            .try_take()
            .expect("the bound answered")
            .expect("an answer");
        assert_eq!(outcome.end, WaitEnd::Unsatisfied, "{}", row.name);
        let named: Vec<_> = outcome
            .unsettled
            .iter()
            .map(|cause| (cause.reason, cause.cleared_by))
            .collect();
        assert_eq!(
            named,
            [(UnsettledReason::Timer, ClearedBy::Deadline)],
            "{}: the cause due at that instant, not the later step",
            row.name
        );
    }
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

/// A sink that queues every frame and keeps its epoch, as a host with a raster thread does.
#[derive(Default)]
struct Queueing(Vec<flui_foundation::FrameEpoch>);

impl FrameSink for Queueing {
    fn surface_size(&mut self) -> (u32, u32) {
        (40, 40)
    }

    fn submit(
        &mut self,
        epoch: flui_foundation::FrameEpoch,
        _: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        self.0.push(epoch);
        SubmitVerdict::Queued
    }
}

/// A queued frame settles by a call outside any turn, as a native runner's raster side makes
/// it. The settlement discharges the last cause, the frame in flight, and no turn follows it:
/// on a host that composes no half the settled wait is answered there, not at its own deadline.
///
/// Fails while only the end of a turn answers from the runtime's half.
#[test]
fn a_queued_frame_s_settlement_answers_a_settled_wait_with_no_turn_after_it() {
    let clock = ManualClock::new();
    let (mut runtime, _held, window) = mounted_runtime(&clock, false);
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: runtime.id(),
        presentation_id: runtime.presentation_id(),
    };
    let mut settled = wait_until(&window, Condition::Settled, 30_000);
    let mut sink = Queueing::default();

    // Turns at each wake, every queued frame settled after its turn, until nothing is listed.
    let mut emptied_by_a_settlement = false;
    let mut half = Unsettled::new(clock.now());
    for _ in 0..16 {
        let next = half.wake().map_or(Duration::from_millis(16), |wake| {
            wake.saturating_duration_since(clock.now())
        });
        clock.advance(next);
        let _ = runtime.pump(&mut SampledClock(clock.now()), &mut sink);
        let mut after_turn = Unsettled::new(clock.now());
        runtime.unsettled(clock.now(), &mut after_turn);
        for epoch in sink.0.drain(..) {
            let applied = runtime.settle_frame(FrameSettlement::new(
                address,
                epoch,
                SubmitVerdict::Presented,
            ));
            assert_eq!(applied, Settled::Applied);
        }
        half = Unsettled::new(clock.now());
        runtime.unsettled(clock.now(), &mut half);
        if half.is_empty() {
            emptied_by_a_settlement = !after_turn.is_empty();
            break;
        }
    }
    assert!(
        emptied_by_a_settlement,
        "the last cause was the frame in flight, which its settlement discharged: {:?}",
        half.causes().collect::<Vec<_>>()
    );

    let outcome = settled
        .try_take()
        .expect("the settlement that left nothing in flight answered the wait")
        .expect("a settled window is an answer");
    assert_eq!(outcome.end, WaitEnd::Satisfied);
    assert_eq!(outcome.scope, SettleScope::RuntimeOnly);
}

/// The payload an agent's waker panics with.
const WAKER_PANIC: &str = "an agent's waker panics";

/// A host wake that panics while `armed` holds, each payload naming its ordinal among the armed
/// calls (`host wake 1`, `host wake 2`, …), which `calls` counts.
fn wake_that_panics_while(
    armed: &Arc<AtomicBool>,
    calls: &Arc<AtomicUsize>,
) -> Arc<dyn Fn() + Send + Sync> {
    let (armed, calls) = (Arc::clone(armed), Arc::clone(calls));
    Arc::new(move || {
        if armed.load(Ordering::SeqCst) {
            let ordinal = calls.fetch_add(1, Ordering::SeqCst) + 1;
            std::panic::panic_any(format!("host wake {ordinal}"));
        }
    })
}

/// A `wait.frames` that holds out: every answer pass that puts it back asks the host for the
/// frames it counts, which wakes the host.
fn frames_that_hold_out(window: &AgentWindow) -> AgentAnswer<WaitOutcome> {
    window
        .call::<WaitFrames>(frames_params(window.id(), 1_000, 60_000))
        .expect("the inbox has room")
}

/// A runtime whose host composes no half answers its settled waits at a turn's end behind the
/// turn's own failure. A reply whose waker panics is answered and fails nothing: the reply
/// contains its waker (ADR-0288 §3). Where the turn failed first (its host wake panicked), that
/// failure continues and the answer pass's own (the wake it asks for the frames a parked
/// `wait.frames` counts) is discarded under containment (ADR-0281). Either way the wait is
/// answered once, nothing is left parked, and the next turn works.
///
/// Fails against a turn end that answers uncontained (the answer pass's later panic replaces the
/// turn's) or that skips the answers once the turn failed (the wait is not answered).
#[test]
fn a_turn_s_end_answers_settled_waits_behind_the_turn_s_first_failure() {
    struct Row {
        name: &'static str,
        turn_fails: bool,
    }
    let rows = [
        Row {
            name: "a reply whose waker panics",
            turn_fails: false,
        },
        Row {
            name: "a turn that failed first",
            turn_fails: true,
        },
    ];
    for row in rows {
        let clock = ManualClock::new();
        let armed = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (runtime, _held, window) = run_until_idle(
            &clock,
            mounted_runtime_waking(&clock, false, wake_that_panics_while(&armed, &calls)),
        );
        let frames = frames_that_hold_out(&window);
        let _ = runtime.drain_owner_inbox();
        // Parked and past its deadline in one turn: its end answers it.
        let mut until = wait_until(&window, missing(), 0);
        until.register(&panicking_waker(WAKER_PANIC));

        armed.store(row.turn_fails, Ordering::SeqCst);
        let turn = catch_unwind(AssertUnwindSafe(|| runtime.drain_owner_inbox()));
        armed.store(false, Ordering::SeqCst);

        match turn {
            Ok(_) => assert!(
                !row.turn_fails,
                "{}: the turn's failure continues",
                row.name
            ),
            Err(payload) => {
                assert!(
                    row.turn_fails,
                    "{}: a reply's waker fails nothing",
                    row.name
                );
                assert_eq!(
                    payload_text(&*payload),
                    Some("host wake 1"),
                    "{}: the turn's own failure, the first",
                    row.name
                );
                assert!(
                    calls.load(Ordering::SeqCst) >= 2,
                    "{}: the answer pass ran after the failure and woke the host too",
                    row.name
                );
            }
        }
        let outcome = until
            .try_take()
            .expect("the turn's end answered the wait")
            .expect("an answer");
        assert_eq!(outcome.end, WaitEnd::Unsatisfied, "{}", row.name);
        assert_eq!(outcome.scope, SettleScope::RuntimeOnly, "{}", row.name);

        drop(frames);
        let _ = runtime.drain_owner_inbox();
        assert!(
            !runtime.awaits_settled_answer(),
            "{}: no wait is left parked to be answered again",
            row.name
        );
    }
}

/// A queued frame's settlement, outside any turn, answers the settled waits of a runtime whose
/// host composes no half behind the settlement's own failure: a reply whose waker panics fails
/// nothing, and where the settlement's wake for the frame it retries panicked first, that failure
/// continues and the answer pass's later one is discarded. Either way the wait is answered once,
/// nothing is left parked, and the next turn works.
///
/// Fails against a settlement that answers uncontained (the answer pass's later panic replaces
/// the settlement's) or only once its own tail succeeded (the wait is not answered).
#[test]
fn a_settlement_answers_settled_waits_behind_its_own_first_failure() {
    struct Row {
        name: &'static str,
        verdict: SubmitVerdict,
        settlement_fails: bool,
    }
    let rows = [
        Row {
            name: "a reply whose waker panics",
            verdict: SubmitVerdict::Presented,
            settlement_fails: false,
        },
        Row {
            name: "a settlement that failed first",
            verdict: SubmitVerdict::Retry,
            settlement_fails: true,
        },
    ];
    for row in rows {
        let clock = ManualClock::new();
        let armed = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let (mut runtime, _held, window) =
            mounted_runtime_waking(&clock, false, wake_that_panics_while(&armed, &calls));
        let address = flui_foundation::PresentationAddress {
            ui_runtime_id: runtime.id(),
            presentation_id: runtime.presentation_id(),
        };
        let frames = frames_that_hold_out(&window);
        let mut until = wait_until(&window, missing(), 10);
        until.register(&panicking_waker(WAKER_PANIC));
        let mut sink = Queueing::default();
        clock.advance(Duration::from_millis(16));
        let _ = runtime.pump(&mut SampledClock(clock.now()), &mut sink);
        assert!(
            until.try_take().is_none(),
            "{}: the turn ended before the wait's deadline",
            row.name
        );
        let epoch = *sink
            .0
            .last()
            .unwrap_or_else(|| panic!("{}: the turn queued a frame", row.name));

        // The settlement comes past the wait's deadline.
        clock.advance(Duration::from_millis(20));
        armed.store(row.settlement_fails, Ordering::SeqCst);
        let settled = catch_unwind(AssertUnwindSafe(|| {
            runtime.settle_frame(FrameSettlement::new(address, epoch, row.verdict))
        }));
        armed.store(false, Ordering::SeqCst);

        match settled {
            Ok(applied) => {
                assert!(
                    !row.settlement_fails,
                    "{}: the settlement's failure continues",
                    row.name
                );
                assert_eq!(applied, Settled::Applied, "{}", row.name);
            }
            Err(payload) => {
                assert!(
                    row.settlement_fails,
                    "{}: a reply's waker fails nothing",
                    row.name
                );
                assert_eq!(
                    payload_text(&*payload),
                    Some("host wake 1"),
                    "{}: the settlement's own failure, the first",
                    row.name
                );
                assert!(
                    calls.load(Ordering::SeqCst) >= 2,
                    "{}: the answer pass ran after the failure and woke the host too",
                    row.name
                );
            }
        }
        let outcome = until
            .try_take()
            .expect("the settlement answered the wait")
            .expect("an answer");
        assert_eq!(outcome.end, WaitEnd::Unsatisfied, "{}", row.name);

        drop(frames);
        let _ = runtime.drain_owner_inbox();
        assert!(
            !runtime.awaits_settled_answer(),
            "{}: no wait is left parked to be answered again",
            row.name
        );
    }
}

/// A runner's owner: a frame it delivers pumps the runtime at the clock's reading into a sink
/// that shows every frame; it installs nothing and retires nothing.
struct OwnerFrames(ManualClock);

impl OwnerEffects for OwnerFrames {
    fn runtime_lifecycle(
        &self,
        _: flui_foundation::UiRuntimeId,
        _: flui_scheduler::AppLifecycleState,
    ) {
    }
    fn runtimes_stopped(&self, _: RecoveryState) {}
    fn retire_host(&self, _: &[flui_foundation::PresentationAddress], _: RecoveryState) {}
    fn commit_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: RecoveryState,
    ) -> Option<flui_runtime::owner::InstallInitialization> {
        panic!("no pending install");
    }
    fn finish_install(
        &self,
        _: flui_foundation::PresentationAddress,
        _: flui_runtime::owner::InitializationOutcome,
        _: RecoveryState,
    ) {
    }
    fn cancel_install(&self, _: flui_runtime::owner::InstallToken, _: RecoveryState) {
        panic!("no pending install");
    }
    fn resize_surface(
        &self,
        _: flui_foundation::PresentationAddress,
        _: flui_foundation::geometry::WindowMetrics,
    ) {
    }
    fn frame(&self, _: flui_foundation::PresentationAddress, runtime: &mut UiRuntime) {
        let _ = runtime.pump(&mut SampledClock(self.0.now()), &mut Shown);
    }
    fn retire_presentation(
        &self,
        _: flui_foundation::PresentationAddress,
        _: Option<flui_foundation::PresentationAddress>,
    ) {
    }
    fn after_turn(&self, _: RecoveryState) {}
    fn request_continuation(&self) -> bool {
        true
    }
}

/// On a runner every turn comes through an owner dispatch, which holds the runtime's turn open
/// around its work, so no turn the runtime runs inside it is the outermost: a runtime whose host
/// composes no half answers its settled waits where the dispatch ends. A settled wait is answered
/// once the frames a dispatch delivered leave nothing listed, and a wait that never holds is
/// answered at its deadline by the background turn delivered then.
///
/// Fails against a runtime that answers only where a turn it ran by itself ends: inside a
/// dispatch neither wait is ever answered.
#[test]
fn an_owner_dispatch_answers_the_settled_waits_of_a_host_without_a_half() {
    let clock = ManualClock::new();
    let (runtime, _held, window) = mounted_runtime(&clock, false);
    let owner = OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("the runtime publishes")
        .commit();
    let effects = OwnerFrames(clock.clone());
    let mut settled = wait_until(&window, Condition::Settled, 5_000);
    let mut never = wait_until(&window, missing(), 200);

    let frames = owner.frame_dispatcher(address).expect("frame authority");
    let mut answered = None;
    for _ in 0..8 {
        clock.advance(Duration::from_millis(16));
        frames
            .deliver(crate::VSYNC, &effects)
            .expect("the owner admits the frame");
        answered = settled.try_take();
        if answered.is_some() {
            break;
        }
    }
    let outcome = answered
        .expect("a frame dispatch's end answered the settled wait")
        .expect("an answer");
    assert_eq!(outcome.end, WaitEnd::Satisfied);
    assert_eq!(outcome.scope, SettleScope::RuntimeOnly);
    assert!(never.try_take().is_none(), "before its deadline");

    clock.advance(Duration::from_millis(200));
    owner
        .runtime_dispatcher(address.ui_runtime_id)
        .expect("runtime authority")
        .deliver(RuntimeOperation::Background, &effects)
        .expect("the owner admits the background turn");
    let outcome = never
        .try_take()
        .expect("the background dispatch's end answered the wait at its deadline")
        .expect("an answer");
    assert_eq!(outcome.end, WaitEnd::Unsatisfied);
    assert_eq!(outcome.scope, SettleScope::RuntimeOnly);
}
