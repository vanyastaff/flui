//! What a [`HeadlessHost`]'s turn carries out as a
//! runner's owner turn does: the closes the windows asked for or their guards
//! let through, a quit, and the session's end. A mirror of the runner's, which
//! lives in `flui-app` above this crate (`docs/gaps.md`, "The headless host
//! mirrors the runner's owner turn").

use flui_foundation::PresentationId;
use flui_foundation::panic::FirstPanic;
use flui_platform::{SessionEndAnswer, SessionEndPhase};
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::__runtime::FlushWrite;
use flui_view::CloseReason;

use super::{HeadlessHost, HeadlessWindowId, QuitAsk, SESSION_END_BUDGET};

impl HeadlessHost {
    /// The session ends now, in the order a runner carries it out: the
    /// session-ended notice reaches every subscriber, then every window
    /// closes and what the documents published is written, as in
    /// [`quit_now`](Self::quit_now).
    ///
    /// Each step runs whatever an earlier one's user code did: the first
    /// panic resumes once the write is done.
    pub(crate) fn end_session_now(&mut self) {
        let mut first = FirstPanic::new();
        if let Some(runtime) = self.try_ui_runtime() {
            first.contain(|| {
                runtime.deliver_host_notice(&flui_platform::HostNotice::SessionEnded);
            });
        }
        self.quit_into(&mut first);
        first.resume();
    }

    /// The application quits now: each window commits its composition and
    /// closes past every hold, so its Detached observers publish what they
    /// keep, and then what the documents published is written. A write a
    /// storage holds is not waited for, since nothing on this thread can
    /// finish it: it stays with the storage, which decides it when its hold
    /// is released or dropped, and what any document still owes then is
    /// lost, as at an exit the write outlasts.
    ///
    /// Each step runs whatever an earlier one's user code did: the first
    /// panic resumes once the write is done.
    pub(crate) fn quit_now(&mut self) {
        let mut first = FirstPanic::new();
        self.quit_into(&mut first);
        first.resume();
    }

    /// Close every window, then write what the documents published without
    /// waiting for a held write; a panic goes to `first`.
    fn quit_into(&mut self, first: &mut FirstPanic) {
        for id in self.presentation_ids() {
            first.contain(|| self.ui_runtime().commit_composition_for_close(id));
            first.contain(|| self.close_presentation(id));
        }
        first.contain(|| self.write_published());
    }

    /// Run the queued registry writes until none finishes, then write what
    /// every document still owes and run what that queued. A write the
    /// storage holds is not waited for: nothing on this thread can finish it,
    /// and while any write is in flight the flush starts none, so what a
    /// document still owes then, a retry included, is not written.
    fn write_published(&self) {
        self.run_flush_writes();
        if let Some(flush) = &self.flush {
            let _report = flush.flush_within(std::time::Instant::now());
        }
        self.run_flush_writes();
    }

    /// One phase of the user's session ending, as the operating system reports
    /// it (ADR-0260). `Query` asks every window's guard for the session's
    /// end and answers [`Block`](SessionEndAnswer::Block) when one refuses;
    /// `Cancelled` withdraws only those session-end records; `Ending` is
    /// what the platform the primary window names does
    /// ([`session_end_support`](Self::session_end_support)), as a runner
    /// does it (ADR-0260, ADR-0350 §2):
    ///
    /// - on a desktop (`Hold`, `Delay`) the session-ended notice is
    ///   delivered, every window closes past every hold, what the documents
    ///   published is written and the host retires;
    /// - on a phone (`Background`) the notice is delivered and the documents
    ///   are written with every window kept open for the user's return, and
    ///   the answer is [`Block`](SessionEndAnswer::Block) while writes are
    ///   still in flight: the hold is released at the turn after they end
    ///   ([`session_end_releases`](Self::session_end_releases)), and a later
    ///   trip's session end replaces it.
    ///
    /// # Panics
    ///
    /// At `Ending`, with the first panic of a notice subscriber, a close or
    /// the write, once every step has run: a runner contains and logs it,
    /// and the host raises it, as it raises a dropped frame (see the
    /// [module docs](super#failures)).
    pub fn end_session(&mut self, phase: SessionEndPhase) -> SessionEndAnswer {
        let windows = self.presentation_ids();
        match phase {
            SessionEndPhase::Query => {
                let mut answer = SessionEndAnswer::Proceed;
                for id in windows {
                    if self.ui_runtime().close_refused(id, CloseReason::SessionEnd) {
                        answer = SessionEndAnswer::Block;
                    }
                }
                answer
            }
            SessionEndPhase::Cancelled => {
                for id in windows {
                    self.ui_runtime()
                        .withdraw_close(id, CloseReason::SessionEnd);
                }
                SessionEndAnswer::Proceed
            }
            SessionEndPhase::Ending => match self.session_end_support() {
                flui_platform::SessionEndSupport::Hold
                | flui_platform::SessionEndSupport::Delay { .. } => {
                    self.end_session_now();
                    SessionEndAnswer::Proceed
                }
                _ => self.end_session_in_background(),
            },
            // A phase added later asks this host nothing.
            _ => SessionEndAnswer::Proceed,
        }
    }

