//! The user's session ending (ADR-0237, ADR-0260): the query every
//! presentation is asked and the notice its subscribers hear, the cancel that
//! withdraws what the query recorded, and the end, which writes what the
//! documents published within a budget and ends the application where the
//! platform ends it with the session.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::panic::{FirstPanic, discard_payload, payload_text};
use flui_foundation::{MonotonicClock, PresentationAddress};
use flui_platform::{
    HostNotice, SessionEndAnswer, SessionEndBlock, SessionEndPhase, SessionEndRequest,
    SessionEndSupport,
};
use flui_view::CloseReason;

use super::{CloseRequestRouter, CloseResponse, WindowSystem};

/// The least of the session end's budget that must be left for a step that
/// runs application code to start, so the flush keeps time to write
/// (ADR-0260, "the 1 s threshold").
const SESSION_END_USER_CODE_THRESHOLD: Duration = Duration::from_secs(1);

/// How long the framework's part of a session end may take where the platform
/// lets it end with `support`: [`SESSION_END_BUDGET`](flui_view::__runtime::SESSION_END_BUDGET),
/// or less where the platform waits a shorter bounded time.
fn session_end_budget(support: Option<SessionEndSupport>) -> Duration {
    use flui_view::__runtime::SESSION_END_BUDGET;
    match support {
        Some(SessionEndSupport::Delay { max }) => max.min(SESSION_END_BUDGET),
        _ => SESSION_END_BUDGET,
    }
}

/// How a notice reached a host's UI runtimes: on the spot, queued behind a
/// turn that was already running, or not at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct NoticeDelivery {
    /// Runtimes whose subscribers had heard it when the call returned.
    pub ran: usize,
    /// Runtimes whose turn was busy: they hear it later, after the call.
    pub queued: usize,
    /// Runtimes that could not take it, or were not started because the
    /// budget for application code was spent.
    pub refused: usize,
}

impl NoticeDelivery {
    /// Whether every subscriber has heard the notice by now.
    #[must_use]
    pub fn is_complete(self) -> bool {
        self.queued == 0 && self.refused == 0
    }
}

/// A step of the session end whose panic was contained, which the host is
/// told of ([`SessionEndHost::contained`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionEndStep {
    /// A window asked at the query whether the session may end: what its
    /// question ran past its close handler's own answer (the drop of a
    /// handler that closed its own window).
    Query,
    /// A subscriber of a session-end notice.
    Notice,
    /// A `Detached` observer at the session's end, or a window's close.
    Close,
    /// The write of what the documents published.
    Flush,
}

/// What a host does for its session end's steps; which steps run, in which
/// order and within which budget is [`SessionEnd`]'s.
///
/// Each method runs on the owner thread, with no borrow of the [`SessionEnd`]
/// held, and may run application code; the session end contains a panic
/// that escapes a window's question at the query,
/// [`deliver_notice`](Self::deliver_notice),
/// [`close_for_session_end`](Self::close_for_session_end) or
/// [`flush`](Self::flush) and hands it to [`contained`](Self::contained), so
/// every later step still runs and the platform still gets its answer.
pub trait SessionEndHost {
    /// How the platform lets the session end; `None` while the host has no
    /// platform to ask.
    fn support(&self) -> Option<SessionEndSupport>;

    /// The window system the host's windows are drawn in: a browser's page
    /// ends its session as `pagehide`, with the notice alone. A host states
    /// it (a runner built for the web, a headless window playing the
    /// browser); nothing here reads the build target.
    fn window_system(&self) -> WindowSystem;

    /// The clock the session end's budget runs on: the operating system's on
    /// a runner, the test's manual clock on a headless host, where a slow run
    /// spends none of the budget and only the test moves it.
    fn clock(&self) -> Rc<dyn MonotonicClock>;

    /// The router the host's presentations are registered with; `None` while
    /// the host's state cannot be read.
    fn router(&self) -> Option<Rc<CloseRequestRouter>>;

