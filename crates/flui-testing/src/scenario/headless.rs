//! [`Headless`]: the connection of a view mounted in process, on a virtual clock (ADR-0400 §1).

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_protocol::audit::AuditRule;
use flui_protocol::client::{CallError, ClockReading, Connection, EndChecks, Performed};
use flui_protocol::error::{ErrorCode, ErrorObject};
use flui_protocol::input::{ActionSequence, InputParams};
use flui_protocol::method::{
    Addressed, AuditA11y, InputInject, InputRelease, WaitUntil, WindowMethod,
};
use flui_protocol::report::{Check, Outcome, UnpassedCheck};
use flui_protocol::session::WindowParams;
use flui_protocol::tree::WindowId;
use flui_protocol::wait::{Condition, UnsettledCause, WaitOutcome, WaitParams};
use flui_view::IntoView;
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook, InputLease};

use super::clock::{Advanced, VirtualClock};
use super::report::{TestReport, write_report};
use super::target::sealed::Ends;
use super::window::{TestWindow, WindowStorage};
use crate::FrameReport;
use crate::drop_ledger::DropLedger;
use crate::host::{HeadlessDevAgent, HeadlessHost, HeadlessWindow, KitStorage};
use crate::storage::MemoryStorage;

/// One frame at 60 Hz: what a tester's `pump` advances, and the frame period the host's loop
/// paces a cause that re-arms at one instant by.
pub const FRAME: Duration = Duration::from_millis(16);

/// How much of the connection's clock a call that the window serves at once may still take to
/// be answered, past what its parameters say the window may hold it for
/// ([`Addressed::holds_for`]).
const ANSWER_LIMIT: Duration = Duration::from_secs(1);

/// How long the end of a test holds out for the application to settle.
pub(crate) const SETTLE_LIMIT: Duration = Duration::from_secs(5);

/// The frames of [`FRAME`] the end of a test with no agent pumps to settle: as many as
/// [`SETTLE_LIMIT`] holds.
const SETTLE_FRAMES: usize = (SETTLE_LIMIT.as_millis() / FRAME.as_millis()) as usize;

/// The owner turns the end of a test runs for what the unmount and the release queued, at most.
const RELEASE_TURNS: usize = 16;

/// What a [`Headless`] connection attaches to its window: the development agent it is handed.
#[derive(Debug)]
pub struct Observed {
    /// The window's one input lease, through which every call goes; `None` once the test ended.
    lease: Option<InputLease>,
    /// The window the lease drives: a clone keeps its semantics collected while it lives.
    _window: AgentWindow,
    /// Kept alive so the window keeps its agent: dropping it detaches.
    agent: Option<HeadlessDevAgent>,
}

/// What a [`Headless`] host with no agent attaches to its window: nothing, the state a cost
/// baseline holds (ADR-0400 §1, ADR-0396 §1).
///
/// Nothing resolves a locator, so `Headless<Unobserved>` is no
/// [`Connection`]: its tester pumps, reads its frame in process
/// ([`Tester::frame`](super::Tester::frame)) and measures
/// ([`Tester::measure`](super::Tester::measure)), and a scenario over a `Connection` does not
/// compile for it. The window collects no semantics, as no assistive technology asked for them.
#[derive(Debug)]
pub struct Unobserved {
    _private: (),
}

/// The connection of a view mounted in process, as `run_app` mounts it, on a virtual clock
/// (ADR-0400 §1).
///
/// The connection owns a private [`HeadlessHost`] and, on [`Observed`], the window's development
/// agent: every call goes to the agent through the window's one input lease, so a scenario's
/// finders, input and conditions are evaluated in the application, as on a device.
///
/// - A read is one owner turn with no frame (`HeadlessHost::owner_turn`): it applies the
///   commands queued since the last turn and presents nothing, so it does not change the frame
///   number.
/// - An act or a wait the window holds runs the host's loop until it is answered
///   ([`HeadlessHost::run_until_answered`]): each turn at the one list's wake, the application's
///   timers included, so a wait's limit is reached in virtual time. An act ends with an owner
///   turn that drains what its frames queued.
/// - [`VirtualClock::advance`] moves the clock in frames, and keeps what they did for
///   [`Frame::report`](crate::frame::Frame::report).
///
/// `S` is what is attached to the window: [`Observed`], its agent, or [`Unobserved`], nothing.
pub struct Headless<S = Observed> {
    /// The host; `None` once the end of the test closed its window and retired its runtime.
    host: Option<HeadlessHost>,
    attached: S,
    window: WindowId,
    ledger: DropLedger,
    /// What the frames of the last [`VirtualClock::advance`] did.
    advanced: FrameReport,
}

