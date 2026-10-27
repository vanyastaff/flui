//! An application handler for the loops' and features' tests, which records
//! what it hears and answers what it is asked, and a delivery bound to one.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice};

use super::AppDelivery;
use crate::error::BootstrapError;
use crate::owner::signal::OwnerSignal;
use crate::owner::{DirectOwnerHooks, OwnerHooks, OwnerPlatform};
use crate::platform::{AppHandler, Platform};

/// One call a [`RecordingApp`] heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AppCall {
    Ready,
    Quit,
    Reopen,
    SessionEnd(SessionEndPhase),
    Notice(SystemNotice),
    ExitPolicy,
    WakeDeadline,
}

/// What a [`RecordingApp`] does on each call, after recording it.
type Reaction = Rc<dyn Fn(AppCall, &OwnerPlatform)>;

/// Records every call and answers with what it was given.
pub(crate) struct RecordingApp {
    calls: RefCell<Vec<AppCall>>,
    reaction: RefCell<Option<Reaction>>,
    session_end: Cell<SessionEndAnswer>,
    exit: Cell<bool>,
    deadline: RefCell<Rc<dyn Fn() -> Option<web_time::Instant>>>,
}

impl RecordingApp {
    /// A handler that answers `Proceed`, allows the exit and asks for no
    /// deadline.
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            calls: RefCell::new(Vec::new()),
            reaction: RefCell::new(None),
            session_end: Cell::new(SessionEndAnswer::Proceed),
            exit: Cell::new(true),
            deadline: RefCell::new(Rc::new(|| None)),
        })
    }

    /// What it heard, in order.
    pub(crate) fn calls(&self) -> Vec<AppCall> {
        self.calls.borrow().clone()
    }

    /// The notices it heard, in order.
    #[cfg(windows)]
    pub(crate) fn notices(&self) -> Vec<SystemNotice> {
        self.calls
            .borrow()
            .iter()
            .filter_map(|call| match call {
                AppCall::Notice(notice) => Some(*notice),
                _ => None,
            })
            .collect()
    }

    /// Run `reaction` on every later call, after recording it.
    pub(crate) fn react(&self, reaction: impl Fn(AppCall, &OwnerPlatform) + 'static) {
        self.reaction.replace(Some(Rc::new(reaction)));
    }

    /// Answer a session end's query with `answer`.
    pub(crate) fn answer_session_end(&self, answer: SessionEndAnswer) {
        self.session_end.set(answer);
    }

    /// Answer the exit policy with `allow`.
    pub(crate) fn answer_exit(&self, allow: bool) {
        self.exit.set(allow);
    }

    /// Answer the wake deadline with `deadline`.
    pub(crate) fn answer_deadline(&self, deadline: Option<web_time::Instant>) {
        self.answer_deadline_with(move || deadline);
    }

    /// Answer the wake deadline with what `answer` says when asked.
    pub(crate) fn answer_deadline_with(
        &self,
        answer: impl Fn() -> Option<web_time::Instant> + 'static,
    ) {
        self.deadline.replace(Rc::new(answer));
    }

    fn heard(&self, call: AppCall, owner: &OwnerPlatform) {
        self.calls.borrow_mut().push(call);
        let reaction = self.reaction.borrow().clone();
        if let Some(reaction) = reaction {
            reaction(call, owner);
        }
    }
}

impl AppHandler for RecordingApp {
    fn ready(&self, owner: OwnerPlatform) -> Result<(), BootstrapError> {
        self.heard(AppCall::Ready, &owner);
        Ok(())
    }

    fn quit_requested(&self, owner: &OwnerPlatform) {
        self.heard(AppCall::Quit, owner);
    }

    fn reopen(&self, owner: &OwnerPlatform) {
        self.heard(AppCall::Reopen, owner);
    }

    fn session_end(&self, owner: &OwnerPlatform, phase: SessionEndPhase) -> SessionEndAnswer {
        self.heard(AppCall::SessionEnd(phase), owner);
        self.session_end.get()
    }

    fn host_notice(&self, owner: &OwnerPlatform, notice: SystemNotice) {
        self.heard(AppCall::Notice(notice), owner);
    }

    fn exit_policy(&self, owner: &OwnerPlatform) -> bool {
        self.heard(AppCall::ExitPolicy, owner);
        self.exit.get()
    }

    fn wake_deadline(&self, owner: &OwnerPlatform) -> Option<web_time::Instant> {
        self.heard(AppCall::WakeDeadline, owner);
        let answer = self.deadline.borrow().clone();
        answer()
    }
}

/// A handler whose bootstrap is `ready` and that has nothing to say about
/// anything else: it ignores reports, lets a session end proceed, keeps the
/// loop running when its last window closes, and asks for no deadline. The
/// tests' stand-in for `run(on_ready)`.
pub(crate) fn on_ready(
    ready: impl FnOnce(OwnerPlatform) -> Result<(), BootstrapError> + 'static,
) -> Rc<dyn AppHandler> {
    struct OnReady<F>(Cell<Option<F>>);
    impl<F: FnOnce(OwnerPlatform) -> Result<(), BootstrapError> + 'static> AppHandler for OnReady<F> {
        fn ready(&self, owner: OwnerPlatform) -> Result<(), BootstrapError> {
            let ready = self.0.take().expect("BUG: a run bootstraps once");
            ready(owner)
        }
        fn quit_requested(&self, _: &OwnerPlatform) {}
        fn reopen(&self, _: &OwnerPlatform) {}
        fn session_end(&self, _: &OwnerPlatform, _: SessionEndPhase) -> SessionEndAnswer {
            SessionEndAnswer::Proceed
        }
        fn host_notice(&self, _: &OwnerPlatform, _: SystemNotice) {}
        fn exit_policy(&self, _: &OwnerPlatform) -> bool {
            false
        }
        fn wake_deadline(&self, _: &OwnerPlatform) -> Option<web_time::Instant> {
            None
        }
    }
    Rc::new(OnReady(Cell::new(Some(ready))))
}

/// A delivery bound to a handler on this thread over a headless platform, and
/// the platform and owner hooks the delivery holds weakly.
pub(crate) struct Bound {
    pub(crate) delivery: Arc<AppDelivery>,
    pub(crate) platform: Arc<dyn Platform>,
    pub(crate) hooks: Arc<dyn OwnerHooks>,
}

impl Bound {
    /// The owner capability `ready` is handed.
    pub(crate) fn owner(&self) -> OwnerPlatform {
        OwnerPlatform::new(Arc::clone(&self.platform), Arc::clone(&self.hooks))
    }
}

/// `delivery` bound to `handler` on this thread, over a headless platform.
pub(crate) fn bind(delivery: Arc<AppDelivery>, handler: Rc<dyn AppHandler>) -> Bound {
    let platform: Arc<dyn Platform> = Arc::new(crate::HeadlessPlatform::new());
    let signal = OwnerSignal::new(Arc::new(|| Ok(())));
    let hooks: Arc<dyn OwnerHooks> =
        Arc::new(DirectOwnerHooks::with_signal(Arc::clone(&platform), signal));
    delivery.bind(handler, &platform, &hooks);
    Bound {
        delivery,
        platform,
        hooks,
    }
}

/// A new delivery bound to `handler`, whose `ready` has run.
pub(crate) fn bound(handler: Rc<dyn AppHandler>) -> Bound {
    let run = bind(Arc::new(AppDelivery::new()), handler);
    run.delivery
        .ready(run.owner())
        .expect("the recording handler's bootstrap succeeds");
    run
}
