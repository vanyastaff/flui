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
use flui_runtime::settledness::Unsettled;
use flui_runtime::ui_runtime::DEFAULT_REFRESH_PERIOD;
use flui_scheduler::Instant;
use flui_testing::host::{
    HeadlessDevAgent, HeadlessHost, HeadlessWindow, HeadlessWindowId, UnshownFrame,
};
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

/// A widget that spins an activity indicator while `on` holds, and is still once it does not.
#[derive(Clone, StatefulView)]
struct Spinning {
    on: SharedSignal<bool>,
}

struct SpinningState {
    on: SharedSignal<bool>,
}

impl StatefulView for Spinning {
    type State = SpinningState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        SpinningState {
            on: self.on.clone(),
        }
    }
}

impl ViewState<Spinning> for SpinningState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.on.bind(ctx);
    }

    fn build(&self, _view: &Spinning, ctx: &dyn BuildContext) -> impl IntoView {
        if self.on.get(ctx) {
            ActivityIndicator::new().into_view().boxed()
        } else {
            still().into_view().boxed()
        }
    }
}

/// A host with two windows, `first` the primary's root and `second` the root of a window opened
/// beside it, both served to the agent and pumped once; the second window's id, and both wire
/// ids, the primary's first.
fn two_windows(
    first: &(impl View + Clone + 'static),
    second: &(impl View + Clone + 'static),
) -> (Served, HeadlessWindowId, [WindowId; 2]) {
    let mut served = Served::mounted(HeadlessWindow::new(40, 40), first);
    let other = served.host.open_window(HeadlessWindow::new(40, 40));
    served
        .host
        .attach_to(other, second)
        .expect("a new window has no root");
    served.host.serve_window(other, &mut served.agent);
    let _ = served.host.pump(FRAME);
    let ids = {
        let windows = served.windows.lock();
        let [first, second] = windows.as_slice() else {
            panic!("the host handed the agent both windows: {}", windows.len());
        };
        [first.id(), second.id()]
    };
    (served, other, ids)
}

/// The windows the list names for `reason`, in order.
fn windows_named(unsettled: &Unsettled, reason: UnsettledReason) -> Vec<Option<WindowId>> {
    unsettled
        .causes()
        .filter(|cause| cause.reason == reason)
        .map(|cause| cause.window)
        .collect()
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
/// holding out to the wait's limit, and its loop moves no time. A hold with nothing queued keeps
/// nothing.
///
/// Fails against a host that wakes for held work (the loop spins a turn at a time), that
/// lists it as the next turn's (the wait holds out to its limit), or that lists a hold as work.
#[test]
fn held_background_work_never_wakes_the_loop_and_ends_a_false_wait_idle() {
    let mut served = Served::new(HeadlessWindow::new(40, 40));
    let background = acquire_capability::<BackgroundCapability>(&mut served.host)
        .expect("the headless host runs background work");
    served.host.hold_background();
    assert!(
        served.host.unsettled().is_empty(),
        "a hold with nothing queued keeps nothing: {:?}",
        served.host.unsettled().causes().collect::<Vec<_>>()
    );
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
/// frame sooner, so the redraw, and the host's own cause the next turn serves, are listed as
/// served at the retry's instant, and the loop sleeps until then; at that instant both are the
/// next turn's.
///
/// Fails against a list that keeps the redraw or the host's cause the next turn's while the
/// retry is ahead (the loop wakes at once and the frame waits anyway), or that still holds them
/// at the retry's instant.
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
    // A host cause the next turn serves waits for the retry too.
    host.window()
        .admit_and_apply(WindowCommand::SetFrame(WindowFrame::None))
        .expect("the headless window takes every frame");

    let cleared_by = |host: &HeadlessHost, reason| {
        let listed = host.unsettled();
        listed
            .causes()
            .find(|cause| cause.reason == reason)
            .map(|cause| cause.cleared_by)
            .unwrap_or_else(|| {
                panic!(
                    "{reason:?} is listed: {:?}",
                    listed.causes().collect::<Vec<_>>()
                )
            })
    };
    for reason in [
        UnsettledReason::NeedsRedraw,
        UnsettledReason::WindowObservation,
    ] {
        assert_eq!(cleared_by(&host, reason), ClearedBy::Deadline, "{reason:?}");
    }
    assert_eq!(
        host.time_to_next_frame(),
        Some(DEFAULT_REFRESH_PERIOD),
        "the loop sleeps until the retry"
    );

    // At the retry's instant nothing holds them: the next turn serves them.
    host.advance(DEFAULT_REFRESH_PERIOD);
    for reason in [
        UnsettledReason::NeedsRedraw,
        UnsettledReason::WindowObservation,
    ] {
        assert_eq!(
            cleared_by(&host, reason),
            ClearedBy::NextFrame,
            "{reason:?} at the retry"
        );
    }
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

/// **A window that cannot be seen is named for no frame, whether or not another window
/// animates.** A hidden window whose content changed keeps that work until it is seen: no frame
/// of the other window serves it. While a second, seen window animates and once it has stopped,
/// the list names no cause with the hidden window and its `wait.settled` is satisfied alike, as
/// with every window hidden; seen again, it is named.
///
/// Fails against a list that names each presentation with work for a frame: while the other
/// window animates, its redraw names the hidden window too, whose wait then holds out on a
/// next-frame cause no frame serves, and settles only once the animation stops.
#[test]
fn a_hidden_window_is_named_for_no_frame_whether_or_not_another_window_animates() {
    let number = SharedSignal::new(10);
    let spinning = SharedSignal::new(true);
    let (mut served, _, [hidden_id, animating_id]) = two_windows(
        &Shown {
            number: number.clone(),
        },
        &Spinning {
            on: spinning.clone(),
        },
    );
    let hidden = served.host.primary_window();
    served.host.set_window_occluded(hidden, true);
    let _ = served.host.pump(FRAME);
    number.set(12);
    let _ = served.host.pump(FRAME);
    let hidden_window = served.windows.lock()[0].clone();
    let answer_on_hidden = |served: &mut Served, phase: &str| {
        let named: Vec<_> = served
            .host
            .unsettled()
            .causes()
            .filter(|cause| cause.window == Some(hidden_id))
            .cloned()
            .collect();
        assert!(
            named.is_empty(),
            "{phase}: the hidden window is named: {named:?}"
        );
        let mut answer = hidden_window
            .call::<WaitSettled>(settled_params(hidden_id, 0))
            .expect("the inbox has room");
        let _ = served.host.pump(Duration::ZERO);
        let outcome = answer
            .try_take()
            .expect("the pump answered the hidden window's wait")
            .expect("a wait is an answer");
        assert_eq!(outcome.end, WaitEnd::Satisfied, "{phase}: {outcome:?}");
    };

    assert!(
        windows_named(&served.host.unsettled(), UnsettledReason::Ticker)
            .contains(&Some(animating_id)),
        "the other window animates"
    );
    answer_on_hidden(&mut served, "while the other window animates");

    spinning.set(false);
    served
        .host
        .pump_until_settled(FRAME, 16)
        .expect("the other window stops, and the hidden window's work holds nothing");
    answer_on_hidden(&mut served, "once the other window stopped");

    served.host.set_window_occluded(hidden, false);
    assert!(
        served
            .host
            .unsettled()
            .causes()
            .any(|cause| cause.window == Some(hidden_id)),
        "seen again, the window is named for its frame"
    );
    served.kept.push(Box::new((number, spinning)));
}

/// Which window a cause of a frame's demand names.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Named {
    /// The window whose state lists it.
    Other,
    /// No window: the whole application's.
    Application,
}

/// **A frame's demand names the window whose work it is, beside a still window; due work names
/// the application.** One row per kind of demand, and the host's window observation, each with
/// a still primary window and a second window: the cause names the second window alone, never
/// the still one, or, for due work, no window.
///
/// Fails against tree work or a scheduled frame listed with no window (a still window's wait
/// would hold out on another window's change), against motion counted only where every window
/// holds some (the other window's motion is not listed), or against due work named for the
/// windows with frame work (the still window's wait would not see a task due now), or against a
/// host that lists a window's observation for no window.
#[test]
fn a_frame_s_demand_names_the_window_whose_work_it_is() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerKind, make_move_event_for_id};
    use flui_interaction::ids::PointerId;
    use flui_platform::PlatformInput;

    struct Row {
        name: &'static str,
        reason: UnsettledReason,
        named: Named,
        /// The host in the state that lists `reason`, and both wire ids, the still one's first.
        state: fn() -> (Served, [WindowId; 2]),
    }
    fn changed() -> (Served, [WindowId; 2]) {
        let number = SharedSignal::new(10);
        let (mut served, _, ids) = two_windows(
            &still(),
            &Shown {
                number: number.clone(),
            },
        );
        served
            .host
            .pump_until_settled(FRAME, 8)
            .expect("both windows settle");
        number.set(12);
        served.kept.push(Box::new(number));
        (served, ids)
    }
    let rows = [
        Row {
            name: "the other window's tree work",
            reason: UnsettledReason::PendingWork,
            named: Named::Other,
            state: changed,
        },
        Row {
            name: "the frame the other window's change scheduled",
            reason: UnsettledReason::FrameScheduled,
            named: Named::Other,
            state: changed,
        },
        Row {
            name: "motion the other window holds for its frame",
            reason: UnsettledReason::PendingWork,
            named: Named::Other,
            state: || {
                let (mut served, other, ids) = two_windows(&still(), &still());
                served
                    .host
                    .set_pointer_resampling(
                        other,
                        flui_runtime::presentation::PointerResampling::FrameAligned,
                    )
                    .expect("no contact holds the policy");
                served
                    .host
                    .pump_until_settled(FRAME, 8)
                    .expect("both windows settle");
                let mouse = PointerId::try_from(1).expect("nonzero pointer id");
                served.host.dispatch_to(
                    other,
                    PlatformInput::Pointer(
                        make_move_event_for_id(mouse, Offset::new(5.0, 5.0), PointerKind::Mouse)
                            .expect("move"),
                    ),
                );
                (served, ids)
            },
        },
        Row {
            name: "an observation of the other window not yet delivered",
            reason: UnsettledReason::WindowObservation,
            named: Named::Other,
            state: || {
                let (mut served, other, ids) = two_windows(&still(), &still());
                served
                    .host
                    .pump_until_settled(FRAME, 8)
                    .expect("both windows settle");
                served
                    .host
                    .window_of(other)
                    .admit_and_apply(WindowCommand::SetFrame(WindowFrame::None))
                    .expect("the headless window takes every frame");
                (served, ids)
            },
        },
        Row {
            name: "a task due while the other window animates",
            reason: UnsettledReason::PendingWork,
            named: Named::Application,
            state: || {
                let (mut served, _, ids) = two_windows(
                    &Sleeper::new(Duration::from_secs(1)),
                    &ActivityIndicator::new(),
                );
                // The task spawned at the mount sleeps once polled.
                let _ = served.host.pump(FRAME);
                served.host.advance(Duration::from_secs(1));
                (served, ids)
            },
        },
    ];
    for row in rows {
        let (served, [still_id, other_id]) = (row.state)();
        let unsettled = served.host.unsettled();
        let named = windows_named(&unsettled, row.reason);
        let expected = match row.named {
            Named::Other => Some(other_id),
            Named::Application => None,
        };
        assert!(
            named.contains(&expected),
            "{}: {:?} names {:?}: {named:?}",
            row.name,
            row.reason,
            row.named
        );
        assert!(
            !named.contains(&Some(still_id)),
            "{}: the still window is not named: {named:?}",
            row.name
        );
        if row.named == Named::Other {
            assert!(
                !named.contains(&None),
                "{}: not the whole application's: {named:?}",
                row.name
            );
        }
    }
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

/// **A covered window's frame request wakes the loop at the instant the window was asked for.**
/// The host lists the request at that instant exactly, so its loop's wait ends where the window
/// serves the frame, not a nanosecond after.
///
/// Fails against a host that converts the window's instant to the list's with any offset.
#[test]
fn a_covered_window_s_frame_request_wakes_the_loop_at_its_instant() {
    let served = covered_with_a_change();
    let asked = served
        .host
        .window()
        .timed_frame_requests()
        .last()
        .copied()
        .flatten()
        .expect("the covered window was asked for its frame");
    let wait = served
        .host
        .time_to_next_frame()
        .expect("the request wakes the loop");
    assert_eq!(
        served.host.clock().elapsed() + wait,
        Duration::from_nanos(asked.as_nanos()),
        "the loop wakes at the window's instant"
    );
}

/// **A parked wait and a held act each wake the loop, the sooner first.** The runtime's half
/// carries the earlier of the two deadlines, whichever of them it is, as the list's next
/// deadline; neither is a cause.
///
/// Fails against a runtime that hands its host the later of the agent's deadlines: the loop
/// sleeps past the sooner one.
#[test]
fn a_parked_wait_and_a_held_act_wake_the_loop_at_the_sooner_deadline() {
    use flui_protocol::input::TapParams;
    use flui_protocol::locator::Target;
    use flui_protocol::method::ElementTap;

    struct Row {
        wait_ms: u64,
        act_ms: u64,
    }
    for row in [
        Row {
            wait_ms: 100,
            act_ms: 1_000,
        },
        Row {
            wait_ms: 1_000,
            act_ms: 100,
        },
    ] {
        let mut served = Served::mounted(HeadlessWindow::new(40, 40), &still());
        served
            .host
            .pump_until_settled(FRAME, 8)
            .expect("a still root settles");
        let lease = served
            .window()
            .input_lease()
            .expect("an open window leases its input");
        let _wait = served.wait_until(missing(), row.wait_ms);
        let mut tap = TapParams::new(Target::Locator(Locator::named("Missing")));
        tap.options.timeout_ms = Some(row.act_ms);
        let _act = lease.call::<ElementTap>(tap).expect("the inbox has room");
        let _ = served.host.pump(Duration::ZERO);

        let unsettled = served.host.unsettled();
        assert_eq!(
            unsettled
                .next_deadline()
                .map(|due| due.saturating_duration_since(unsettled.at())),
            Some(Duration::from_millis(100)),
            "wait {} ms, act {} ms: the sooner deadline",
            row.wait_ms,
            row.act_ms
        );
    }
}

/// **`run_until_answered` counts turns at one instant only.** An animation moves the clock a
/// frame at every turn, so a request that outlasts more turns than the bound at one instant is
/// held out to its limit of virtual time.
///
/// Fails against a loop that counts every turn toward the bound, whether the clock moved or
/// not: it stops after the bound's turns, short of the limit.
#[test]
fn run_until_answered_counts_turns_at_one_instant_only() {
    let mut served = Served::mounted(HeadlessWindow::new(4, 4), &ActivityIndicator::new());
    let mut elsewhere = Served::mounted(HeadlessWindow::new(4, 4), &still());
    let mut request = elsewhere.wait_settled(60_000);
    let frame = Duration::from_millis(1);
    let turns = u64::from(flui_runtime::settledness::TURNS_AT_ONE_INSTANT) + 50;
    let limit = frame * u32::try_from(turns).expect("the turns fit");
    let started = served.host.clock().elapsed();
    assert!(
        served
            .host
            .run_until_answered(&mut request, frame, limit)
            .is_none(),
        "another host's request is never answered here"
    );
    assert_eq!(
        served.host.clock().elapsed().saturating_sub(started),
        limit,
        "held out to its limit, past the bound's turns"
    );
    let _ = elsewhere.host.pump(Duration::ZERO);
}

/// **A drag resting over a spring-loaded target lists the spring's deadline.** The runtime wakes
/// for it, so its half names it: a `Timer` of the whole application, due at the spring's delay.
///
/// Fails against a list that leaves the drag's deadline out: the host's loop would sleep past
/// the spring.
#[test]
fn a_drag_over_a_spring_loaded_target_lists_the_spring_s_deadline() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::drag::{DragItem, SpringLoading};
    use flui_interaction::events::{PointerKind, make_down_event_for_id, make_move_event_for_id};
    use flui_interaction::ids::PointerId;
    use flui_platform::PlatformInput;
    use flui_platform::transfer::data::TransferActions;
    use flui_widgets::{
        CommandAppearance, DragCommands, Draggable, DropCommands, DropTarget, Text,
    };

    let card = Draggable::try_new(
        DragCommands::new(CommandAppearance::new("Cut card", Text::new("x"))),
        || vec![DragItem::new().with_value("card").with_text("card")],
        SizedBox::new(200.0, 40.0).child(Text::new("card")),
    )
    .expect("the card's label names it");
    let target = DropTarget::try_new(
        DropCommands::new(CommandAppearance::new("Paste into target", Text::new("v"))),
        |drag| drag.value::<&'static str>().map(|_| TransferActions::MOVE),
        |_hover| {
            SizedBox::new(200.0, 40.0)
                .child(Text::new("target"))
                .into_view()
                .boxed()
        },
    )
    .expect("the target's label names it")
    .spring_loaded(
        SpringLoading {
            delay: Duration::from_millis(300),
        },
        |_ctx, _drag| {},
    );
    let mut served = Served::mounted(HeadlessWindow::new(200, 100), &Column::new((card, target)));
    served
        .host
        .pump_until_settled(FRAME, 8)
        .expect("the desk settles");
    let mouse = PointerId::try_from(1).expect("nonzero pointer id");
    let at = |x, y| Offset::new(x, y);
    served.host.dispatch(PlatformInput::Pointer(
        make_down_event_for_id(mouse, at(100.0, 20.0), PointerKind::Mouse).expect("down"),
    ));
    for y in [35.0, 60.0] {
        served.host.dispatch(PlatformInput::Pointer(
            make_move_event_for_id(mouse, at(100.0, y), PointerKind::Mouse).expect("move"),
        ));
        let _ = served.host.pump(FRAME);
    }

    let listed = served.host.unsettled();
    assert!(
        listed
            .causes()
            .any(|cause| cause.reason == UnsettledReason::Timer
                && cause.window.is_none()
                && cause.due_in_ms.is_some_and(|due| due > 0 && due <= 300)),
        "the spring's deadline: {:?}",
        listed.causes().collect::<Vec<_>>()
    );
}
