//! Application handlers to run a platform with in a test: one that only
//! bootstraps, and a log that records every call and answers what it was told
//! to.

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice};

use crate::error::BootstrapError;
use crate::owner::OwnerPlatform;
use crate::platform::{AppHandler, ExitAnswer, Platform};

/// A handler whose bootstrap is `ready` and that has nothing to say about
/// anything else: it ignores reports, lets a session end proceed, keeps the
/// loop running when its last window closes, and asks for no deadline.
pub fn on_ready(
    ready: impl FnOnce(OwnerPlatform) -> Result<(), BootstrapError> + 'static,
) -> Rc<dyn AppHandler> {
    let log = AppLog::new();
    log.bootstrap(ready);
    log
}

/// A log whose bootstrap is `ready`, which is handed the log too, to set
/// what it answers from then on.
pub fn with_log(
    ready: impl FnOnce(&AppLog, OwnerPlatform) -> Result<(), BootstrapError> + 'static,
) -> Rc<dyn AppHandler> {
    let log = AppLog::new();
    let weak = Rc::downgrade(&log);
    log.bootstrap(move |owner| {
        let log = weak.upgrade().expect("BUG: the log runs its own bootstrap");
        ready(&log, owner)
    });
    log
}

/// Run `platform` with `log` as its handler and hand back the owner
/// capability its bootstrap received: for a host whose `run` returns once the
/// bootstrap has (headless, the web).
///
/// # Panics
/// When the platform does not run, or runs without bootstrapping.
pub fn run_keeping_owner(platform: Box<dyn Platform>, log: &Rc<AppLog>) -> OwnerPlatform {
    let kept = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&kept);
    log.bootstrap(move |owner| {
        keep.replace(Some(owner));
        Ok(())
    });
    platform
        .run(Rc::clone(log) as Rc<dyn AppHandler>)
        .expect("the platform runs");
    kept.take().expect("the bootstrap received the owner")
}

/// One call an [`AppLog`] heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppCall {
    /// [`AppHandler::ready`].
    Ready,
    /// [`AppHandler::quit_requested`].
    Quit,
    /// [`AppHandler::reopen`].
    Reopen,
    /// [`AppHandler::session_end`] with its phase.
    SessionEnd(SessionEndPhase),
    /// [`AppHandler::host_notice`] with its notice.
    Notice(SystemNotice),
    /// [`AppHandler::exit_policy`].
    ExitPolicy,
    /// [`AppHandler::wake_deadline`].
    WakeDeadline,
}

type Bootstrap = Box<dyn FnOnce(OwnerPlatform) -> Result<(), BootstrapError>>;
type Reaction = Rc<dyn Fn(AppCall, &OwnerPlatform)>;

/// Records every call, runs what it was told to on each, and answers with
/// what it was given: `Proceed`, keep running and no deadline until told
/// otherwise.
pub struct AppLog {
    calls: RefCell<Vec<AppCall>>,
    bootstrap: RefCell<Option<Bootstrap>>,
    reaction: RefCell<Option<Reaction>>,
    session_end: RefCell<Rc<dyn Fn(SessionEndPhase) -> SessionEndAnswer>>,
    exit: RefCell<Rc<dyn Fn() -> ExitAnswer>>,
    deadline: RefCell<Rc<dyn Fn() -> Option<web_time::Instant>>>,
}

