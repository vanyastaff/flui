//! Where the macOS loop is in its life, and the reopen requests it owes the
//! application: the state the loop control keeps on the main thread, with no
//! AppKit in it.
//!
//! Every transition takes the state by `&mut` and answers what the control
//! does next, so the control keeps it in a `Cell` and runs the application's
//! code only after the transition has been stored: a callback that re-enters
//! the control reads the state it left, and no write made inside it is lost.
//!
//! `accepting` is whether the owner signal still admits work: it is fenced by
//! an explicit quit before the control is asked, and closed once the loop
//! stops.

#![forbid(unsafe_code)]

use crate::owner::app_delivery::AppDelivery;
use crate::owner::signal::OwnerSignal;

/// Where the loop is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Made, not yet installed as `NSApplication`'s delegate.
    Dormant,
    /// Installed; `NSApplication::run` has not been entered.
    Starting,
    /// Inside `NSApplication::run`.
    Running,
    /// Stopping: no window is admitted and no request is queued.
    Stopping,
    /// Stopped: the windows are closed and the application was told.
    Stopped,
}

/// The loop's phase, its queued exit evaluation and its owed reopens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoopState {
    phase: Phase,
    /// An exit evaluation is queued on the main queue.
    queued: bool,
    /// An exit evaluation was asked for and not yet run.
    requested: bool,
    /// The application asked to quit: the next evaluation stops the loop.
    explicit_quit: bool,
    /// Reopens AppKit reported and the application has not heard yet.
    pending_reopens: usize,
    /// A drain of the pending reopens is queued on the main queue.
    reopen_queued: bool,
    /// A drain is running: the application is hearing a reopen, which can pump
    /// a nested AppKit loop that reports another.
    reopen_active: bool,
    /// `applicationDidFinishLaunching:` has come: what AppKit opens from here
    /// on reached a running application.
    launched: bool,
}

impl LoopState {
    /// A loop made and not yet installed.
    pub(crate) const fn new() -> Self {
        Self {
            phase: Phase::Dormant,
            queued: false,
            requested: false,
            explicit_quit: false,
            pending_reopens: 0,
            reopen_queued: false,
            reopen_active: false,
            launched: false,
        }
    }

    /// Whether the loop is stopping or has stopped.
    pub(crate) const fn stopping(&self) -> bool {
        matches!(self.phase, Phase::Stopping | Phase::Stopped)
    }

    /// Whether `applicationDidFinishLaunching:` has come.
    pub(crate) const fn launched(&self) -> bool {
        self.launched
    }

    /// `applicationDidFinishLaunching:` came.
    pub(crate) fn finish_launching(&mut self) {
        self.launched = true;
    }

    /// Whether a window may open now.
    pub(crate) const fn admits_windows(&self, accepting: bool) -> bool {
        accepting && !self.explicit_quit && !self.stopping()
    }

    /// The control is installed as the application's delegate.
    pub(crate) fn install(&mut self) {
        if self.phase == Phase::Dormant {
            self.phase = Phase::Starting;
        }
    }

    /// AppKit reported a reopen: `true` when it is owed to the application.
    pub(crate) fn accept_reopen(&mut self, accepting: bool) -> bool {
        if !accepting || self.explicit_quit || self.stopping() {
            return false;
        }
        self.pending_reopens = self
            .pending_reopens
            .checked_add(1)
            .expect("BUG: more pending reopen signals than addressable memory");
        true
    }

    /// Whether a drain of the owed reopens is to be queued now; `true` marks
    /// it queued.
    pub(crate) fn queue_reopens(&mut self, accepting: bool) -> bool {
        let queue = self.phase == Phase::Running
            && accepting
            && !self.explicit_quit
            && self.pending_reopens != 0
            && !self.reopen_active
            && !self.reopen_queued;
        self.reopen_queued |= queue;
        queue
    }

    /// The queued drain runs: `true` when it goes on and is now the active
    /// one. A loop that stops owes nothing; a drain already active leaves the
    /// reopens to it.
    pub(crate) fn begin_drain(&mut self, accepting: bool) -> bool {
        self.reopen_queued = false;
        if self.phase != Phase::Running || self.explicit_quit || !accepting {
            self.pending_reopens = 0;
            return false;
        }
        if self.reopen_active {
            return false;
        }
        self.reopen_active = true;
        true
    }

