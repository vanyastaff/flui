//! The session end's steps over a host that records what they ask of it:
//! a window whose question panics at the query refuses and the windows after
//! it are still asked, and a host whose report of that panic fails costs the
//! query neither its notice nor its refusals.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::panic::discard_payload;
use flui_foundation::{
    ManualClock, MonotonicClock, PresentationAddress, PresentationId, UiRuntimeId,
};
use flui_platform::{
    HostNotice, PlatformWindow, SessionEndAnswer, SessionEndPhase, SessionEndSupport,
    TargetPlatform,
};
use flui_runtime::owner_turn::{
    CloseRequestHandler, CloseRequestRouter, CloseResponse, NoticeDelivery, PreparedCloseRequest,
    SessionEnd, SessionEndHost, SessionEndStep,
};
use flui_runtime::window_system::WindowSystem;
use flui_view::__runtime::WritesInFlight;
use flui_view::CloseReason;

/// Panics as it drops, as a handler's capture whose destructor fails does.
struct FailsToDrop;

impl Drop for FailsToDrop {
    fn drop(&mut self) {
        panic!("the handler's capture failed to drop");
    }
}

/// A desktop host whose windows are `presentations`, recording the notices
/// it delivers, the panics it is handed and the platform's releases.
struct RecordingHost {
    router: Rc<CloseRequestRouter>,
    presentations: Vec<PresentationAddress>,
    notices: Vec<&'static str>,
    contained: Vec<SessionEndStep>,
    /// Whether the host's report of a contained panic fails.
    report_fails: bool,
    releases: Rc<Cell<usize>>,
}

impl SessionEndHost for RecordingHost {
    fn support(&self) -> Option<SessionEndSupport> {
        Some(SessionEndSupport::Hold)
    }

    fn window_system(&self) -> WindowSystem {
        WindowSystem::Platform(TargetPlatform::Windows)
    }

    fn clock(&self) -> Rc<dyn MonotonicClock> {
        Rc::new(ManualClock::new())
    }

    fn router(&self) -> Option<Rc<CloseRequestRouter>> {
        Some(Rc::clone(&self.router))
    }

    fn askable_presentations(&self) -> Vec<PresentationAddress> {
        self.presentations.clone()
    }

    fn deliver_notice(
        &mut self,
        notice: &HostNotice,
        _may_start: &dyn Fn() -> bool,
    ) -> NoticeDelivery {
        self.notices.push(match notice {
            HostNotice::SessionEnding(_) => "ending",
            _ => "other",
        });
        let mut delivery = NoticeDelivery::default();
        delivery.ran += 1;
        delivery
    }

    fn release_hook(&self) -> Box<dyn FnOnce()> {
        let releases = Rc::clone(&self.releases);
        Box::new(move || releases.set(releases.get() + 1))
    }

    fn close_for_session_end(&mut self, _may_start: &dyn Fn() -> bool) {}

    fn flush(&mut self, _deadline: web_time::Instant) -> Option<WritesInFlight> {
        None
    }

    fn is_runtime_busy(&self) -> bool {
        false
    }

    fn end_application(&mut self, _tear_down: bool) {}

    fn hold_background(
        &mut self,
        _writes: WritesInFlight,
        _until: web_time::Instant,
    ) -> SessionEndAnswer {
        SessionEndAnswer::Proceed
    }

    fn drop_background_hold(&mut self) {}

    fn contained(&mut self, step: SessionEndStep, payload: Box<dyn Any + Send>) {
        self.contained.push(step);
        discard_payload(payload);
        assert!(!self.report_fails, "the host's report failed");
    }
}

fn address(presentation: usize) -> PresentationAddress {
    PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(1),
        presentation_id: PresentationId::new(presentation),
    }
}

fn register(
    router: &CloseRequestRouter,
    at: PresentationAddress,
    window: &Rc<dyn PlatformWindow>,
    handler: CloseRequestHandler,
) {
    let prepared = PreparedCloseRequest::new(at, window, Some(handler));
    router
        .publication(&prepared)
        .expect("a fresh address has room")
        .publish(prepared);
}

/// Two windows: the first one's handler closes its own window from inside
/// its answer and its capture fails to drop, so its question panics past the
/// handler; the second's handler records what it is asked and lets the
/// session end. For each host report, failing or not: the query answers
/// `Block` (the first window refuses), the second window is asked, the
/// notice is delivered, the host is handed the query's panic, and the
/// refusal holds the platform until its window carries the close out.
///
/// Fails against a query that asks the windows uncontained (the panic
/// unwinds out of `answer`) and against a session end that hands the host
/// its panic uncontained (the failing report unwinds out of `answer`).
fn a_window_whose_question_panics_refuses_and_a_failing_report_costs_nothing() {
    for report_fails in [false, true] {
        let router = Rc::new(CloseRequestRouter::new());
        let windows: [Rc<dyn PlatformWindow>; 2] = [
            Rc::new(flui_testing::MockWindow::new()),
            Rc::new(flui_testing::MockWindow::new()),
        ];
        let (panics, asked_after) = (address(1), address(2));
        let own_router = Rc::downgrade(&router);
        let capture = FailsToDrop;
        register(
            &router,
            panics,
            &windows[0],
            CloseRequestHandler::new(move |_| {
                let _ = &capture;
                let router = own_router.upgrade().expect("the router asks");
                drop(router.take(panics));
                CloseResponse::Close
            }),
        );
        let asked = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&asked);
        register(
            &router,
            asked_after,
            &windows[1],
            CloseRequestHandler::new(move |request| {
                recorded.borrow_mut().push(request.reason());
                CloseResponse::Close
            }),
        );
        let mut host = RecordingHost {
            router,
            presentations: vec![panics, asked_after],
            notices: Vec::new(),
            contained: Vec::new(),
            report_fails,
            releases: Rc::default(),
        };
        let book = SessionEnd::new();

        assert_eq!(
            book.answer(SessionEndPhase::Query, &mut host),
            SessionEndAnswer::Block,
            "report_fails={report_fails}: the window whose question panicked refuses"
        );
        assert_eq!(
            *asked.borrow(),
            [CloseReason::SessionEnd],
            "report_fails={report_fails}: the window after it is asked"
        );
        assert_eq!(host.notices, ["ending"], "report_fails={report_fails}");
        assert_eq!(
            host.contained,
            [SessionEndStep::Query],
            "report_fails={report_fails}"
        );
        assert_eq!(host.releases.get(), 0, "report_fails={report_fails}");
        book.release_refusal(panics);
        assert_eq!(
            host.releases.get(),
            1,
            "report_fails={report_fails}: its close released the platform"
        );
    }
}

#[test]
fn session_end_steps() {
    crate::table_test::run_table(
        "session_end_steps",
        &[(
            "a_window_whose_question_panics_refuses_and_a_failing_report_costs_nothing",
            a_window_whose_question_panics_refuses_and_a_failing_report_costs_nothing as fn(),
        )],
    );
}