impl<S: std::fmt::Debug> std::fmt::Debug for Headless<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Headless")
            .field("window", &self.window)
            .field("attached", &self.attached)
            .field("ended", &self.host.is_none())
            .finish_non_exhaustive()
    }
}

/// Hands the windows the host opens to the connection, over a channel: the hook is `Send`, and
/// the connection reads it on the owner thread.
pub(crate) struct Windows(pub(crate) Sender<AgentWindow>);

impl DevAgentHook for Windows {
    fn attach(&mut self) -> bool {
        true
    }

    fn window_opened(&mut self, window: AgentWindow) {
        // The receiver is the connection's, alive until the mount took the window.
        let _ = self.0.send(window);
    }
}

/// The host `window` describes, with no root attached and no agent.
///
/// # Panics
///
/// When a registration of `window` conflicts with another.
fn open(window: &TestWindow) -> HeadlessHost {
    let mut headless = HeadlessWindow::new(window.width, window.height).with_text_input();
    if let Some(platform) = window.platform {
        headless = headless.with_platform(platform);
    }
    if window.in_browser {
        headless = headless.in_browser();
    }
    if let Some(handler) = window.close_request_handler.clone() {
        headless = headless.with_close_request_handler(handler);
    }
    let storage = match &window.storage {
        WindowStorage::Fresh => Some(MemoryStorage::new()),
        WindowStorage::Given(storage) => Some(storage.clone()),
        WindowStorage::Absent => None,
    };
    let run_storage = storage.as_ref().map(|storage| storage.for_run().0);
    HeadlessHost::with_application(
        headless,
        run_storage.map(|storage| KitStorage {
            host: std::sync::Arc::new(storage),
            provided: None,
        }),
        window.registrations.clone(),
    )
}

impl Headless<Observed> {
    /// Mount `view` in `window`, as `run_app` mounts it: the window is handed to its agent before
    /// the root attaches, and two frames are presented, since semantics publish from the frame
    /// after the agent took the window.
    ///
    /// # Panics
    ///
    /// When a registration of `window` conflicts with another, or a first frame fails.
    pub(crate) fn mount(view: impl IntoView, window: &TestWindow) -> Self {
        let (opened, windows): (Sender<AgentWindow>, Receiver<AgentWindow>) = channel();
        let mut agent = HeadlessDevAgent::attach(Windows(opened));
        let mut host = open(window).with_dev_agent(&mut agent);
        host.attach_root(view)
            .expect("BUG: a fresh host has no root attached");
        let _ = host.pump(FRAME);
        let agent_window = windows
            .try_recv()
            .expect("BUG: a host with an attached agent hands it its window");
        let lease = agent_window
            .input_lease()
            .expect("BUG: a fresh window has an input lease to hand out");
        // The window collects semantics from the frame after its agent took it.
        let _ = host.pump(FRAME);
        Self {
            window: agent_window.id(),
            host: Some(host),
            attached: Observed {
                lease: Some(lease),
                _window: agent_window,
                agent: Some(agent),
            },
            ledger: DropLedger::new(),
            advanced: FrameReport::default(),
        }
    }

    /// The call had no answer: the lease's input is asked to be released, and whether the agent
    /// answered that it did is read, never assumed.
    fn unanswered(&mut self, may_have_run: bool) -> CallError {
        let released = match (&self.attached.lease, self.host.as_mut()) {
            (Some(lease), Some(host)) => lease
                .call::<InputRelease>(WindowParams::new(self.window))
                .ok()
                .and_then(|mut release| {
                    host.owner_turn();
                    release
                        .try_take()
                        .or_else(|| host.run_until_answered(&mut release, FRAME, ANSWER_LIMIT))
                })
                .is_some_and(|released| released.is_ok()),
            _ => false,
        };
        CallError::Unanswered {
            may_have_run,
            released,
        }
    }
}

