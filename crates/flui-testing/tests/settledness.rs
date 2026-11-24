//! The headless host owns one list of what keeps the application from being idle (ADR-0400 §3):
//! its settle, its loop's wake and the agent's settled waits all read it, and a wait run on its
//! virtual clock moves the clock to that wake.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Poll, Waker};
use std::time::Duration;

use flui_foundation::panic::payload_text;
use flui_foundation::testing::{CountingWaker, panicking_waker};
use flui_platform::{WindowCommand, WindowFrame};
use flui_protocol::locator::Locator;
use flui_protocol::method::{WaitSettled, WaitUntil};
use flui_protocol::tree::WindowId;
use flui_protocol::wait::{
    ClearedBy, Condition, SettleScope, SettledParams, UnsettledCause, UnsettledReason, WaitEnd,
    WaitOutcome, WaitParams,
};
use flui_runtime::owner_turn::{CloseRequestHandler, CloseResponse};
use flui_runtime::ui_runtime::DEFAULT_REFRESH_PERIOD;
use flui_scheduler::Instant;
use flui_testing::host::{HeadlessDevAgent, HeadlessHost, HeadlessWindow, UnshownFrame};
use flui_testing::widgets::SharedSignal;
use flui_testing::window_commands::AdmitAndApply as _;
use flui_view::background::BackgroundCapability;
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook};
use flui_view::prelude::*;
use flui_widgets::{ActivityIndicator, Column, IndexedStack, SizedBox};
use parking_lot::Mutex;

use crate::capability_seam::acquire_capability;

const FRAME: Duration = Duration::from_millis(16);

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

/// A host whose agent holds its window, and that window.
struct Served {
    host: HeadlessHost,
    agent: HeadlessDevAgent,
    windows: Arc<Mutex<Vec<AgentWindow>>>,
    /// What the row keeps alive: a background task, a parked future's release.
    kept: Vec<Box<dyn std::any::Any>>,
}

impl Served {
    fn new(window: HeadlessWindow) -> Self {
        let windows = Arc::new(Mutex::new(Vec::new()));
        let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
        let host = HeadlessHost::new(window).with_dev_agent(&mut agent);
        Self {
            host,
            agent,
            windows,
            kept: Vec::new(),
        }
    }

    /// Mounted with `root` and pumped once.
    fn mounted(window: HeadlessWindow, root: &(impl View + Clone + 'static)) -> Self {
        let mut served = Self::new(window);
        served.host.attach(root).expect("a fresh host has no root");
        let _ = served.host.pump(FRAME);
        served
    }

    fn window(&self) -> AgentWindow {
        self.windows
            .lock()
            .last()
            .cloned()
            .expect("the host handed the agent its window")
    }

    fn wait_settled(&self, timeout_ms: u64) -> AgentAnswer<WaitOutcome> {
        let window = self.window();
        window
            .call::<WaitSettled>(settled_params(window.id(), timeout_ms))
            .expect("the inbox has room")
    }

    fn wait_until(&self, condition: Condition, timeout_ms: u64) -> AgentAnswer<WaitOutcome> {
        let window = self.window();
        window
            .call::<WaitUntil>(WaitParams::new(window.id(), condition).timeout_ms(timeout_ms))
            .expect("the inbox has room")
    }
}

/// `wait.settled` on `window`; the protocol gives the parameters no constructor, so they are
/// read as a client writes them.
fn settled_params(window: WindowId, timeout_ms: u64) -> SettledParams {
    serde_json::from_value(serde_json::json!({ "window": window, "timeout_ms": timeout_ms }))
        .expect("the parameters a client writes")
}

fn named(causes: &[UnsettledCause]) -> Vec<(UnsettledReason, ClearedBy)> {
    causes
        .iter()
        .map(|cause| (cause.reason, cause.cleared_by))
        .collect()
}

/// A widget that spawns a task sleeping `after` on the runtime's clock, then setting `done`.
#[derive(Clone, StatefulView)]
struct Sleeper {
    after: Duration,
    /// The runtime's clock reading when the sleep resolved.
    woke: Rc<Cell<Option<Instant>>>,
}

impl Sleeper {
    fn new(after: Duration) -> Self {
        Self {
            after,
            woke: Rc::default(),
        }
    }
}

struct SleeperState {
    after: Duration,
    woke: Rc<Cell<Option<Instant>>>,
    task: Option<flui_scheduler::TaskToken>,
}

impl StatefulView for Sleeper {
    type State = SleeperState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        SleeperState {
            after: self.after,
            woke: Rc::clone(&self.woke),
            task: None,
        }
    }
}