    /// The registered presentations a query may ask, in registration order:
    /// those whose UI runtime can run code now.
    fn askable_presentations(&self) -> Vec<PresentationAddress>;

    /// Deliver `notice` to every UI runtime the host runs, starting each
    /// one's subscribers only while `may_start` allows.
    fn deliver_notice(
        &mut self,
        notice: &HostNotice,
        may_start: &dyn Fn() -> bool,
    ) -> NoticeDelivery;

    /// What tells the platform the application is done with the session end
    /// it asked to wait for (`Platform::release_session_end`): run once, when
    /// the last block of a query's request is released.
    fn release_hook(&self) -> Box<dyn FnOnce()>;

    /// Tell every UI runtime whose code can run that the application ends
    /// with the session: each presentation commits its composition and its
    /// observers hear `Detached`, so what they publish reaches the flush
    /// after it. A runtime's code starts only while `may_start` allows.
    fn close_for_session_end(&mut self, may_start: &dyn Fn() -> bool);

    /// Write what the documents published, by `deadline`, running no
    /// application code; the writes still in flight then, if any.
    fn flush(
        &mut self,
        deadline: web_time::Instant,
    ) -> Option<flui_view::__runtime::WritesInFlight>;

    /// Write what the host keeps for itself (a runner's window placements)
    /// by `deadline`, after the application's own work and the documents.
    fn flush_host_state(&mut self, deadline: web_time::Instant) {
        let _ = deadline;
    }

    /// Whether a UI runtime's code is on the stack, so none may be torn down.
    fn is_runtime_busy(&self) -> bool;

    /// The application ends with the session: with `tear_down`, every UI
    /// runtime is shut down and dropped, which runs the application's
    /// destructors; then the host stops.
    fn end_application(&mut self, tear_down: bool);

    /// Keep a phone's process running for `writes` until `until`, and release
    /// the platform once they end. [`Block`](SessionEndAnswer::Block) while
    /// the hold is taken, [`Proceed`](SessionEndAnswer::Proceed) when it
    /// cannot be.
    fn hold_background(
        &mut self,
        writes: flui_view::__runtime::WritesInFlight,
        until: web_time::Instant,
    ) -> SessionEndAnswer;

    /// Drop the hold an earlier trip's session end took: this one answers for
    /// what is in flight now.
    fn drop_background_hold(&mut self);

    /// A panic `step` raised was contained: report it, or keep it to raise
    /// once the session end returns.
    fn contained(&mut self, step: SessionEndStep, payload: Box<dyn Any + Send>);
}

/// A host's session end: the query's request and the refusals it holds until
/// the session end is cancelled or happens, or the window that refused
/// closes.
///
/// The book is borrowed only inside these methods and never across a call
/// into the host or the application, so a handler or subscriber that
/// re-enters it (closing a window that refused) finds it free.
#[derive(Default)]
pub struct SessionEnd {
    book: RefCell<Book>,
}

#[derive(Default)]
struct Book {
    /// The query's request, kept until the session end is cancelled or
    /// happens, so the blocks its subscribers took outlive the query.
    request: Option<SessionEndRequest>,
    /// Whether that request is still the one the platform waits on: a later
    /// query displaces it, and a displaced request's release answers nothing.
    current: Option<Rc<Cell<bool>>>,
    /// The windows that refused the session end, each holding a block on the
    /// request until it carries its close out.
    refusals: Vec<(PresentationAddress, SessionEndBlock)>,
}