impl AppLog {
    /// A log with no bootstrap and the default answers.
    #[must_use]
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            calls: RefCell::new(Vec::new()),
            bootstrap: RefCell::new(None),
            reaction: RefCell::new(None),
            session_end: RefCell::new(Rc::new(|_| SessionEndAnswer::Proceed)),
            exit: RefCell::new(Rc::new(|| ExitAnswer::KeepRunning)),
            deadline: RefCell::new(Rc::new(|| None)),
        })
    }

    /// Run `ready` as the bootstrap, after recording it.
    pub fn bootstrap(
        &self,
        ready: impl FnOnce(OwnerPlatform) -> Result<(), BootstrapError> + 'static,
    ) {
        self.bootstrap.replace(Some(Box::new(ready)));
    }

    /// Run `reaction` on every later call, after recording it.
    pub fn react(&self, reaction: impl Fn(AppCall, &OwnerPlatform) + 'static) {
        self.reaction.replace(Some(Rc::new(reaction)));
    }

    /// Answer a session end with `answer`.
    pub fn answer_session_end(&self, answer: SessionEndAnswer) {
        self.answer_session_end_with(move |_| answer);
    }

    /// Answer a session end with what `answer` says of its phase.
    pub fn answer_session_end_with(
        &self,
        answer: impl Fn(SessionEndPhase) -> SessionEndAnswer + 'static,
    ) {
        self.session_end.replace(Rc::new(answer));
    }

    /// Answer the exit policy: [`ExitAnswer::End`] when `allow`, else
    /// [`ExitAnswer::KeepRunning`].
    pub fn answer_exit(&self, allow: bool) {
        self.answer_exit_with(move || allow);
    }

    /// Answer the exit policy with `answer`.
    pub fn answer_exit_policy(&self, answer: ExitAnswer) {
        self.exit.replace(Rc::new(move || answer));
    }

    /// Answer the exit policy with what `allow` says when asked:
    /// [`ExitAnswer::End`] or [`ExitAnswer::KeepRunning`].
    pub fn answer_exit_with(&self, allow: impl Fn() -> bool + 'static) {
        self.exit.replace(Rc::new(move || {
            if allow() {
                ExitAnswer::End
            } else {
                ExitAnswer::KeepRunning
            }
        }));
    }

    /// Answer the wake deadline with `deadline`.
    pub fn answer_deadline(&self, deadline: Option<web_time::Instant>) {
        self.answer_deadline_with(move || deadline);
    }

    /// Answer the wake deadline with what `answer` says when asked.
    pub fn answer_deadline_with(&self, answer: impl Fn() -> Option<web_time::Instant> + 'static) {
        self.deadline.replace(Rc::new(answer));
    }

    /// What it heard, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<AppCall> {
        self.calls.borrow().clone()
    }

    /// How many times it heard `call`.
    #[must_use]
    pub fn count(&self, call: AppCall) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|heard| **heard == call)
            .count()
    }

    /// The notices it heard, in order.
    #[must_use]
    pub fn notices(&self) -> Vec<SystemNotice> {
        self.calls
            .borrow()
            .iter()
            .filter_map(|call| match call {
                AppCall::Notice(notice) => Some(*notice),
                _ => None,
            })
            .collect()
    }

    fn heard(&self, call: AppCall, owner: &OwnerPlatform) {
        self.calls.borrow_mut().push(call);
        let reaction = self.reaction.borrow().clone();
        if let Some(reaction) = reaction {
            reaction(call, owner);
        }
    }
}

impl std::fmt::Debug for AppLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppLog")
            .field("calls", &self.calls.borrow())
            .finish_non_exhaustive()
    }
}

impl AppHandler for AppLog {
    fn ready(&self, owner: OwnerPlatform) -> Result<(), BootstrapError> {
        self.heard(AppCall::Ready, &owner);
        let bootstrap = self.bootstrap.borrow_mut().take();
        bootstrap.map_or(Ok(()), |bootstrap| bootstrap(owner))
    }

    fn quit_requested(&self, owner: &OwnerPlatform) {
        self.heard(AppCall::Quit, owner);
    }

    fn reopen(&self, owner: &OwnerPlatform) {
        self.heard(AppCall::Reopen, owner);
    }

    fn session_end(&self, owner: &OwnerPlatform, phase: SessionEndPhase) -> SessionEndAnswer {
        self.heard(AppCall::SessionEnd(phase), owner);
        let answer = self.session_end.borrow().clone();
        answer(phase)
    }

    fn host_notice(&self, owner: &OwnerPlatform, notice: SystemNotice) {
        self.heard(AppCall::Notice(notice), owner);
    }

    fn exit_policy(&self, owner: &OwnerPlatform) -> ExitAnswer {
        self.heard(AppCall::ExitPolicy, owner);
        let exit = self.exit.borrow().clone();
        exit()
    }

    fn wake_deadline(&self, owner: &OwnerPlatform) -> Option<web_time::Instant> {
        self.heard(AppCall::WakeDeadline, owner);
        let deadline = self.deadline.borrow().clone();
        deadline()
    }
}