    /// The active drain takes the next reopen: `true` when one is to be told
    /// to the application now. An explicit quit needs no check here: it
    /// empties the owed reopens ([`request`](Self::request)) and refuses new
    /// ones ([`accept_reopen`](Self::accept_reopen)).
    pub(crate) fn take_reopen(&mut self, accepting: bool) -> bool {
        if self.phase != Phase::Running || !accepting {
            self.pending_reopens = 0;
            return false;
        }
        let Some(left) = self.pending_reopens.checked_sub(1) else {
            return false;
        };
        self.pending_reopens = left;
        true
    }

    /// The active drain is over, its callback and the callback's captures
    /// dropped.
    pub(crate) fn end_drain(&mut self) {
        self.reopen_active = false;
    }

    /// An exit evaluation is asked for, by the application (`explicit_quit`)
    /// or because a window closed: `true` when it is to be queued now. A loop
    /// that stops takes none; one not yet running runs it as it starts.
    pub(crate) fn request(&mut self, explicit_quit: bool) -> bool {
        if self.stopping() {
            return false;
        }
        self.requested = true;
        self.explicit_quit |= explicit_quit;
        if explicit_quit {
            self.pending_reopens = 0;
        }
        let queue = self.phase == Phase::Running && !self.queued;
        self.queued |= queue;
        queue
    }

    /// The queued evaluation runs: `Some(explicit_quit)` while the loop runs,
    /// `None` otherwise.
    pub(crate) fn begin_evaluation(&mut self) -> Option<bool> {
        self.queued = false;
        if self.phase != Phase::Running {
            return None;
        }
        self.requested = false;
        Some(self.explicit_quit)
    }

    /// Whether the loop still runs, for an evaluation that asked the
    /// application.
    pub(crate) fn running(&self) -> bool {
        self.phase == Phase::Running
    }

    /// The loop is to stop: `Some(was_running)` when this stops it, `None`
    /// when it already was stopping.
    pub(crate) fn begin_quit(&mut self) -> Option<bool> {
        if self.stopping() {
            return None;
        }
        let running = self.phase == Phase::Running;
        self.phase = Phase::Stopping;
        self.pending_reopens = 0;
        Some(running)
    }

    /// `NSApplication::run` is entered: `Some(requested)` when the loop now
    /// runs, with whether an evaluation was asked for before it did; `None`
    /// when it was not starting (a quit came during bootstrap).
    pub(crate) fn start(&mut self) -> Option<bool> {
        if self.phase != Phase::Starting {
            return None;
        }
        self.phase = Phase::Running;
        Some(self.requested)
    }

    /// The loop is torn down: `true` when this is the first teardown.
    pub(crate) fn begin_finish(&mut self) -> bool {
        if self.phase == Phase::Stopped {
            return false;
        }
        self.phase = Phase::Stopping;
        self.pending_reopens = 0;
        true
    }

    /// The teardown is over.
    pub(crate) fn stopped(&mut self) {
        self.phase = Phase::Stopped;
    }

    /// The reopens still owed to the application.
    #[cfg(test)]
    pub(crate) const fn pending_reopens(&self) -> usize {
        self.pending_reopens
    }
}

/// Store what `body` makes of `cell`'s value and answer what it answers.
/// `body` runs on a copy and must run no code of anyone else's, so no write to
/// the cell can happen while it runs and none is lost.
pub(crate) fn transition<T: Copy, R>(
    cell: &std::cell::Cell<T>,
    body: impl FnOnce(&mut T) -> R,
) -> R {
    let mut value = cell.get();
    let answer = body(&mut value);
    cell.set(value);
    answer
}

/// The loop's state with the owner signal and the application around it: an
/// explicit quit fences the signal before the state takes it, and a drain
/// tells the application each reopen it owes, its state stored first.
///
/// What it decides is the loop control's to schedule: a `true` from
/// [`request`](Self::request) or [`accept_reopen`](Self::accept_reopen) asks
/// for a turn on the main queue.
#[derive(Debug)]
pub(crate) struct LoopRequests {
    state: std::cell::Cell<LoopState>,
}