impl ViewState<Sleeper> for SleeperState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let timer = ctx.timer().expect("a hosted tree has a timer");
        let sleep = timer.sleep(self.after);
        let woke = Rc::clone(&self.woke);
        self.task = ctx.async_driver().map(|driver| {
            driver.spawn_local(Box::pin(async move {
                sleep.await;
                woke.set(Some(timer.now()));
            }))
        });
    }

    fn build(&self, _view: &Sleeper, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(10.0)
    }
}

/// A future parked until the test releases it.
#[derive(Clone, Default)]
struct Parked(Rc<RefCell<(bool, Option<Waker>)>>);

impl Parked {
    fn release(&self) {
        let waker = {
            let mut slot = self.0.borrow_mut();
            slot.0 = true;
            slot.1.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl std::future::Future for Parked {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, ctx: &mut std::task::Context<'_>) -> Poll<()> {
        let mut slot = self.0.borrow_mut();
        if slot.0 {
            return Poll::Ready(());
        }
        slot.1 = Some(ctx.waker().clone());
        Poll::Pending
    }
}

/// A widget whose task waits on a [`Parked`] future.
#[derive(Clone, StatefulView)]
struct Awaiting {
    parked: Parked,
}

struct AwaitingState {
    parked: Parked,
    task: Option<flui_scheduler::TaskToken>,
}

impl StatefulView for Awaiting {
    type State = AwaitingState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        AwaitingState {
            parked: self.parked.clone(),
            task: None,
        }
    }
}

impl ViewState<Awaiting> for AwaitingState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let parked = self.parked.clone();
        self.task = ctx
            .async_driver()
            .map(|driver| driver.spawn_local(Box::pin(parked)));
    }

    fn build(&self, _view: &Awaiting, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(10.0)
    }
}

/// A widget that shows the number a test sets.
#[derive(Clone, StatefulView)]
struct Shown {
    number: SharedSignal<u32>,
}

struct ShownState {
    number: SharedSignal<u32>,
}

impl StatefulView for Shown {
    type State = ShownState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ShownState {
            number: self.number.clone(),
        }
    }
}

impl ViewState<Shown> for ShownState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.number.bind(ctx);
    }

    fn build(&self, _view: &Shown, ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(f64::from(self.number.get(ctx)))
    }
}

fn still() -> SizedBox {
    SizedBox::square(10.0)
}

/// One row: a state that lists `reason`, discharged by `cleared_by`.
struct Row {
    reason: UnsettledReason,
    cleared_by: ClearedBy,
    /// Whether the turn that serves the wait leaves the cause listed.
    persists: bool,
    state: fn() -> Served,
}

