//! [`Headless`]: the connection of a view mounted in process, as `run_app` mounts it, on a
//! virtual clock (ADR-0400 §1).
//!
//! The connection owns a private [`HeadlessHost`] and the window's development agent
//! ([`Observed`]): every call goes to the agent through the window's one input lease, so a
//! scenario's finders, input and conditions are evaluated in the application, as on a device.
//!
//! - A read is one owner turn with no frame ([`HeadlessHost::owner_turn`]): it applies the
//!   commands queued since the last turn and presents nothing, so it does not change the frame
//!   number.
//! - An act or a wait the window holds runs the host's loop until it is answered
//!   ([`HeadlessHost::run_until_answered`]): each turn at the one list's wake, the application's
//!   timers included, so a wait's limit is reached in virtual time. An act ends with an owner
//!   turn that drains what its frames queued.
//! - [`VirtualClock::advance`] moves the clock in frames.

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
use flui_protocol::wait::{Condition, WaitOutcome, WaitParams};
use flui_view::IntoView;
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook, InputLease};

use super::clock::{Advanced, VirtualClock};
use super::report::{TestReport, write_report};
use super::window::{TestWindow, WindowStorage};
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

/// The owner turns the end of a test runs for what the unmount and the release queued, at most.
const RELEASE_TURNS: usize = 16;

/// A headless connection whose window an agent is handed: see the [module docs](self).
#[derive(Debug)]
pub struct Observed {
    /// The window's one input lease, through which every call goes; `None` once the test ended.
    lease: Option<InputLease>,
    /// The window the lease drives: a clone keeps its semantics collected while it lives.
    _window: AgentWindow,
    /// Kept alive so the window keeps its agent: dropping it detaches.
    agent: Option<HeadlessDevAgent>,
}

/// The connection of a view mounted in process: see the [module docs](self).
///
/// `S` is what is attached to the window: [`Observed`], its agent.
pub struct Headless<S = Observed> {
    /// The host; `None` once the end of the test closed its window and retired its runtime.
    host: Option<HeadlessHost>,
    attached: S,
    window: WindowId,
    ledger: DropLedger,
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

impl Headless<Observed> {
    /// Mount `view` in `window`, as `run_app` mounts it: the window is handed to its agent before
    /// the root attaches, and two frames are presented, since semantics publish from the frame
    /// after the agent took the window.
    ///
    /// # Panics
    ///
    /// When a registration of `window` conflicts with another, or a first frame fails.
    pub(crate) fn mount(view: impl IntoView, window: &TestWindow) -> Self {
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
        let (opened, windows): (Sender<AgentWindow>, Receiver<AgentWindow>) = channel();
        let mut agent = HeadlessDevAgent::attach(Windows(opened));
        let run_storage = storage.as_ref().map(|storage| storage.for_run().0);
        let mut host = HeadlessHost::with_application(
            headless,
            run_storage.map(|storage| KitStorage {
                host: std::sync::Arc::new(storage),
                provided: None,
            }),
            window.registrations.clone(),
        )
        .with_dev_agent(&mut agent);
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
        }
    }

    /// The ledger the end of the test checks: every sentinel planted from it must have been
    /// dropped once the root is unmounted and what it held released.
    pub(crate) fn ledger(&self) -> &DropLedger {
        &self.ledger
    }

    /// The window the connection drives.
    pub(crate) fn window_id(&self) -> WindowId {
        self.window
    }

    fn host_mut(&mut self) -> Result<&mut HeadlessHost, CallError> {
        self.host.as_mut().ok_or_else(ended)
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

    /// The end checks of ADR-0400 §7 from the audit on, in order; the settle is the caller's.
    ///
    /// The unmount and the turns after it run the application's code (a `dispose`, a `Drop`): a
    /// panic there is held while the lease's release still runs, and it is handed back, the
    /// first failure, with the checks that would read the tree it left half unmounted not made.
    fn end_checks(&mut self, checks: &EndChecks, unpassed: &mut Vec<UnpassedCheck>) -> FirstPanic {
        if !checks.audit.is_empty() {
            audit(self, &checks.audit, unpassed);
        }
        let mut first = FirstPanic::new();
        // Unmount the root, keeping the window and the runtime, and run what it dropped.
        if let Some(host) = self.host.as_mut() {
            first.contain(|| {
                host.unmount_root();
                let _ = host.pump(Duration::ZERO);
            });
        }
        // Release the lease, which queues the release of what it holds.
        first.retire(self.attached.lease.take());
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
            let mut left = unsettled.causes().peekable();
            if left.peek().is_some() {
                let mut check = Check::new(
                    "left_behind",
                    Outcome::Failed {
                        reason: "the application keeps work once its root is unmounted: a timer, \
                                 a ticker or background work was left behind"
                            .to_owned(),
                    },
                );
                for cause in left {
                    check = check.evidence(format!(
                        "{:?}, cleared by {:?}{}",
                        cause.reason,
                        cause.cleared_by,
                        cause
                            .window
                            .map_or_else(String::new, |window| format!(" in {window}"))
                    ));
                }
                unpassed.extend(UnpassedCheck::try_from_check(check));
            }
        }
        first
    }
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
        match &self.host {
            Some(host) => {
                let mut reading = ClockReading::new(host.clock().elapsed(), true);
                reading.frame = Some(host.sink().submits());
                reading
            }
            None => ClockReading::new(Duration::ZERO, true),
        }
    }

    fn finish(&mut self, checks: &EndChecks) -> Vec<UnpassedCheck> {
        let mut unpassed = Vec::new();
        if self.host.is_none() {
            return unpassed;
        }
        if checks.settle {
            // A cause that repeats keeps the frame at the limit: the checks after it read that
            // frame, and what the unmount does not clear fails `left_behind`.
            if let Err(error) = self.wait(&Condition::Settled, SETTLE_LIMIT) {
                unpassed.extend(UnpassedCheck::try_from_check(Check::new(
                    "settle",
                    Outcome::of_failure(&error),
                )));
            }
        }
        let mut first = self.end_checks(checks, &mut unpassed);
        // Close the window and retire the runtime, then let the agent go.
        first.retire(self.host.take());
        first.retire(self.attached.agent.take());
        if first.is_held() {
            // The panic is the test's failure; what the checks before it found goes to the
            // test's output, since a panic carries no report.
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

impl VirtualClock for Headless<Observed> {
    fn advance(&mut self, by: Duration) -> Result<Advanced, CallError> {
        let host = self.host_mut()?;
        let before = host.sink().submits();
        let mut left = by;
        while !left.is_zero() {
            let step = left.min(FRAME);
            let _ = host.pump(step);
            left = left.saturating_sub(step);
        }
        Ok(Advanced {
            frames: host.sink().submits().saturating_sub(before),
        })
    }
}