    /// How the platform the primary window names lets the session end, as
    /// its host reports it: `Background` on Android and iOS, `Delay` on
    /// Linux (logind), and `Hold` on Windows, macOS and the platform-neutral
    /// window, as the native headless platform does. A duration names the
    /// session end's budget: this host's clock waits for nothing.
    #[must_use]
    pub fn session_end_support(&self) -> flui_platform::SessionEndSupport {
        use flui_platform::{SessionEndSupport, TargetPlatform};
        match self.primary.window.platform {
            TargetPlatform::Android | TargetPlatform::iOS => SessionEndSupport::Background {
                window: SESSION_END_BUDGET,
            },
            TargetPlatform::Linux => SessionEndSupport::Delay {
                max: SESSION_END_BUDGET,
            },
            _ => SessionEndSupport::Hold,
        }
    }

    /// How many holds a phone's session end took for its writes were
    /// released once the writes ended.
    #[must_use]
    pub fn session_end_releases(&self) -> usize {
        self.session_end_releases
    }

    /// A session end that leaves the application running: the notice, then
    /// the write, every window open; `Block` while writes are in flight.
    fn end_session_in_background(&mut self) -> SessionEndAnswer {
        // As a runner's: the notice first, so a subscriber still publishes
        // what the write then takes.
        let mut first = FirstPanic::new();
        if let Some(runtime) = self.try_ui_runtime() {
            first.contain(|| {
                runtime.deliver_host_notice(&flui_platform::HostNotice::SessionEnded);
            });
        }
        first.contain(|| self.write_published());
        // An earlier trip's hold answers for nothing now: this one answers
        // for what is in flight.
        self.background_hold = !self.flush_writes.lock().is_empty();
        let answer = if self.background_hold {
            SessionEndAnswer::Block
        } else {
            SessionEndAnswer::Proceed
        };
        first.resume();
        answer
    }

    /// The owner turn's settle, before its frame, in a runner's order: carry
    /// out the closes the windows' widgets asked for, then what the windows'
    /// guards now let through (a close is carried out; a quit's close is the
    /// window's consent to the quit and closes nothing alone, and a quit's
    /// close the application kept open cancels the quit), consult or commit
    /// the quit, run the registry's queued writes and wake what they
    /// resolved, and release a phone's background hold once its writes end.
    ///
    /// Each step and each close runs contained, so one that panics costs
    /// none of the others; the first panic resumes once all have run.
    pub(super) fn settle_lifecycle(&mut self) {
        let mut first = FirstPanic::new();
        let asked = std::mem::take(&mut *self.close_asks.borrow_mut());
        for id in asked {
            first.contain(|| self.close_asked(id));
        }
        let settled = self
            .ui_runtime
            .as_ref()
            .map_or_else(Default::default, UiRuntime::settle_close_guards);
        for (id, reason) in settled.due {
            if reason != CloseReason::Program {
                first.contain(|| {
                    self.close_presentation(id);
                });
            }
            self.quit_consented(id);
        }
        for (id, reason) in settled.declined {
            if reason == CloseReason::Program {
                self.quit_declined(id);
            }
        }
        self.consult_quit(&mut first);
        first.contain(|| self.run_flush_writes());
        if let Some(flush) = &self.flush {
            for waker in flush.settle() {
                first.contain(|| waker.wake());
            }
        }
        if self.background_hold && self.flush_writes.lock().is_empty() {
            self.background_hold = false;
            self.session_end_releases += 1;
        }
        first.resume();
    }