fn rows() -> Vec<Row> {
    vec![
        Row {
            reason: UnsettledReason::NeedsRedraw,
            cleared_by: ClearedBy::NextFrame,
            persists: false,
            state: || {
                let served = Served::mounted(HeadlessWindow::new(40, 40), &still());
                served.host.request_frame();
                served
            },
        },
        Row {
            reason: UnsettledReason::FrameScheduled,
            cleared_by: ClearedBy::NextFrame,
            persists: false,
            state: || {
                let parked = Parked::default();
                let mut served = Served::mounted(
                    HeadlessWindow::new(40, 40),
                    &Awaiting {
                        parked: parked.clone(),
                    },
                );
                let _ = served.host.pump(FRAME);
                parked.release();
                served.kept.push(Box::new(parked));
                served
            },
        },
        Row {
            reason: UnsettledReason::PendingWork,
            cleared_by: ClearedBy::NextFrame,
            persists: false,
            state: || {
                let number = SharedSignal::new(10);
                let mut served = Served::mounted(
                    HeadlessWindow::new(40, 40),
                    &Shown {
                        number: number.clone(),
                    },
                );
                number.set(12);
                served.kept.push(Box::new(number));
                served
            },
        },
        Row {
            reason: UnsettledReason::Timer,
            cleared_by: ClearedBy::Deadline,
            persists: true,
            state: || {
                let mut served = Served::mounted(
                    HeadlessWindow::new(40, 40),
                    &Sleeper::new(Duration::from_secs(60)),
                );
                // The task spawned at the mount sleeps once polled.
                let _ = served.host.pump(FRAME);
                served
            },
        },
        Row {
            // A ticker whose widget is not painted is still named: an indexed stack lays its
            // hidden children out and keeps their animations running.
            reason: UnsettledReason::Ticker,
            cleared_by: ClearedBy::NextFrame,
            persists: true,
            state: || {
                Served::mounted(
                    HeadlessWindow::new(40, 40),
                    &IndexedStack::new(vec![
                        still().into_view().boxed(),
                        ActivityIndicator::new().into_view().boxed(),
                    ])
                    .index(Some(0)),
                )
            },
        },
        Row {
            reason: UnsettledReason::Background,
            cleared_by: ClearedBy::Test,
            persists: true,
            state: || {
                let mut served = Served::new(HeadlessWindow::new(40, 40));
                let background = acquire_capability::<BackgroundCapability>(&mut served.host)
                    .expect("the headless host runs background work");
                served.host.hold_background();
                served.kept.push(Box::new(background.compute(|| 1)));
                served
            },
        },
        Row {
            reason: UnsettledReason::Background,
            cleared_by: ClearedBy::NextFrame,
            persists: false,
            state: || {
                let mut served = Served::new(HeadlessWindow::new(40, 40));
                let background = acquire_capability::<BackgroundCapability>(&mut served.host)
                    .expect("the headless host runs background work");
                served.kept.push(Box::new(background.compute(|| 1)));
                served
            },
        },
        Row {
            reason: UnsettledReason::Background,
            cleared_by: ClearedBy::Outside,
            persists: true,
            state: || {
                let mut served = Served::new(HeadlessWindow::new(40, 40));
                let background = acquire_capability::<BackgroundCapability>(&mut served.host)
                    .expect("the headless host runs background work");
                served
                    .kept
                    .push(Box::new(background.io(std::future::pending::<()>())));
                let _ = served.host.pump(Duration::ZERO);
                served
            },
        },
        Row {
            reason: UnsettledReason::OwedTurn,
            cleared_by: ClearedBy::NextFrame,
            persists: false,
            state: || {
                let window = HeadlessWindow::new(40, 40).with_close_request_handler(
                    CloseRequestHandler::new(|_| CloseResponse::KeepOpen),
                );
                let mut served = Served::mounted(window, &still());
                served.host.request_quit();
                served
            },
        },
        Row {
            reason: UnsettledReason::WindowObservation,
            cleared_by: ClearedBy::NextFrame,
            persists: false,
            state: || {
                let served = Served::mounted(HeadlessWindow::new(40, 40), &still());
                served
                    .host
                    .window()
                    .admit_and_apply(WindowCommand::SetFrame(WindowFrame::None))
                    .expect("the headless window takes every frame");
                served
            },
        },
        Row {
            reason: UnsettledReason::FrameRequest,
            cleared_by: ClearedBy::Deadline,
            persists: true,
            state: covered_with_a_change,
        },
        Row {
            reason: UnsettledReason::SurfaceFrame,
            cleared_by: ClearedBy::NextFrame,
            persists: false,
            state: || {
                let served = covered_with_a_change();
                let wait = served
                    .host
                    .time_to_next_frame()
                    .expect("the covered window asked for its frame");
                served.host.advance(wait);
                served
            },
        },
        Row {
            reason: UnsettledReason::ScenarioStep,
            cleared_by: ClearedBy::Deadline,
            persists: true,
            state: || {
                let served = Served::mounted(HeadlessWindow::new(40, 40), &still());
                served
                    .host
                    .scenario()
                    .events::<u32>("ticks")
                    .emit_at(Duration::from_secs(1), 7);
                served
            },
        },
    ]
}