impl Headless<Unobserved> {
    /// Mount `view` in `window` with no agent attached, and present its first frame.
    ///
    /// # Panics
    ///
    /// When a registration of `window` conflicts with another, or the first frame fails.
    pub(crate) fn mount_unobserved(view: impl IntoView, window: &TestWindow) -> Self {
        let mut host = open(window);
        host.attach_root(view)
            .expect("BUG: a fresh host has no root attached");
        let _ = host.pump(FRAME);
        // The agent names a window by its presentation (`w<n>` on the wire): the same name, so a
        // diagnosis reads alike at both depths.
        let window = WindowId::from_u64(host.ui_runtime().presentation_id().as_u64())
            .expect("BUG: a presentation id packs a non-zero generation");
        Self {
            host: Some(host),
            attached: Unobserved { _private: () },
            window,
            ledger: DropLedger::new(),
            advanced: FrameReport::default(),
        }
    }
}

impl<S> Headless<S> {
    /// The ledger the end of the test checks: every sentinel planted from it must have been
    /// dropped once the root is unmounted and what it held released.
    pub(crate) fn ledger(&self) -> &DropLedger {
        &self.ledger
    }

    /// The window the connection drives.
    pub(crate) fn window_id(&self) -> WindowId {
        self.window
    }

    /// The host, `None` once the end of the test retired it.
    pub(crate) fn host(&self) -> Option<&HeadlessHost> {
        self.host.as_ref()
    }

    /// What the frames of the last [`VirtualClock::advance`] did.
    pub(crate) fn advanced(&self) -> &FrameReport {
        &self.advanced
    }

    pub(crate) fn host_mut(&mut self) -> Result<&mut HeadlessHost, CallError> {
        self.host.as_mut().ok_or_else(ended)
    }

    fn reading(&self) -> ClockReading {
        match &self.host {
            Some(host) => {
                let mut reading = ClockReading::new(host.clock().elapsed(), true);
                reading.frame = Some(host.sink().submits());
                reading
            }
            None => ClockReading::new(Duration::ZERO, true),
        }
    }

    /// The end checks of ADR-0400 §7 from the unmount on, in order; the settle and the audit
    /// are the caller's, contained in `first`.
    ///
    /// The settle, the audit, the unmount and the turns after it run the application's code (a
    /// timer, a `dispose`, a `Drop`): a panic in any of them is held in `first` while the unmount
    /// and `lease`'s release still run, and it is handed back, the first failure, with the checks
    /// that would read the tree it left half unmounted not made.
    fn teardown(
        &mut self,
        mut first: FirstPanic,
        lease: Option<InputLease>,
        checks: &EndChecks,
        unpassed: &mut Vec<UnpassedCheck>,
    ) -> FirstPanic {
        // Unmount the root, keeping the window and the runtime, and run what it dropped.
        if let Some(host) = self.host.as_mut() {
            first.contain(|| {
                host.unmount_root();
                let _ = host.pump(Duration::ZERO);
            });
        }
        // Release the lease, which queues the release of what it holds.
        first.retire(lease);
        if let Some(host) = self.host.as_mut() {
            first.contain(|| {
                for _ in 0..RELEASE_TURNS {
                    host.owner_turn();
                    if host.time_to_next_frame() != Some(Duration::ZERO) {
                        break;
                    }
                    let _ = host.pump(Duration::ZERO);
                }
            });
        }
        if first.is_held() {
            return first;
        }
        if checks.drop_ledger && self.ledger.live() > 0 {
            unpassed.extend(UnpassedCheck::try_from_check(Check::new(
                "drop_ledger",
                Outcome::Failed {
                    reason: format!(
                        "{} of {} values planted in the tester's ledger were not dropped",
                        self.ledger.live(),
                        self.ledger.planted()
                    ),
                },
            )));
        }
        if checks.left_behind
            && let Some(host) = &self.host
        {
            let unsettled = host.unsettled();
            if !unsettled.is_empty() {
                let check = Check::new(
                    "left_behind",
                    Outcome::Failed {
                        reason: "the application keeps work once its root is unmounted: a timer, \
                                 a ticker or background work was left behind"
                            .to_owned(),
                    },
                );
                unpassed.extend(UnpassedCheck::try_from_check(naming(
                    check,
                    unsettled.causes(),
                )));
            }
        }
        first
    }

