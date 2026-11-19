//! A quit's consultation keeps a panic of the code it runs instead of
//! unwinding: the quit is cancelled as a panicked consultation, the host
//! finishes it with nothing unwinding, and the panic resumes once.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::{PresentationAddress, PresentationId, UiRuntimeId};
use flui_platform::PlatformWindow;
use flui_runtime::owner_turn::{
    CloseRequestHandler, CloseRequestRouter, CloseResponse, PreparedCloseRequest, QuitRequest,
    QuitVerdict,
};
use flui_view::CloseReason;

/// A log subscriber that fails on every event, whatever its target, as a
/// faulty one does; the panic names the event's target.
struct FailsOnEveryEvent;

impl tracing::Subscriber for FailsOnEveryEvent {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        panic!("log subscriber failed on {}", event.metadata().target());
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// A router with one presentation, whose close handler panics at the
/// program's close: the router contains that panic and reports it, and the
/// report reaches a subscriber that fails on every event. `ask` keeps that
/// panic and returns; the host finishes the quit, which is cancelled as a
/// panicked consultation, then reports it to the same failing subscriber,
/// which the finished consultation keeps behind the first; resuming raises
/// the router's report, once. The next quit is asked again, and a handler's
/// own contained panic then cancels it as a refusal, not a panic.
///
/// Fails against a consultation that lets a presentation's panic unwind out
/// of `ask`.
fn a_quit_whose_consultation_panics_is_cancelled_and_resumed_once() {
    let router = CloseRequestRouter::new();
    let window: Rc<dyn PlatformWindow> = Rc::new(flui_runtime::testing::TestWindow::new());
    let at = PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(1),
        presentation_id: PresentationId::new(1),
    };
    let prepared = PreparedCloseRequest::new(
        at,
        &window,
        Some(CloseRequestHandler::new(|request| {
            assert_ne!(
                request.reason(),
                CloseReason::Program,
                "deliberate handler panic under test"
            );
            CloseResponse::Close
        })),
    );
    router
        .publication(&prepared)
        .expect("a fresh router has room")
        .publish(prepared);

    let mut quit = QuitRequest::new();
    quit.ask();
    let consultation = quit.begin_consult().expect("a quit is asked");
    let finished = {
        let _diagnostics = crate::scoped_diagnostics(FailsOnEveryEvent);
        let answered = catch_unwind(AssertUnwindSafe(|| consultation.ask(&router, [at])))
            .expect("the consultation keeps the subscriber's panic instead of unwinding");
        let mut finished = quit.finish_consult(answered);
        assert_eq!(finished.verdict(), QuitVerdict::Abandoned);
        assert!(finished.panicked(), "cancelled as a panic, not a refusal");
        // A host reports the cancelled quit to the same subscriber.
        finished.contain(|| tracing::info!("quit cancelled"));
        finished
    };
    assert!(!quit.is_pending(), "the quit is cancelled");
    let resumed =
        catch_unwind(AssertUnwindSafe(|| finished.resume())).expect_err("the kept panic resumes");
    let message = resumed
        .downcast_ref::<String>()
        .expect("the subscriber panics with a message");
    assert!(
        message.contains("flui_runtime::owner_turn"),
        "the first panic, the router's report, is the one resumed: {message}"
    );

    quit.ask();
    let consultation = quit.begin_consult().expect("the next quit is asked again");
    let finished = quit.finish_consult(consultation.ask(&router, [at]));
    assert_eq!(finished.verdict(), QuitVerdict::Abandoned);
    assert!(
        !finished.panicked(),
        "a handler's contained panic is a refusal without a record"
    );
    finished.resume();
}

#[test]
fn quit_consultation() {
    crate::table_test::run_table(
        "quit_consultation",
        &[(
            "a_quit_whose_consultation_panics_is_cancelled_and_resumed_once",
            a_quit_whose_consultation_panics_is_cancelled_and_resumed_once as fn(),
        )],
    );
}