/// A window that tells a covering and keeps a covered window's surface, covered, with its
/// content changed after a frame: it owes its surface a frame once quiet, which its window was
/// asked for at an instant.
fn covered_with_a_change() -> Served {
    let number = SharedSignal::new(10);
    let mut served = Served::mounted(
        HeadlessWindow::new(40, 40).with_occlusion_reports(),
        &Shown {
            number: number.clone(),
        },
    );
    served
        .host
        .set_window_occluded(served.host.primary_window(), true);
    let _ = served.host.pump(FRAME);
    number.set(12);
    let _ = served.host.pump(FRAME);
    served.kept.push(Box::new(number));
    served
}

/// **One row per cause the headless host lists** (the runtime's and the host's; no host lists a
/// platform reply in flight), on which the host's settle and `wait.settled` agree. Before the
/// turn the list names the cause with what discharges it, and the settle does not end; the
/// `wait.settled` the next turn serves, answered once that turn's own steps are done, is
/// satisfied exactly when the settle then ends at once, and where the cause outlives that turn it
/// names it, with its `cleared_by`, as the list does.
///
/// Fails against a host whose settle keeps a predicate of its own beside the list, or a runtime
/// that answers `wait.settled` from its half alone: a host cause is missing from the answer.
#[test]
fn the_host_settle_and_wait_settled_agree_on_every_cause() {
    let mut failed = Vec::new();
    for row in rows() {
        let label = format!("{:?}/{:?}", row.reason, row.cleared_by);
        if std::panic::catch_unwind(|| check(&row)).is_err() {
            failed.push(label);
        }
    }
    assert!(failed.is_empty(), "failing rows: {failed:?}");
}

fn check(row: &Row) {
    let mut served = (row.state)();
    let label = format!("{:?}/{:?}", row.reason, row.cleared_by);
    let listed = served.host.unsettled();
    let listed_causes: Vec<UnsettledCause> = listed.causes().cloned().collect();
    assert!(
        named(&listed_causes).contains(&(row.reason, row.cleared_by)),
        "{label}: the list names it: {listed_causes:?}"
    );
    assert!(
        served.host.pump_until_settled(FRAME, 0).is_err(),
        "{label}: the settle does not end on it"
    );

    let mut answer = served.wait_settled(0);
    let _ = served.host.pump(Duration::ZERO);
    let outcome = answer
        .try_take()
        .expect("answered after the turn that served it")
        .expect("a wait is an answer");
    assert_eq!(outcome.scope, SettleScope::Complete, "{label}");
    let settles = served.host.pump_until_settled(FRAME, 0);
    assert_eq!(
        outcome.end == WaitEnd::Satisfied,
        settles == Ok(0),
        "{label}: the wait and the settle read one list: {outcome:?}, {settles:?}"
    );
    if row.persists {
        assert!(
            named(&outcome.unsettled).contains(&(row.reason, row.cleared_by)),
            "{label}: the answer names what the list names: {outcome:?}"
        );
    }
    // A step the scenario still holds is acknowledged, not left to fail its drop.
    let _ = served.host.scenario().verify();
}