    /// The last of the end: close the window and retire the runtime, then let `agent` go; a panic
    /// held from the teardown is the test's failure, and what the checks before it found goes to
    /// the test's output, since a panic carries no report.
    fn conclude(
        &mut self,
        mut first: FirstPanic,
        agent: Option<HeadlessDevAgent>,
        unpassed: Vec<UnpassedCheck>,
    ) -> Vec<UnpassedCheck> {
        first.retire(self.host.take());
        first.retire(agent);
        if first.is_held() {
            write_report(&TestReport {
                unpassed,
                soft: Vec::new(),
                skipped: None,
            });
            first.resume();
            return Vec::new();
        }
        unpassed
    }
}

/// `check` with each of `causes` as its evidence.
fn naming<'a>(mut check: Check, causes: impl Iterator<Item = &'a UnsettledCause>) -> Check {
    for cause in causes {
        check = check.evidence(format!(
            "{:?}, cleared by {:?}{}",
            cause.reason,
            cause.cleared_by,
            cause
                .window
                .map_or_else(String::new, |window| format!(" in {window}"))
        ));
    }
    check
}

/// `audit.a11y` of the window's settled frame by `rules`: each finding fails the audit, and a
/// rule that cannot run is cannot-verify, never a pass.
fn audit(
    connection: &mut Headless<Observed>,
    rules: &[AuditRule],
    unpassed: &mut Vec<UnpassedCheck>,
) {
    let mut params = flui_protocol::audit::AuditParams::new(connection.window);
    params.rules = rules.to_vec();
    match connection.call::<AuditA11y>(params) {
        Ok(report) => {
            if !report.findings.is_empty() {
                let mut check = Check::new(
                    "audit",
                    Outcome::Failed {
                        reason: format!(
                            "{} element(s) break an accessibility rule",
                            report.findings.len()
                        ),
                    },
                );
                for finding in &report.findings {
                    check = check.evidence(format!(
                        "{:?} at {}: {}",
                        finding.rule, finding.node, finding.detail
                    ));
                }
                unpassed.extend(UnpassedCheck::try_from_check(check));
            }
            unpassed.extend(report.not_checked.iter().map(|skipped| {
                UnpassedCheck::cannot_verify(
                    format!("audit {:?}", skipped.rule),
                    skipped.reason.clone(),
                    None,
                )
            }));
        }
        Err(error) => unpassed.extend(UnpassedCheck::try_from_check(Check::new(
            "audit",
            Outcome::of_failure(&error),
        ))),
    }
}

/// What a call after the end of the test answers.
fn ended() -> CallError {
    CallError::failed(ErrorObject::new(
        ErrorCode::Gone,
        "the test has ended: its window is closed",
    ))
}

impl Connection for Headless<Observed> {
    fn call<M: WindowMethod>(&mut self, mut params: M::Params) -> Result<M::Output, CallError> {
        params.address(self.window);
        let holds = params.holds_for();
        let lease = self.attached.lease.as_ref().ok_or_else(ended)?;
        let mut answer: AgentAnswer<M::Output> =
            lease.call::<M>(params).map_err(CallError::failed)?;
        let host = self.host_mut()?;
        host.owner_turn();
        // A read the turn answered runs no frame (ADR-0400 §4).
        let answered = answer.try_take().or_else(|| {
            let answered =
                host.run_until_answered(&mut answer, FRAME, holds.saturating_add(ANSWER_LIMIT));
            // What the frames that served it queued is drained before the answer is handed
            // back, so a failure the step caused is the step's.
            host.owner_turn();
            answered
        });
        match answered {
            Some(answered) => answered.map_err(CallError::failed),
            None => Err(self.unanswered(M::EFFECT.changes_app())),
        }
    }

