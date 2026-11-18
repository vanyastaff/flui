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
    /// to the application now.
    pub(crate) fn take_reopen(&mut self, accepting: bool) -> bool {
        if self.phase != Phase::Running || self.explicit_quit || !accepting {
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

    /// An explicit quit fences a reopen already accepted: the drain tells the
    /// application nothing, and no window is admitted after.
    #[test]
    fn an_explicit_quit_fences_a_reopen_already_accepted() {
        let mut state = LoopState::new();
        assert!(state.accept_reopen(true));
        // Not running yet: queued as the loop starts, not now.
        assert!(!state.request(true));
        // The quit fences the owner signal before the control is asked again.
        assert!(!state.admits_windows(false));
        assert!(
            !state.admits_windows(true),
            "an explicit quit admits no window"
        );
        state.install();
        assert_eq!(state.start(), Some(true));
        assert!(!state.begin_drain(false));
        assert!(!state.take_reopen(false));
        assert_eq!(state.pending_reopens(), 0);
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