/// **Held background work never wakes the loop, and a false wait ends idle naming it.** Only
/// the test can release held work, so a host whose half is complete answers at once instead of
/// holding out to the wait's limit, and its loop moves no time.
///
/// Fails against a host that wakes for held work (the loop spins a turn at a time) or that
/// lists it as the next turn's (the wait holds out to its limit).
#[test]
fn held_background_work_never_wakes_the_loop_and_ends_a_false_wait_idle() {
    let mut served = Served::new(HeadlessWindow::new(40, 40));
    let background = acquire_capability::<BackgroundCapability>(&mut served.host)
        .expect("the headless host runs background work");
    served.host.hold_background();
    let _task = background.compute(|| 1);
    assert_eq!(
        served.host.time_to_next_frame(),
        None,
        "held work wakes nothing"
    );

    let started = served.host.clock().elapsed();
    let mut answer = served.wait_until(Condition::Appears(Locator::named("Missing")), 5_000);
    let outcome = served
        .host
        .run_until_answered(&mut answer, FRAME, Duration::from_secs(5))
        .expect("answered")
        .expect("a wait is an answer");
    assert_eq!(outcome.end, WaitEnd::Idle);
    assert_eq!(
        named(&outcome.unsettled),
        [(UnsettledReason::Background, ClearedBy::Test)]
    );
    assert_eq!(
        served.host.clock().elapsed(),
        started,
        "the loop moved no time"
    );
}

/// **A parked wait's deadline, and a timer a fraction of a millisecond away, are reached in
/// virtual time.** The loop moves the clock to the list's wake: the wait's own deadline before a
/// timer a minute away, and a timer 0.5 ms away exactly, though its report rounds it to 0 ms.
///
/// Fails against a loop that steps a frame at a time (the wait ends past its deadline, the timer
/// a frame late) or that wakes from the rounded report (the timer is never reached).
#[test]
fn a_wait_on_the_virtual_clock_reaches_each_deadline_exactly() {
    struct Case {
        name: &'static str,
        timer: Duration,
        timeout_ms: u64,
        ends: WaitEnd,
        /// When the wait is answered, from the turn that parked it.
        answered: Duration,
        /// When the timer's task woke, if it did.
        woke: Option<Duration>,
    }
    let cases = [
        Case {
            name: "the wait's deadline before a far timer",
            timer: Duration::from_secs(60),
            timeout_ms: 200,
            ends: WaitEnd::Unsatisfied,
            answered: Duration::from_millis(200),
            woke: None,
        },
        Case {
            // The timer is reached at its instant though its report rounds it to 0 ms; the
            // window settles a frame later, once the frame its task's wake asked for has run.
            name: "a timer a fraction of a millisecond away",
            timer: Duration::from_micros(500),
            timeout_ms: 1_000,
            ends: WaitEnd::Satisfied,
            answered: Duration::from_micros(500) + FRAME,
            woke: Some(Duration::from_micros(500)),
        },
    ];
    for case in cases {
        let mut served = Served::new(HeadlessWindow::new(40, 40));
        let sleeper = Sleeper::new(case.timer);
        let woke = Rc::clone(&sleeper.woke);
        served
            .host
            .attach(&sleeper)
            .expect("a fresh host has no root");
        let _ = served.host.pump(Duration::ZERO);
        let started = flui_foundation::MonotonicClock::now(served.host.clock());
        let mut answer = served.wait_settled(case.timeout_ms);
        let outcome = served
            .host
            .run_until_answered(&mut answer, FRAME, Duration::from_secs(2))
            .expect("answered")
            .expect("a wait is an answer");
        assert_eq!(outcome.end, case.ends, "{}: {outcome:?}", case.name);
        assert_eq!(
            flui_foundation::MonotonicClock::now(served.host.clock())
                .saturating_duration_since(started),
            case.answered,
            "{}",
            case.name
        );
        assert_eq!(
            woke.get().map(|at| at.saturating_duration_since(started)),
            case.woke,
            "{}",
            case.name
        );
        if case.ends == WaitEnd::Unsatisfied {
            assert!(
                named(&outcome.unsettled).contains(&(UnsettledReason::Timer, ClearedBy::Deadline)),
                "{}: the timer that kept it unsettled is named: {outcome:?}",
                case.name
            );
        }
    }
}