impl std::fmt::Debug for SessionEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let book = self.book.borrow();
        f.debug_struct("SessionEnd")
            .field("request", &book.request)
            .field(
                "refusals",
                &book.refusals.iter().map(|(at, _)| at).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl SessionEnd {
    /// No session end under way.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer one phase of the user's session ending.
    ///
    /// A [`Query`](SessionEndPhase::Query) asks the close of every askable
    /// presentation with [`CloseReason::SessionEnd`], its close handler then
    /// its guard ([`CloseRequestRouter::consult`]), where the
    /// platform holds the session for a block; each refusal holds a block
    /// on the query's request until its window carries its close out
    /// ([`release_refusal`](Self::release_refusal)). The query is then one
    /// [`HostNotice::SessionEnding`] for every runtime, all sharing the
    /// request, whose blocks outlive the query. The answer is
    /// [`Block`](SessionEndAnswer::Block) when a presentation refused or a
    /// subscriber holds a block. Subscribers of a runtime whose turn is busy
    /// hear the notice after the request's book has closed, so a block taken
    /// then is refused instead of silently too late.
    ///
    /// [`Cancelled`](SessionEndPhase::Cancelled) withdraws only the
    /// session-end closes the query recorded and tells the subscribers.
    ///
    /// [`Ending`](SessionEndPhase::Ending) delivers
    /// [`HostNotice::SessionEnded`] while the budget
    /// ([`SESSION_END_BUDGET`](flui_view::__runtime::SESSION_END_BUDGET), or
    /// a platform's shorter delay, on [`SessionEndHost::clock`]) leaves a
    /// second for application code; where the platform ends the
    /// application with the session ([`Hold`](SessionEndSupport::Hold),
    /// [`Delay`](SessionEndSupport::Delay)) every runtime then hears
    /// `Detached`; what the documents published is written, then what the
    /// host keeps; and the application ends, its runtimes torn down only
    /// while their destructors can start within the budget and none is on
    /// the stack. On a phone ([`Background`](SessionEndSupport::Background))
    /// the windows stay open and the writes still in flight hold the process
    /// ([`SessionEndHost::hold_background`]). In a browser's page
    /// ([`WindowSystem::Browser`], as the host names it), which grants no
    /// synchronous budget, the notice is all.
    pub fn answer(
        &self,
        phase: SessionEndPhase,
        host: &mut dyn SessionEndHost,
    ) -> SessionEndAnswer {
        match phase {
            SessionEndPhase::Query => self.query(host),
            SessionEndPhase::Cancelled => {
                if let Some(router) = host.router() {
                    for address in router.addresses() {
                        router.withdraw(address, CloseReason::SessionEnd);
                    }
                }
                drop(self.take());
                deliver_contained(host, &HostNotice::SessionEndCancelled, &|| true);
                SessionEndAnswer::Proceed
            }
            SessionEndPhase::Ending => {
                drop(self.take());
                end(host)
            }
            // A phase this host does not know asks for nothing.
            _ => SessionEndAnswer::Proceed,
        }
    }

    /// The window at `address` carried out the session-end close it refused:
    /// its refusal no longer holds the session. Once nothing else holds it,
    /// the platform is released.
    pub fn release_refusal(&self, address: PresentationAddress) {
        let released: Vec<_> = {
            let mut book = self.book.borrow_mut();
            let (released, kept) = std::mem::take(&mut book.refusals)
                .into_iter()
                .partition(|(refused, _)| *refused == address);
            book.refusals = kept;
            released
        };
        // Outside the borrow: the last release runs the platform's hook.
        drop(released);
    }

    /// Whether a window still refuses the session end being answered.
    fn has_refusals(&self) -> bool {
        !self.book.borrow().refusals.is_empty()
    }

    fn query(&self, host: &mut dyn SessionEndHost) -> SessionEndAnswer {
        let current = Rc::new(Cell::new(true));
        let live = Rc::clone(&current);
        let release = host.release_hook();
        let request = SessionEndRequest::new().on_release(move || {
            // A request a later query displaced answers nothing: the platform
            // waits on the later one.
            if live.get() {
                release();
            }
        });
        let displaced = {
            let mut book = self.book.borrow_mut();
            if let Some(displaced) = book.current.replace(current) {
                displaced.set(false);
            }
            (
                book.request.replace(request.clone()),
                std::mem::take(&mut book.refusals),
            )
        };
        drop(displaced);
        self.refuse(host, &request);
        // A subscriber that panics still gets the platform its answer, and
        // the book still closes. A panicking subscriber holds no block.
        let delivery =
            deliver_contained(host, &HostNotice::SessionEnding(request.clone()), &|| true);
        let held = request.answer();
        if let Some(delivery) = delivery
            && !delivery.is_complete()
        {
            // A subscriber is user code; the platform still gets its answer.
            let mut caught = FirstPanic::new();
            caught.contain(|| {
                tracing::warn!(
                    queued = delivery.queued,
                    refused = delivery.refused,
                    "a session end arrived while a UI runtime was busy; its subscribers hear it \
                     after the answer and cannot hold the session"
                );
            });
            if let Some(payload) = caught.into_payload() {
                report_contained(host, SessionEndStep::Notice, payload);
            }
        }
        if delivery.is_some() || self.has_refusals() {
            held
        } else {
            SessionEndAnswer::Proceed
        }
    }

    /// Ask every askable presentation whether the session may end, and take a
    /// block on `request` for each one that refuses. Where the platform does
    /// not hold a session end for a block, nothing can be refused and nobody
    /// is asked. Each refusal is published before the next window is asked:
    /// a later handler that finishes the work and closes a window that
    /// already refused releases it through that close.
    ///
    /// A panic that escapes one window's question (the drop of a handler that
    /// closed its own window) is no answer, so that window
    /// refuses, as a handler's own panic does; every later window is still
    /// asked, and the first such panic goes to [`SessionEndHost::contained`].
    fn refuse(&self, host: &mut dyn SessionEndHost, request: &SessionEndRequest) {
        if host.support() != Some(SessionEndSupport::Hold) {
            return;
        }
        let Some(router) = host.router() else {
            return;
        };
        let mut caught = FirstPanic::new();
        for address in host.askable_presentations() {
            let answer = caught
                .contain(|| router.consult(address, CloseReason::SessionEnd))
                .unwrap_or(CloseResponse::KeepOpen);
            if answer != CloseResponse::KeepOpen {
                continue;
            }
            match request.block("a window waits for the user's decision") {
                Ok(block) => self.book.borrow_mut().refusals.push((address, block)),
                Err(error) => {
                    caught.contain(|| {
                        tracing::warn!(?address, %error, "a window's refusal holds no block");
                    });
                }
            }
        }
        if let Some(payload) = caught.into_payload() {
            report_contained(host, SessionEndStep::Query, payload);
        }
    }

    /// The request and the refusals, taken out of the book; dropping them
    /// releases what they held.
    fn take(
        &self,
    ) -> (
        Option<SessionEndRequest>,
        Vec<(PresentationAddress, SessionEndBlock)>,
    ) {
        let mut book = self.book.borrow_mut();
        book.current = None;
        (book.request.take(), std::mem::take(&mut book.refusals))
    }
}

/// Deliver `notice` through `host`, containing a subscriber's panic: the
/// caller answers a native callback, where an unwind would abort the process
/// or skip the answer. `None` when a subscriber panicked.
fn deliver_contained(
    host: &mut dyn SessionEndHost,
    notice: &HostNotice,
    may_start: &dyn Fn() -> bool,
) -> Option<NoticeDelivery> {
    let mut caught = FirstPanic::new();
    let delivery = caught.contain(|| host.deliver_notice(notice, may_start));
    if let Some(payload) = caught.into_payload() {
        report_contained(host, SessionEndStep::Notice, payload);
    }
    delivery
}

/// The session's end happens (ADR-0260 §3, §4, §6), in the window system the
/// host names.
fn end(host: &mut dyn SessionEndHost) -> SessionEndAnswer {
    if host.window_system() == WindowSystem::Browser {
        return end_page(host);
    }
    let support = host.support();
    let clock = host.clock();
    let deadline = clock.now() + session_end_budget(support);
    // A platform that only tells, or a phone leaving the screen, reports that
    // the process may be killed, not that the application is over: it may
    // come back to the same UI. Only a platform that holds or delays the
    // session ends the application here.
    let ends_application = matches!(
        support,
        Some(SessionEndSupport::Hold | SessionEndSupport::Delay { .. })
    );
    host.drop_background_hold();
    // Application code runs while the budget can absorb it: a subscriber or a
    // Detached observer may still publish what the flush then writes. Each
    // runtime's code starts only while the margin is left, whatever the
    // runtimes before it spent.
    let margin_left =
        || deadline.saturating_duration_since(clock.now()) >= SESSION_END_USER_CODE_THRESHOLD;
    let _delivery = deliver_contained(host, &HostNotice::SessionEnded, &margin_left);
    if ends_application {
        let mut caught = FirstPanic::new();
        caught.contain(|| host.close_for_session_end(&margin_left));
        if let Some(payload) = caught.into_payload() {
            report_contained(host, SessionEndStep::Close, payload);
        }
    }
    let mut caught = FirstPanic::new();
    let unfinished = caught.contain(|| host.flush(deadline)).flatten();
    if let Some(payload) = caught.into_payload() {
        report_contained(host, SessionEndStep::Flush, payload);
    }
    host.flush_host_state(deadline);
    if ends_application {
        // The UI runtimes are torn down only while their destructors can
        // start within the budget and none is on the stack; otherwise they
        // are left as they are and the system ends the process.
        let tear_down = margin_left() && !host.is_runtime_busy();
        host.end_application(tear_down);
    }
    match (support, unfinished) {
        (Some(SessionEndSupport::Background { window }), Some(writes)) => {
            host.hold_background(writes, clock.now() + window)
        }
        _ => SessionEndAnswer::Proceed,
    }
}

/// The web's `pagehide`: the browser grants no synchronous budget and the
/// page may come back from its cache, so the notice is delivered and the
/// flush registry keeps writing on its own; nothing waits, closes or quits.
fn end_page(host: &mut dyn SessionEndHost) -> SessionEndAnswer {
    let _delivery = deliver_contained(host, &HostNotice::SessionEnded, &|| true);
    SessionEndAnswer::Proceed
}

/// Hand the panic `step` raised to `host`, containing whatever the host's
/// report runs (a `tracing` subscriber is user code): the session end goes on
/// to its next step, and the platform still gets its answer.
fn report_contained(
    host: &mut dyn SessionEndHost,
    step: SessionEndStep,
    payload: Box<dyn Any + Send>,
) {
    let mut caught = FirstPanic::new();
    caught.contain(|| host.contained(step, payload));
    // The step's own panic is the host's now; a panic of its report has
    // nowhere left to go.
    drop(caught);
}

impl SessionEndStep {
    /// Log the panic this step raised and drop its payload: how a host that
    /// answers a native callback reports it. A subscriber that panics on the
    /// event is contained, and both payloads are dropped.
    pub fn log(self, payload: Box<dyn Any + Send>) {
        let mut caught = FirstPanic::new();
        caught.contain(|| self.log_event(&*payload));
        discard_payload(payload);
        drop(caught);
    }

    fn log_event(self, payload: &(dyn Any + Send)) {
        let text = payload_text(payload).unwrap_or("<non-string panic payload>");
        match self {
            Self::Query => tracing::error!(
                panic = text,
                "asking a window at a session-end query panicked; the window refuses"
            ),
            Self::Notice => tracing::error!(
                panic = text,
                "a session-end subscriber panicked; the session end is answered regardless"
            ),
            Self::Close => {
                tracing::error!(
                    panic = text,
                    "a Detached observer panicked at the session's end"
                );
            }
            Self::Flush => tracing::error!(panic = text, "the session-end flush panicked"),
        }
    }
}
