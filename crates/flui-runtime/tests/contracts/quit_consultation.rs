//! A quit's consultation ends on every exit: a panic that unwinds out of the
//! code it runs cancels the quit instead of leaving it asked forever.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_foundation::{PresentationAddress, PresentationId, UiRuntimeId};
use flui_platform::PlatformWindow;
use flui_runtime::owner_turn::{
    CloseRequestHandler, CloseRequestRouter, CloseResponse, PreparedCloseRequest, QuitConsultation,
    QuitRequest, QuitVerdict,
};
use flui_view::CloseReason;

/// A log subscriber that fails on every event of the owner turn's steps, as
/// a faulty one does.
struct FailsOnOwnerTurnEvents;

impl tracing::Subscriber for FailsOnOwnerTurnEvents {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if event
            .metadata()
            .target()
            .starts_with("flui_runtime::owner_turn")
        {
            panic!("log subscriber failed");
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// A router with one presentation, whose close handler panics at the
/// program's close: the router contains that panic and reports it, and the
/// report reaches a subscriber that fails, so the panic unwinds out of the
/// consultation. The quit is cancelled, not left in its consultation, and the
/// next quit the application asks for is asked again.
///
/// Fails against a consultation that ends only when `ask` returns: the quit
/// stays in its consultation, pending, and is never asked again.
fn a_quit_whose_consultation_unwinds_is_cancelled_and_asked_again() {
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
    assert!(quit.begin_consult());
    let unwound = {
        let _diagnostics = crate::scoped_diagnostics(FailsOnOwnerTurnEvents);
        catch_unwind(AssertUnwindSafe(|| {
            QuitConsultation::new(|answer| quit.finish_consult(answer)).ask(&router, [at])
        }))
    };
    assert!(
        unwound.is_err(),
        "the subscriber's panic unwinds out of the consultation"
    );
    assert!(
        !quit.is_pending(),
        "the unwound consultation cancelled the quit"
    );

    quit.ask();
    assert!(quit.begin_consult(), "the next quit is asked again");
    assert_eq!(
        QuitConsultation::new(|answer| quit.finish_consult(answer)).ask(&router, [at]),
        QuitVerdict::Abandoned,
        "the handler's contained panic refuses the quit without a record"
    );
}

#[test]
fn quit_consultation() {
    crate::table_test::run_table(
        "quit_consultation",
        &[(
            "a_quit_whose_consultation_unwinds_is_cancelled_and_asked_again",
            a_quit_whose_consultation_unwinds_is_cancelled_and_asked_again as fn(),
        )],
    );
}