/// **A redraw while a dropped frame's retry is armed wakes at the retry.** The runtime runs no
/// frame sooner, so the redraw is listed as served at the retry's instant, and the loop sleeps
/// until then.
///
/// Fails against a list that keeps the redraw the next turn's: the loop wakes at once and the
/// frame waits anyway.
#[test]
fn a_redraw_while_a_retry_is_armed_wakes_at_the_retry() {
    let number = SharedSignal::new(10);
    let mut host = HeadlessHost::new(HeadlessWindow::new(40, 40));
    host.attach(&Shown {
        number: number.clone(),
    })
    .expect("a fresh host has no root");
    let _ = host.pump(Duration::ZERO);
    // The frame of a change fails, which arms its retry a refresh period on.
    host.leave_unshown(1, UnshownFrame::Failed);
    number.set(12);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host.pump(Duration::ZERO)));
    host.request_frame();

    let listed = host.unsettled();
    let redraw = listed
        .causes()
        .find(|cause| cause.reason == UnsettledReason::NeedsRedraw)
        .unwrap_or_else(|| {
            panic!(
                "the redraw is listed: {:?}",
                listed.causes().collect::<Vec<_>>()
            )
        });
    assert_eq!(redraw.cleared_by, ClearedBy::Deadline, "{redraw:?}");
    assert_eq!(
        host.time_to_next_frame(),
        Some(DEFAULT_REFRESH_PERIOD),
        "the loop sleeps until the retry"
    );
}

fn missing() -> Condition {
    Condition::Appears(Locator::named("Missing"))
}

/// **A pump answers its settled waits behind the frame's own failure.** A reply whose waker
/// panics is answered and fails nothing (the reply contains its waker, ADR-0288 §3), so a healthy
/// pump returns its outcome; a pump whose frame failed raises that failure, and answers its
/// waits all the same. Either way every wait is answered once, the other waits too, nothing is
/// left parked, and the next pump works.
///
/// Fails against a pump that answers only after a frame that failed nothing (the waits stay
/// parked past their deadline), or that raises a reply's panic in place of the frame's failure.
#[test]
fn a_pump_answers_its_settled_waits_behind_the_frame_s_failure() {
    const WAKER_PANIC: &str = "an agent's waker panics";
    struct Row {
        name: &'static str,
        frame_fails: bool,
        /// How a false wait past its deadline ends: idle on a still window, unsatisfied while
        /// the failed frame's retry is listed.
        ends: WaitEnd,
    }
    let rows = [
        Row {
            name: "a healthy frame",
            frame_fails: false,
            ends: WaitEnd::Idle,
        },
        Row {
            name: "a frame that failed first",
            frame_fails: true,
            ends: WaitEnd::Unsatisfied,
        },
    ];
    for row in rows {
        let number = SharedSignal::new(10);
        let mut served = Served::mounted(
            HeadlessWindow::new(40, 40),
            &Shown {
                number: number.clone(),
            },
        );
        served
            .host
            .pump_until_settled(FRAME, 8)
            .expect("a still root settles");
        let mut panicking = served.wait_until(missing(), 0);
        panicking.register(&panicking_waker(WAKER_PANIC));
        let counting = CountingWaker::new().shared();
        let mut counted = served.wait_until(missing(), 0);
        counted.register(&counting.waker());
        if row.frame_fails {
            // A lost device is a dropped frame the pump raises.
            served.host.leave_unshown(1, UnshownFrame::DeviceLost);
            number.set(12);
        }

        let pumped = catch_unwind(AssertUnwindSafe(|| served.host.pump(Duration::ZERO)));
        match pumped {
            Ok(_) => assert!(
                !row.frame_fails,
                "{}: the frame's failure is raised",
                row.name
            ),
            Err(payload) => {
                assert!(
                    row.frame_fails,
                    "{}: a reply's waker fails nothing",
                    row.name
                );
                let text = payload_text(&*payload);
                assert!(
                    text.is_some_and(|text| text != WAKER_PANIC),
                    "{}: the frame's failure, not the waker's: {text:?}",
                    row.name
                );
            }
        }
        for (answer, which) in [
            (&mut panicking, "the wait whose waker panicked"),
            (&mut counted, "the other wait"),
        ] {
            let Some(answered) = answer.try_take() else {
                panic!("{}: {which} is answered by the pump", row.name);
            };
            let outcome = answered.expect("an answer");
            assert_eq!(outcome.end, row.ends, "{}: {which}", row.name);
        }
        assert_eq!(counting.count(), 1, "{}: woken once", row.name);

        served
            .host
            .pump_until_settled(FRAME, 8)
            .expect("the next pumps work and settle");
        assert_eq!(
            served.host.time_to_next_frame(),
            None,
            "{}: no wait is left parked, whose deadline would wake the loop",
            row.name
        );
        served.kept.push(Box::new(number));
    }
}

