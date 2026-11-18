//! What a [`HeadlessHost`]'s turn carries out, through the steps every
//! runner's owner turn runs (`flui_runtime::owner_turn`): the closes the
//! windows asked for or their guards let through, a quit, and the session's
//! end. This file holds what those steps do to a headless host's own state.

use std::any::Any;
use std::rc::Rc;

use flui_foundation::panic::FirstPanic;
use flui_foundation::{PresentationAddress, PresentationId};
use flui_platform::{HostNotice, SessionEndAnswer, SessionEndPhase, SessionEndSupport};
use flui_runtime::owner_turn::{
    CloseRequestRouter, CloseResponse, NoticeDelivery, QuitVerdict, SessionEndHost, SessionEndStep,
    SettledStep, ask_quit, settled_steps,
};
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::__runtime::{FlushWrite, WritesInFlight};
use flui_view::CloseReason;

use super::{HeadlessHost, HeadlessWindowId, session_end_support_of};

impl HeadlessHost {
    /// The application quits now: each window commits its composition and
    /// closes past every hold, so its Detached observers publish what they
    /// keep, and then what the documents published is written, as a runner's
    /// loop exit tears its runtimes down before its flush. A write a storage
    /// holds is not waited for, since nothing on this thread can finish it:
    /// it stays with the storage, which decides it when its hold is released
    /// or dropped, and what any document still owes then is lost, as at an
    /// exit the write outlasts.
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
        self.quit.clear();
        self.close_every_window(first);
        first.contain(|| self.write_published());
    }

    /// Commit each open window's composition and close it past every hold; a
    /// panic goes to `first`.
    fn close_every_window(&mut self, first: &mut FirstPanic) {
        for id in self.presentation_ids() {
            first.contain(|| self.ui_runtime().commit_composition_for_close(id));
            first.contain(|| self.close_presentation(id));
        }
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
    /// it, answered by the steps a runner answers it with
    /// ([`SessionEnd::answer`](flui_runtime::owner_turn::SessionEnd::answer))
    /// on the platform the primary window names
    /// ([`session_end_support`](Self::session_end_support)):
    ///
    /// - `Query` asks every window, its close-request handler then its
    ///   guard, where the platform holds the session for a block, and tells
    ///   the subscribers with [`HostNotice::SessionEnding`];
    ///   [`Block`](SessionEndAnswer::Block) when a window refused or a
    ///   subscriber holds a block;
    /// - `Cancelled` withdraws only those session-end records and tells the
    ///   subscribers;
    /// - `Ending` delivers the session-ended notice; on a desktop (`Hold`,
    ///   `Delay`) every window hears `Detached`, what the documents published
    ///   is written and the host retires; on a phone (`Background`) the
    ///   documents are written with every window kept open for the user's
    ///   return, and the answer is [`Block`](SessionEndAnswer::Block) while
    ///   writes are still in flight: the hold is released at the turn after
    ///   they end ([`session_end_releases`](Self::session_end_releases)), and
    ///   a later trip's session end replaces it.
    ///
    /// # Panics
    ///
    /// With the first panic of a notice subscriber, a `Detached` observer,
    /// a close or the write, once every step has run: a runner contains and
    /// logs it, and the host raises it, as it raises a dropped frame (see the
    /// [module docs](super#failures)).
    pub fn end_session(&mut self, phase: SessionEndPhase) -> SessionEndAnswer {
        let book = Rc::clone(&self.session_end);
        let mut steps = HeadlessSessionEnd {
            host: self,
            first: FirstPanic::new(),
        };
        let answer = book.answer(phase, &mut steps);
        steps.first.resume();
        answer
    }

    /// How the platform the primary window names lets the session end, as
    /// its host reports it: `Background` on Android and iOS, `Delay` on
    /// Linux (logind), and `Hold` on Windows, macOS and the platform-neutral
    /// window, as the native headless platform does. A duration names the
    /// session end's budget.
    #[must_use]
    pub fn session_end_support(&self) -> SessionEndSupport {
        session_end_support_of(self.primary.window.platform)
    }

    /// How many times the application let the platform's session end go on:
    /// a phone's background hold released once its writes ended, or the last
    /// block a query's request held released (every window that refused
    /// carried its close out, every subscriber let go), as a runner tells its
    /// platform (`Platform::release_session_end`).
    #[must_use]
    pub fn session_end_releases(&self) -> usize {
        self.session_end_releases.get()
    }

    /// The owner turn's settle, before its frame, in a runner's order: carry
    /// out the closes the windows' widgets asked for, then what the windows'
    /// guards now let through ([`settled_steps`]), consult or commit the quit,
    /// run the registry's queued writes and wake what they resolved, and
    /// release a phone's background hold once its writes end.
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
        let session_end = Rc::clone(&self.session_end);
        for step in settled_steps(settled, self.ui_runtime_id, &session_end) {
            match step {
                SettledStep::Close(address) => {
                    first.contain(|| self.close_presentation(address.presentation_id));
                }
                SettledStep::QuitConsented(address) => {
                    let verdict = self.quit.consented(address);
                    self.follow_quit(verdict);
                }
                SettledStep::QuitDeclined(address) => {
                    let _abandoned = self.quit.declined(address);
                }
                _ => {}
            }
        }
        self.consult_quit();
        if std::mem::take(&mut self.quit_owed) {
            self.quit_into(&mut first);
        }
        first.contain(|| self.run_flush_writes());
        if let Some(flush) = &self.flush {
            for waker in flush.settle() {
                first.contain(|| waker.wake());
            }
        }
        if self.background_hold && self.flush_writes.lock().is_empty() {
            self.background_hold = false;
            self.session_end_releases
                .set(self.session_end_releases.get() + 1);
        }
        first.resume();
    }

    /// Carry out the close a widget of window `id` asked for, as a runner's
    /// window queue does: asked as for the program's close
    /// ([`CloseRequestRouter::consult`]), whose refusal is recorded on the
    /// window's guard; otherwise the window commits its composition and
    /// closes. A window already gone closes nothing.
    fn close_asked(&mut self, id: PresentationId) {
        if !self.is_open(HeadlessWindowId(id)) {
            return;
        }
        let router = Rc::clone(&self.close_requests);
        if router.consult(self.address(id), CloseReason::Program) != CloseResponse::Close {
            return;
        }
        self.ui_runtime().commit_composition_for_close(id);
        self.close_presentation(id);
    }

    /// Ask for the application to quit, as `AppHandle::request_quit` does and
    /// as AppKit's Dock Quit reaches a runner (ADR-0350 §1): the next turn
    /// asks every open window, in the order they opened, as for the program's
    /// close ([`ask_quit`]). A window that refuses with a record defers the
    /// quit until its close comes due; one that refuses without one, or keeps
    /// itself open, cancels it; when no window holds it, every window closes
    /// past every hold, what the documents published is written and the host
    /// retires. A quit asked while one is deferred asks every window again.
    pub fn request_quit(&mut self) {
        if !self.is_retired() {
            self.quit.ask();
        }
    }

    /// The quit a turn finds asked or deferred: a window it waits on that is
    /// gone consents, and an asked quit asks every open window. A quit no
    /// window holds is owed to the turn.
    fn consult_quit(&mut self) {
        let open: Vec<PresentationAddress> = self
            .presentation_ids()
            .into_iter()
            .map(|id| self.address(id))
            .collect();
        let verdict = self.quit.forget_gone(&open);
        self.follow_quit(verdict);
        if !self.quit.begin_consult() {
            return;
        }
        let router: Rc<CloseRequestRouter> = Rc::clone(&self.close_requests);
        let verdict = self.quit.finish_consult(ask_quit(&router, open));
        self.follow_quit(verdict);
    }

    /// A quit no window holds is owed to the turn, which carries it out.
    fn follow_quit(&mut self, verdict: QuitVerdict) {
        if verdict == QuitVerdict::Commit {
            self.quit_owed = true;
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

/// A headless host's side of the session end's steps, keeping the first
/// panic a step raised for [`HeadlessHost::end_session`] to raise.
struct HeadlessSessionEnd<'a> {
    host: &'a mut HeadlessHost,
    first: FirstPanic,
}

impl SessionEndHost for HeadlessSessionEnd<'_> {
    fn support(&self) -> Option<SessionEndSupport> {
        Some(self.host.session_end_support())
    }

    fn router(&self) -> Option<Rc<CloseRequestRouter>> {
        Some(Rc::clone(&self.host.close_requests))
    }

    /// Every open window: a headless runtime's code is never on the stack
    /// when a test reports a session end.
    fn askable_presentations(&self) -> Vec<PresentationAddress> {
        self.host
            .presentation_ids()
            .into_iter()
            .map(|id| self.host.address(id))
            .collect()
    }

    fn deliver_notice(
        &mut self,
        notice: &HostNotice,
        may_start: &dyn Fn() -> bool,
    ) -> NoticeDelivery {
        let mut delivery = NoticeDelivery::default();
        let Some(runtime) = self.host.try_ui_runtime() else {
            return delivery;
        };
        if may_start() {
            runtime.deliver_host_notice(notice);
            delivery.ran = 1;
        } else {
            delivery.refused = 1;
        }
        delivery
    }

    fn release_hook(&self) -> Box<dyn FnOnce()> {
        let releases = Rc::clone(&self.host.session_end_releases);
        Box::new(move || releases.set(releases.get() + 1))
    }

    fn close_for_session_end(&mut self, may_start: &dyn Fn() -> bool) {
        if let Some(runtime) = self.host.try_ui_runtime()
            && may_start()
        {
            runtime.enter(|runtime| {
                runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Detached);
            });
        }
    }

    /// The queued writes run, then what every document still owes; a write
    /// the storage holds is not waited for, since nothing on this thread can
    /// finish it, and is handed back as in flight.
    fn flush(&mut self, _deadline: std::time::Instant) -> Option<WritesInFlight> {
        self.host.write_published();
        self.host
            .flush
            .as_ref()
            .and_then(flui_view::__runtime::FlushHost::writes_in_flight)
    }

    fn is_runtime_busy(&self) -> bool {
        false
    }

    fn end_application(&mut self, tear_down: bool) {
        if tear_down {
            self.host.quit.clear();
            self.host.close_every_window(&mut self.first);
        }
    }

    /// Held until the queued writes end, which the turn after releases.
    fn hold_background(
        &mut self,
        _writes: WritesInFlight,
        _until: std::time::Instant,
    ) -> SessionEndAnswer {
        self.host.background_hold = true;
        SessionEndAnswer::Block
    }

    fn drop_background_hold(&mut self) {
        self.host.background_hold = false;
    }

    /// Kept to raise once the session end returns: a runner contains and
    /// logs it, a test sees it.
    fn contained(&mut self, step: SessionEndStep, payload: Box<dyn Any + Send>) {
        tracing::debug!(
            ?step,
            "a session-end step panicked; raised once the session end returns"
        );
        self.first.record(payload);
    }
}