    /// Carry out the close a widget of window `id` asked for, as a runner's
    /// window queue does: the window's guard is asked as for the program's
    /// close, and a refusal is recorded on it; otherwise the window commits
    /// its composition and closes. A window already gone closes nothing.
    fn close_asked(&mut self, id: PresentationId) {
        if !self.is_open(HeadlessWindowId(id))
            || self.ui_runtime().close_refused(id, CloseReason::Program)
        {
            return;
        }
        self.ui_runtime().commit_composition_for_close(id);
        self.close_presentation(id);
    }

    /// Ask for the application to quit, as `AppHandle::request_quit` does and
    /// as AppKit's Dock Quit reaches a runner (ADR-0350 §1): the next turn
    /// asks every open window, in the order they opened, as for the program's
    /// close, its guard recording a refusal. A window that refuses with a
    /// record defers the quit until its close comes due; one that refuses
    /// without one, or keeps itself open, cancels it; when no window holds it,
    /// every window closes past every hold, what the documents published is
    /// written and the host retires. A quit asked while one is under way
    /// changes nothing.
    pub fn request_quit(&mut self) {
        if matches!(self.quit, QuitAsk::None) && !self.is_retired() {
            self.quit = QuitAsk::Asked;
        }
    }

    /// The quit a turn finds asked: ask every open window, or commit it once
    /// no window holds it.
    fn consult_quit(&mut self, first: &mut FirstPanic) {
        match &self.quit {
            QuitAsk::None => {}
            QuitAsk::Asked => {
                let mut waiting = Vec::new();
                let mut cancelled = false;
                // Every window is asked, not only up to the first refusal,
                // so each one that refuses records its close.
                for id in self.presentation_ids() {
                    if self.ui_runtime().close_refused(id, CloseReason::Program) {
                        if self.ui_runtime().close_recorded(id, CloseReason::Program) {
                            waiting.push(id);
                        } else {
                            cancelled = true;
                        }
                    }
                }
                self.quit = if cancelled {
                    tracing::info!(
                        "a window refused the quit without recording it; quit cancelled"
                    );
                    QuitAsk::None
                } else {
                    QuitAsk::Deferred(waiting)
                };
                self.commit_quit_if_free(first);
            }
            QuitAsk::Deferred(_) => self.commit_quit_if_free(first),
        }
    }

    /// Carry out a quit no window holds any more.
    fn commit_quit_if_free(&mut self, first: &mut FirstPanic) {
        if matches!(&self.quit, QuitAsk::Deferred(waiting) if waiting.is_empty()) {
            self.quit = QuitAsk::None;
            self.quit_into(first);
        }
    }

    /// Window `id` consented to the deferred quit: its recorded close came
    /// due, or it is gone.
    pub(super) fn quit_consented(&mut self, id: PresentationId) {
        if let QuitAsk::Deferred(waiting) = &mut self.quit {
            waiting.retain(|waited| *waited != id);
        }
    }

    /// Window `id` kept itself open: the deferred quit that waited on it is
    /// cancelled.
    fn quit_declined(&mut self, id: PresentationId) {
        if matches!(&self.quit, QuitAsk::Deferred(waiting) if waiting.contains(&id)) {
            tracing::info!(?id, "a window stayed open; deferred quit cancelled");
            self.quit = QuitAsk::None;
        }
    }

    /// Poll the queued registry writes, keeping those still waiting for their
    /// storage, until a pass finishes none: a finished write may queue the
    /// revision published behind it, which the next pass runs. The queue's
    /// lock is not held while a write runs.
    pub(super) fn run_flush_writes(&self) {
        let mut ctx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            let writes = std::mem::take(&mut *self.flush_writes.lock());
            let queued = writes.len();
            let waiting: Vec<FlushWrite> = writes
                .into_iter()
                .filter_map(|mut write| write.as_mut().poll(&mut ctx).is_pending().then_some(write))
                .collect();
            let finished_any = waiting.len() < queued;
            self.flush_writes.lock().extend(waiting);
            if !finished_any {
                return;
            }
        }
    }
}