/// **A `wait.settled` counts its own window's causes and the application's, not another
/// window's.** A still window is settled while a second window animates; the animating window
/// is not, and names its ticker; held background work, the application's, keeps the still
/// window unsettled and is what its answer names.
///
/// Fails against a wait that counts every window's causes: the still window's wait holds out
/// while the other animates.
#[test]
fn wait_settled_counts_its_own_window_and_the_application_not_another_window() {
    let mut served = Served::new(HeadlessWindow::new(40, 40));
    // The still window's root is the widget that acquires background work.
    let background = acquire_capability::<BackgroundCapability>(&mut served.host)
        .expect("the headless host runs background work");
    let second = served.host.open_window(HeadlessWindow::new(40, 40));
    served
        .host
        .attach_to(second, &ActivityIndicator::new())
        .expect("a new window has no root");
    served.host.serve_window(second, &mut served.agent);
    let _ = served.host.pump(FRAME);
    let windows = served.windows.lock().clone();
    let [still_window, animating] = windows.as_slice() else {
        panic!("the host handed the agent both windows: {}", windows.len());
    };
    let settled = |window: &AgentWindow, timeout_ms| {
        window
            .call::<WaitSettled>(settled_params(window.id(), timeout_ms))
            .expect("the inbox has room")
    };

    let mut answer = settled(still_window, 1_000);
    let _ = served.host.pump(Duration::ZERO);
    let outcome = answer
        .try_take()
        .expect("the pump answered the still window's wait")
        .expect("a wait is an answer");
    assert_eq!(outcome.end, WaitEnd::Satisfied, "{outcome:?}");

    let mut answer = settled(animating, 0);
    let _ = served.host.pump(Duration::ZERO);
    let outcome = answer
        .try_take()
        .expect("its deadline answered the animating window's wait")
        .expect("a wait is an answer");
    assert_eq!(outcome.end, WaitEnd::Unsatisfied);
    assert!(
        outcome.unsettled.iter().any(|cause| {
            cause.reason == UnsettledReason::Ticker && cause.window == Some(animating.id())
        }),
        "its own ticker is named: {outcome:?}"
    );

    served.host.hold_background();
    let _task = background.compute(|| 1);
    let mut answer = settled(still_window, 0);
    let _ = served.host.pump(Duration::ZERO);
    let outcome = answer
        .try_take()
        .expect("its deadline answered the still window's wait")
        .expect("a wait is an answer");
    assert_eq!(outcome.end, WaitEnd::Unsatisfied);
    assert_eq!(
        named(&outcome.unsettled),
        [(UnsettledReason::Background, ClearedBy::Test)],
        "the application's cause, not the other window's ticker"
    );
}