    fn wait(&mut self, condition: &Condition, limit: Duration) -> Result<WaitOutcome, CallError> {
        let timeout_ms = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
        self.call::<WaitUntil>(
            WaitParams::new(self.window, condition.clone()).timeout_ms(timeout_ms),
        )
    }

    fn perform(&mut self, actions: &ActionSequence) -> Result<Performed, CallError> {
        self.call::<InputInject>(InputParams::new(self.window, actions.clone()))
            .map(Performed::from)
    }

    fn now(&self) -> ClockReading {
        self.reading()
    }

    fn finish(&mut self, checks: &EndChecks) -> Vec<UnpassedCheck> {
        let mut unpassed = Vec::new();
        if self.host.is_none() {
            return unpassed;
        }
        // The settle and the audit run frames, so the application's timers and callbacks: a panic
        // in one is held, and the unmount and the lease's release still run.
        let mut first = FirstPanic::new();
        if checks.settle {
            // A cause that repeats keeps the frame at the limit: the checks after it read that
            // frame, and what the unmount does not clear fails `left_behind`.
            if let Some(Err(error)) = first.contain(|| self.wait(&Condition::Settled, SETTLE_LIMIT))
            {
                unpassed.extend(UnpassedCheck::try_from_check(Check::new(
                    "settle",
                    Outcome::of_failure(&error),
                )));
            }
        }
        if !checks.audit.is_empty() && !first.is_held() {
            first.contain(|| audit(self, &checks.audit, &mut unpassed));
        }
        let lease = self.attached.lease.take();
        let first = self.teardown(first, lease, checks, &mut unpassed);
        let agent = self.attached.agent.take();
        self.conclude(first, agent, unpassed)
    }
}

impl Ends for Headless<Unobserved> {
    fn reading(&self) -> ClockReading {
        Headless::reading(self)
    }

    /// The end of ADR-0400 §7 a host with no agent can run: the settle, on the host's own list
    /// and its loop; the unmount; the drops; the ledger; what is left behind. The audit is not
    /// among them, since nothing reads the window's semantics: the tree a cost scenario measures
    /// is audited by the scenario's test over an [`Observed`] connection.
    fn end(&mut self, checks: &EndChecks) -> Vec<UnpassedCheck> {
        let mut unpassed = Vec::new();
        let Some(host) = self.host.as_mut() else {
            return unpassed;
        };
        // The settle runs the application's frames: a panic in one is held, and the unmount
        // still runs.
        let mut first = FirstPanic::new();
        let settled = !checks.settle
            || first
                .contain(|| host.pump_until_settled(FRAME, SETTLE_FRAMES).is_ok())
                .unwrap_or(true);
        if !settled {
            let check = Check::new(
                "settle",
                Outcome::Failed {
                    reason: format!("the application did not settle within {SETTLE_LIMIT:?}"),
                },
            );
            unpassed.extend(UnpassedCheck::try_from_check(naming(
                check,
                host.unsettled().causes(),
            )));
        }
        let first = self.teardown(first, None, checks, &mut unpassed);
        self.conclude(first, None, unpassed)
    }

    fn outline(&mut self, _window: WindowId) -> String {
        "(no agent is attached to the window, so its semantics are not read: read its frame with \
         `Tester::frame`)"
            .to_owned()
    }
}

impl<S> VirtualClock for Headless<S> {
    fn advance(&mut self, by: Duration) -> Result<Advanced, CallError> {
        let host = self.host.as_mut().ok_or_else(ended)?;
        let before = host.sink().submits();
        let mut report = FrameReport::default();
        let mut left = by;
        while !left.is_zero() {
            let step = left.min(FRAME);
            report += &host.pump_reported(step);
            left = left.saturating_sub(step);
        }
        let frames = host.sink().submits().saturating_sub(before);
        self.advanced = report;
        Ok(Advanced { frames })
    }
}