impl LoopRequests {
    /// A loop made and not yet installed.
    pub(crate) const fn new() -> Self {
        Self {
            state: std::cell::Cell::new(LoopState::new()),
        }
    }

    /// The state as it is now.
    pub(crate) fn get(&self) -> LoopState {
        self.state.get()
    }

    /// Store what `body` makes of the state and answer what it answers
    /// ([`transition`]).
    pub(crate) fn update<R>(&self, body: impl FnOnce(&mut LoopState) -> R) -> R {
        transition(&self.state, body)
    }

    /// Whether a window may open now.
    pub(crate) fn admits_windows(&self, signal: &OwnerSignal) -> bool {
        self.get().admits_windows(signal.accepting())
    }

    /// An exit evaluation is asked for: an explicit quit fences the owner
    /// signal first, so nothing it admits after reaches the application.
    /// `true` when the evaluation is to be queued now.
    pub(crate) fn request(&self, signal: &OwnerSignal, explicit_quit: bool) -> bool {
        if explicit_quit {
            signal.fence();
        }
        self.update(|state| state.request(explicit_quit))
    }

    /// AppKit reported a reopen: `true` when a drain is to be queued now.
    pub(crate) fn accept_reopen(&self, signal: &OwnerSignal) -> bool {
        let accepting = signal.accepting();
        self.update(|state| state.accept_reopen(accepting)) && self.queue_reopens(signal)
    }

    /// Whether a drain of the owed reopens is to be queued now.
    pub(crate) fn queue_reopens(&self, signal: &OwnerSignal) -> bool {
        let accepting = signal.accepting();
        self.update(|state| state.queue_reopens(accepting))
    }

    /// The queued drain: tell `app` each reopen owed while the loop runs and
    /// accepts, then, the drain's callback and its captures dropped (either
    /// can pump a nested AppKit loop that reports another reopen), run
    /// `after`, which queues what arrived meanwhile. A drain that does not
    /// begin runs nothing.
    pub(crate) fn drain_reopens(
        &self,
        signal: &OwnerSignal,
        app: &AppDelivery,
        after: impl FnOnce(),
    ) {
        if !self.update(|state| state.begin_drain(signal.accepting())) {
            return;
        }
        let _active = ActiveDrain {
            requests: self,
            after: Some(after),
        };
        while self.update(|state| state.take_reopen(signal.accepting())) {
            app.reopen().or_contain((), "the application's reopen");
        }
    }
}

/// The drain that runs, until it ends or unwinds: then the state says so,
/// and what follows it runs.
struct ActiveDrain<'a, F: FnOnce()> {
    requests: &'a LoopRequests,
    after: Option<F>,
}

