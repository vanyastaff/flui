//! A quit's consultation keeps a panic of the code it runs instead of
//! unwinding: the quit is cancelled as a panicked consultation, the host
//! finishes it with nothing unwinding, and the panic resumes once. The
//! router's own report of a handler's panic never unwinds into its caller.

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

/// Panics as it drops, as a handler's capture whose destructor fails does.
struct FailsToDrop;

impl Drop for FailsToDrop {
    fn drop(&mut self) {
        panic!("the handler's capture failed to drop");
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

/// A presentation whose close handler closes its own window from inside its
/// answer, its capture failing to drop: the handler goes as the router lets
/// go of it, after its answer, so the panic escapes the router's question.
/// `ask` keeps it and returns, with a subscriber that fails on every event
/// installed; the host finishes the quit, which is cancelled as a panicked
/// consultation, then reports it to the same failing subscriber, which the
/// finished consultation keeps behind the first; resuming raises the
/// handler's drop, once. The next quit is asked again, and a handler's own
/// panic, which the router contains, cancels it as a refusal, not a panic.
///
/// Fails against a consultation that lets a presentation's panic unwind out
/// of `ask`.
fn a_quit_whose_consultation_panics_is_cancelled_and_resumed_once() {
    let router = Rc::new(CloseRequestRouter::new());
    let window: Rc<dyn PlatformWindow> = Rc::new(flui_platform::testing::MockWindow::new());
    let (closes_itself, refuses) = (address(1), address(2));
    let own_router = Rc::downgrade(&router);
    let capture = FailsToDrop;
    register(
        &router,
        closes_itself,
        &window,
        CloseRequestHandler::new(move |_| {
            let _ = &capture;
            let router = own_router.upgrade().expect("the router asks");
            drop(router.take(closes_itself));
            CloseResponse::Close
        }),
    );

    let mut quit = QuitRequest::new();
    quit.ask();
    let consultation = quit.begin_consult().expect("a quit is asked");
    let finished = {
        let _diagnostics = crate::scoped_diagnostics(FailsOnEveryEvent);
        let answered = catch_unwind(AssertUnwindSafe(|| {
            consultation.ask(&router, [closes_itself])
        }))
        .expect("the consultation keeps the drop's panic instead of unwinding");
        let mut finished = quit.finish_consult(answered);
        assert_eq!(finished.verdict(), QuitVerdict::Abandoned);
        assert!(
            finished.cancelled_by_panic(),
            "cancelled as a panic, not a refusal"
        );
        // A host reports the cancelled quit to the same subscriber.
        finished.contain(|| tracing::info!("quit cancelled"));
        assert!(
            finished.cancelled_by_panic(),
            "what cancelled the quit is fixed when it finished"
        );
        finished
    };
    assert!(!quit.is_pending(), "the quit is cancelled");
    let resumed =
        catch_unwind(AssertUnwindSafe(|| finished.resume())).expect_err("the kept panic resumes");
    assert_eq!(
        resumed.downcast_ref::<&str>().copied(),
        Some("the handler's capture failed to drop"),
        "the first panic, the handler's drop, is the one resumed"
    );

    register(
        &router,
        refuses,
        &window,
        CloseRequestHandler::new(|request| {
            assert_ne!(
                request.reason(),
                CloseReason::Program,
                "deliberate handler panic under test"
            );
            CloseResponse::Close
        }),
    );
    quit.ask();
    let consultation = quit.begin_consult().expect("the next quit is asked again");
    let mut finished = quit.finish_consult(consultation.ask(&router, [refuses]));
    assert_eq!(finished.verdict(), QuitVerdict::Abandoned);
    assert!(
        !finished.cancelled_by_panic(),
        "a handler's contained panic is a refusal without a record"
    );
    // A host's follow-up that panics is kept to resume, and changes nothing
    // of why the quit was cancelled.
    finished.contain(|| panic!("the host's follow-up failed"));
    assert!(
        !finished.cancelled_by_panic(),
        "a refusal stays a refusal after the host's follow-up panicked"
    );
    let resumed =
        catch_unwind(AssertUnwindSafe(|| finished.resume())).expect_err("the follow-up resumes");
    assert_eq!(
        resumed.downcast_ref::<&str>().copied(),
        Some("the host's follow-up failed")
    );
}

/// A handler that panics is contained and reported by the router, and a
/// subscriber that fails on that report is contained there too: whoever asks
/// the router hears a refusal, not a panic.
///
/// Fails against a router that reports a handler's panic uncontained.
fn the_router_contains_the_report_of_a_handlers_panic() {
    let router = CloseRequestRouter::new();
    let window: Rc<dyn PlatformWindow> = Rc::new(flui_platform::testing::MockWindow::new());
    let at = address(1);
    register(
        &router,
        at,
        &window,
        CloseRequestHandler::new(|_| panic!("deliberate handler panic under test")),
    );
    let _diagnostics = crate::scoped_diagnostics(FailsOnEveryEvent);
    for reason in [
        CloseReason::User,
        CloseReason::Program,
        CloseReason::SessionEnd,
    ] {
        let answered = catch_unwind(AssertUnwindSafe(|| router.consult(at, reason)))
            .expect("the router's report does not unwind into its caller");
        assert_eq!(answered, CloseResponse::KeepOpen, "{reason:?}");
    }
}

#[test]
fn quit_consultation() {
    crate::table_test::run_table(
        "quit_consultation",
        &[
            (
                "a_quit_whose_consultation_panics_is_cancelled_and_resumed_once",
                a_quit_whose_consultation_panics_is_cancelled_and_resumed_once as fn(),
            ),
            (
                "the_router_contains_the_report_of_a_handlers_panic",
                the_router_contains_the_report_of_a_handlers_panic as fn(),
            ),
        ],
    );
}