/// **`run_until_answered` stops unanswered at its limit of virtual time, and when nothing is
/// left to wake for.** A false wait that holds out a minute while an animation keeps the loop
/// turning is left parked at the limit, the clock exactly there; an idle host run for a request
/// another host holds stops at once, the clock unmoved.
///
/// Fails against a loop that runs past its limit (to the wait's own deadline) or that steps a
/// frame when nothing wakes it (to its limit).
#[test]
fn run_until_answered_stops_at_its_limit_and_when_nothing_wakes() {
    let mut served = Served::mounted(HeadlessWindow::new(40, 40), &ActivityIndicator::new());
    let started = served.host.clock().elapsed();
    let mut answer = served.wait_until(missing(), 60_000);
    let limit = Duration::from_millis(100);
    assert!(
        served
            .host
            .run_until_answered(&mut answer, FRAME, limit)
            .is_none(),
        "the limit stops it unanswered"
    );
    assert_eq!(
        served.host.clock().elapsed().saturating_sub(started),
        limit,
        "the clock stops at the limit"
    );
    assert!(answer.try_take().is_none(), "the wait still holds out");

    let mut idle = HeadlessHost::new(HeadlessWindow::new(40, 40));
    idle.attach(&still()).expect("a fresh host has no root");
    idle.pump_until_settled(FRAME, 8)
        .expect("a still root settles");
    let before = idle.clock().elapsed();
    let mut elsewhere = served.wait_settled(1_000);
    assert!(
        idle.run_until_answered(&mut elsewhere, FRAME, Duration::from_secs(1))
            .is_none(),
        "nothing wakes the idle host"
    );
    assert_eq!(idle.clock().elapsed(), before, "the loop moved no time");
}

/// **A runtime that sees no window lists none of the next turn's causes, only its deadlines.**
/// With its one window covered the runtime runs no frame for a redraw, a scheduled frame, tree
/// work or a running animation, so it lists none of them; a task's sleep is still a timer. Seen
/// again, the animation is listed again.
///
/// Fails against a list that names the next turn's causes while frames are off: the host's loop
/// would wake for a frame nothing runs.
#[test]
fn a_runtime_that_sees_no_window_lists_its_deadlines_alone() {
    use UnsettledReason::{FrameScheduled, NeedsRedraw, PendingWork, Ticker, Timer};

    let number = SharedSignal::new(10);
    let mut served = Served::mounted(
        HeadlessWindow::new(40, 40).with_occlusion_reports(),
        &Column::new((
            Sleeper::new(Duration::from_secs(60)),
            ActivityIndicator::new(),
            Shown {
                number: number.clone(),
            },
        )),
    );
    // The task spawned at the mount sleeps once polled.
    let _ = served.host.pump(FRAME);
    let reasons = |host: &HeadlessHost| {
        host.unsettled()
            .causes()
            .map(|cause| cause.reason)
            .collect::<Vec<_>>()
    };
    let seen = reasons(&served.host);
    assert!(
        seen.contains(&Ticker) && seen.contains(&Timer),
        "a seen window's animation and the task's sleep: {seen:?}"
    );

    let window = served.host.primary_window();
    served.host.set_window_occluded(window, true);
    let _ = served.host.pump(FRAME);
    number.set(12);
    served.host.request_frame();
    let covered = reasons(&served.host);
    assert!(
        [NeedsRedraw, FrameScheduled, PendingWork, Ticker]
            .iter()
            .all(|reason| !covered.contains(reason)),
        "no turn's cause while no window is seen: {covered:?}"
    );
    assert!(covered.contains(&Timer), "the sleep still is: {covered:?}");

    served.host.set_window_occluded(window, false);
    let uncovered = reasons(&served.host);
    assert!(
        uncovered.contains(&Ticker),
        "seen again, the animation is listed: {uncovered:?}"
    );
    served.kept.push(Box::new(number));
}