impl<F: FnOnce()> Drop for ActiveDrain<'_, F> {
    fn drop(&mut self) {
        self.requests.update(LoopState::end_drain);
        if let Some(after) = self.after.take() {
            after();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A loop that runs, with the owner signal admitting work.
    fn running() -> LoopState {
        let mut state = LoopState::new();
        state.install();
        assert_eq!(state.start(), Some(false));
        state
    }

    /// The quit flag alone fences the reopens, with the owner signal still
    /// admitting work: a drain queued before the quit does not begin, and one
    /// already running takes no reopen after it.
    #[test]
    fn an_explicit_quit_fences_a_reopen_already_accepted() {
        let mut state = LoopState::new();
        assert!(state.accept_reopen(true));
        // Not running yet: queued as the loop starts, not now.
        assert!(!state.request(true));
        assert!(
            !state.admits_windows(true),
            "an explicit quit admits no window"
        );
        state.install();
        assert_eq!(state.start(), Some(true));
        assert!(!state.begin_drain(true), "the quit flag fences the drain");
        assert_eq!(state.pending_reopens(), 0);

        let mut state = running();
        assert!(state.accept_reopen(true));
        assert!(state.accept_reopen(true));
        assert!(state.queue_reopens(true));
        assert!(state.begin_drain(true));
        assert!(state.take_reopen(true));
        // The application's reopen asks to quit.
        assert!(state.request(true));
        assert!(!state.take_reopen(true), "the quit flag fences the rest");
        assert_eq!(state.pending_reopens(), 0);
    }

    /// An explicit quit fences the owner signal and a reopen already
    /// accepted: the application never hears it, and no window is admitted
    /// after.
    #[test]
    fn an_explicit_quit_fences_the_signal_and_the_application_hears_no_reopen() {
        use crate::owner::app_delivery::testing::{AppCall, AppLog, bound};

        let signal = OwnerSignal::new(std::sync::Arc::new(|| Ok(())));
        let handler = AppLog::new();
        let run = bound(handler.clone());
        let app = &run.delivery;
        let requests = LoopRequests::new();
        requests.update(LoopState::install);
        assert_eq!(requests.update(LoopState::start), Some(false));
        assert!(requests.accept_reopen(&signal), "a drain is queued");
        // The quit arrives before the queued drain runs.
        assert!(requests.request(&signal, true));
        assert!(!signal.accepting(), "an explicit quit fences the signal");
        assert!(!requests.admits_windows(&signal));
        let mut after = false;
        requests.drain_reopens(&signal, app, || after = true);
        assert!(
            !handler.calls().contains(&AppCall::Reopen),
            "a quit-fenced reopen reached the application"
        );
        assert!(!after, "a drain that does not begin queues nothing");
        assert_eq!(requests.get().pending_reopens(), 0);
    }

    /// A reopen AppKit reports while the application hears one, from inside
    /// its handler, queues no second drain: the running one tells it, and
    /// leaves nothing owed.
    #[test]
    fn a_reopen_during_a_drain_is_told_by_that_drain() {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::sync::Arc;

        use crate::QuitAnswer;
        use crate::error::BootstrapError;
        use crate::owner::OwnerPlatform;
        use crate::owner::app_delivery::testing::{AppCall, AppLog, bound};
        use crate::platform::{AppHandler, ExitAnswer};
        use flui_platform::{Opened, SessionEndAnswer, SessionEndPhase, SystemNotice};

        /// The log, and on its first reopen AppKit reporting another.
        struct Reporting {
            log: Rc<AppLog>,
            requests: Rc<LoopRequests>,
            signal: Arc<OwnerSignal>,
            queued_inside: Cell<Option<bool>>,
        }
        impl AppHandler for Reporting {
            fn ready(&self, owner: OwnerPlatform) -> Result<(), BootstrapError> {
                self.log.ready(owner)
            }
            fn quit_requested(&self, owner: &OwnerPlatform) {
                self.log.quit_requested(owner);
            }
            fn quit_asked(&self, owner: &OwnerPlatform) -> QuitAnswer {
                self.log.quit_asked(owner)
            }
            fn reopen(&self, owner: &OwnerPlatform) {
                self.log.reopen(owner);
                if self.queued_inside.get().is_none() {
                    let queued = self.requests.accept_reopen(&self.signal);
                    self.queued_inside.set(Some(queued));
                }
            }
            fn opened(&self, owner: &OwnerPlatform, opened: Opened) {
                self.log.opened(owner, opened);
            }
            fn session_end(
                &self,
                owner: &OwnerPlatform,
                phase: SessionEndPhase,
            ) -> SessionEndAnswer {
                self.log.session_end(owner, phase)
            }
            fn host_notice(&self, owner: &OwnerPlatform, notice: SystemNotice) {
                self.log.host_notice(owner, notice);
            }
            fn exit_policy(&self, owner: &OwnerPlatform) -> ExitAnswer {
                self.log.exit_policy(owner)
            }
            fn wake_deadline(&self, owner: &OwnerPlatform) -> Option<web_time::Instant> {
                self.log.wake_deadline(owner)
            }
            fn turn_deadline(&self, owner: &OwnerPlatform) -> Option<web_time::Instant> {
                self.log.turn_deadline(owner)
            }
        }

        let signal = OwnerSignal::new(Arc::new(|| Ok(())));
        let requests = Rc::new(LoopRequests::new());
        requests.update(LoopState::install);
        assert_eq!(requests.update(LoopState::start), Some(false));
        let log = AppLog::new();
        let handler = Rc::new(Reporting {
            log: Rc::clone(&log),
            requests: Rc::clone(&requests),
            signal: Arc::clone(&signal),
            queued_inside: Cell::new(None),
        });
        let run = bound(handler.clone());
        let reopens = || {
            log.calls()
                .iter()
                .filter(|call| **call == AppCall::Reopen)
                .count()
        };

        assert!(requests.accept_reopen(&signal));
        let queued_after = Cell::new(true);
        requests.drain_reopens(&signal, &run.delivery, || {
            queued_after.set(requests.queue_reopens(&signal));
        });
        assert_eq!(
            handler.queued_inside.get(),
            Some(false),
            "a reopen reported during the drain queues no second drain"
        );
        assert_eq!(reopens(), 2, "the running drain tells it");
        assert!(!queued_after.get(), "nothing is left for another drain");
        assert!(requests.accept_reopen(&signal), "the drain has ended");
    }

    /// A drain tells the application each reopen it owes, then runs what
    /// queues the reopens reported meanwhile.
    #[test]
    fn a_drain_tells_each_owed_reopen_then_queues_the_rest() {
        use crate::owner::app_delivery::testing::{AppCall, AppLog, bound};

        let signal = OwnerSignal::new(std::sync::Arc::new(|| Ok(())));
        let handler = AppLog::new();
        let run = bound(handler.clone());
        let app = &run.delivery;
        let requests = LoopRequests::new();
        requests.update(LoopState::install);
        assert_eq!(requests.update(LoopState::start), Some(false));
        assert!(requests.accept_reopen(&signal));
        assert!(!requests.accept_reopen(&signal), "one drain is queued");
        let mut after = false;
        requests.drain_reopens(&signal, app, || after = true);
        let reopens = handler
            .calls()
            .iter()
            .filter(|call| **call == AppCall::Reopen)
            .count();
        assert_eq!(reopens, 2);
        assert!(after);
        assert!(requests.accept_reopen(&signal), "the drain has ended");
    }

    /// A reopen AppKit reports while the application is hearing one waits for
    /// that drain to end, and is then queued once.
    #[test]
    fn a_reopen_during_a_drain_waits_for_it_to_end() {
        let mut state = running();
        assert!(state.accept_reopen(true));
        assert!(state.queue_reopens(true));
        assert!(!state.queue_reopens(true), "one drain is queued at a time");
        assert!(state.begin_drain(true));
        assert!(state.take_reopen(true));
        // The application's reopen pumps a nested loop that reports another.
        assert!(state.accept_reopen(true));
        assert!(!state.queue_reopens(true), "the active drain takes it");
        assert!(state.take_reopen(true));
        assert!(!state.take_reopen(true));
        state.end_drain();
        assert!(!state.queue_reopens(true), "nothing is owed");
        assert!(state.accept_reopen(true));
        assert!(state.queue_reopens(true));
    }

    /// Evaluations coalesce while one is queued, and a loop that stops takes
    /// no more.
    #[test]
    fn evaluations_coalesce_and_stop_with_the_loop() {
        let mut state = running();
        assert!(state.request(false));
        assert!(!state.request(false), "already queued");
        assert_eq!(state.begin_evaluation(), Some(false));
        assert!(state.request(true));
        assert_eq!(state.begin_evaluation(), Some(true));
        assert!(state.running());
        assert_eq!(state.begin_quit(), Some(true));
        assert!(!state.running());
        assert_eq!(state.begin_quit(), None);
        assert!(!state.request(false));
        assert_eq!(state.begin_evaluation(), None);
        assert!(state.begin_finish());
        state.stopped();
        assert!(!state.begin_finish());
    }

    /// What AppKit opens before `applicationDidFinishLaunching:` is the
    /// launch, whether or not the loop runs yet; after it, it reaches a
    /// running application.
    #[test]
    fn the_launch_ends_when_appkit_says_it_finished() {
        let mut state = running();
        assert!(!state.launched());
        state.finish_launching();
        assert!(state.launched());
    }

    /// A quit during bootstrap keeps the loop from starting.
    #[test]
    fn a_quit_before_the_loop_runs_keeps_it_from_starting() {
        let mut state = LoopState::new();
        state.install();
        assert_eq!(state.begin_quit(), Some(false));
        assert_eq!(state.start(), None);
    }
}
