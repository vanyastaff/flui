use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice};

use super::testing::{AppCall, Bound, RecordingApp, bind, bound};
use super::*;

const SLEEP: AppCall = AppCall::Notice(SystemNotice::Sleep);
const WAKE: AppCall = AppCall::Notice(SystemNotice::Wake);

/// A report raised from inside a method is queued behind it and heard after
/// it returns, in order, never recursed into.
fn a_report_raised_inside_a_method_is_heard_after_it() {
    let app = RecordingApp::new();
    let run = bound(app.clone());
    let delivery = Arc::clone(&run.delivery);
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = Rc::clone(&log);
    app.react(move |call, _| {
        seen.borrow_mut().push(("start", call));
        if call == SLEEP {
            assert!(matches!(
                delivery.host_notice(SystemNotice::Wake),
                Called::Vacant
            ));
            assert!(matches!(delivery.quit_requested(), Called::Vacant));
            seen.borrow_mut().push(("end", call));
        }
    });

    assert!(matches!(
        run.delivery.host_notice(SystemNotice::Sleep),
        Called::Returned(())
    ));
    assert_eq!(
        *log.borrow(),
        [
            ("start", SLEEP),
            ("end", SLEEP),
            ("start", WAKE),
            ("start", AppCall::Quit),
        ]
    );
}

/// What the platform reports before the run binds its handler, and while
/// `ready` runs, waits for `ready` and is heard once it returns.
fn a_report_before_ready_waits_for_it() {
    let app = RecordingApp::new();
    let delivery = Arc::new(AppDelivery::new());
    assert!(matches!(
        delivery.host_notice(SystemNotice::Sleep),
        Called::Vacant
    ));
    let inside = Arc::clone(&delivery);
    app.react(move |call, _| {
        if call == AppCall::Ready {
            assert!(matches!(
                inside.host_notice(SystemNotice::Wake),
                Called::Vacant
            ));
        }
    });
    let run = bind(delivery, app.clone());
    run.delivery.ready(run.owner()).expect("bootstrap");
    assert_eq!(app.calls(), [AppCall::Ready, SLEEP, WAKE]);
}

/// A bootstrap that fails leaves what was reported queued, not dropped.
fn a_failed_bootstrap_keeps_the_reports() {
    struct Failing(Rc<RecordingApp>);
    impl AppHandler for Failing {
        fn ready(&self, _: OwnerPlatform) -> Result<(), BootstrapError> {
            Err("no window".into())
        }
        fn quit_requested(&self, owner: &OwnerPlatform) {
            self.0.quit_requested(owner);
        }
        fn reopen(&self, owner: &OwnerPlatform) {
            self.0.reopen(owner);
        }
        fn session_end(&self, owner: &OwnerPlatform, phase: SessionEndPhase) -> SessionEndAnswer {
            self.0.session_end(owner, phase)
        }
        fn host_notice(&self, owner: &OwnerPlatform, notice: SystemNotice) {
            self.0.host_notice(owner, notice);
        }
        fn exit_policy(&self, owner: &OwnerPlatform) -> bool {
            self.0.exit_policy(owner)
        }
        fn wake_deadline(&self, owner: &OwnerPlatform) -> Option<web_time::Instant> {
            self.0.wake_deadline(owner)
        }
    }
    let app = RecordingApp::new();
    let delivery = Arc::new(AppDelivery::new());
    let _ = delivery.host_notice(SystemNotice::Sleep);
    let run = bind(delivery, Rc::new(Failing(app.clone())));
    assert!(run.delivery.ready(run.owner()).is_err());
    assert_eq!(app.calls(), []);
    assert!(matches!(
        run.delivery.deliver_queued(),
        Called::Returned(())
    ));
    assert_eq!(app.calls(), [SLEEP], "the report waited, it was not lost");
}

/// A panicking method is contained: the reports queued behind it are still
/// heard, each in its own containment, and the first panic comes back; a
/// second one does not replace it.
fn a_panicking_method_loses_no_report_and_keeps_the_first_panic() {
    let app = RecordingApp::new();
    let run = bound(app.clone());
    let delivery = Arc::clone(&run.delivery);
    app.react(move |call, _| match call {
        SLEEP => {
            let _ = delivery.host_notice(SystemNotice::Wake);
            let _ = delivery.quit_requested();
            std::panic::panic_any("first");
        }
        WAKE => std::panic::panic_any("second"),
        _ => {}
    });
    let Called::Panicked(failure) = run.delivery.host_notice(SystemNotice::Sleep) else {
        panic!("the method's panic was not returned");
    };
    let payload = catch_unwind(AssertUnwindSafe(|| failure.resume())).expect_err("resumed");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"first"));
    assert_eq!(app.calls(), [AppCall::Ready, SLEEP, WAKE, AppCall::Quit]);
    // The delivery carries on.
    assert!(matches!(
        run.delivery.reopen(),
        Called::Returned(())
    ));
}

/// A report from another thread is queued for the owner and asks for an
/// owner turn, which delivers it on the owner thread.
fn a_report_from_another_thread_waits_for_an_owner_turn() {
    let app = RecordingApp::new();
    let run = bound(app.clone());
    let turns = Arc::new(AtomicUsize::new(0));
    let asked = Arc::clone(&turns);
    run.delivery.resume_through(Arc::new(move || {
        asked.fetch_add(1, Ordering::SeqCst);
    }));
    let delivery = Arc::clone(&run.delivery);
    std::thread::spawn(move || {
        assert!(matches!(
            delivery.host_notice(SystemNotice::Sleep),
            Called::Vacant
        ));
        assert!(matches!(delivery.exit_policy(), Called::Vacant));
    })
    .join()
    .expect("the other thread reports");
    assert_eq!(turns.load(Ordering::SeqCst), 1, "it asked for an owner turn");
    assert_eq!(app.calls(), [AppCall::Ready]);
    assert!(matches!(
        run.delivery.deliver_queued(),
        Called::Returned(())
    ));
    assert_eq!(app.calls(), [AppCall::Ready, SLEEP]);
}

/// The quit is one per run, however many times the platform reports it.
fn the_quit_is_heard_once() {
    let app = RecordingApp::new();
    let run = bound(app.clone());
    let _ = run.delivery.quit_requested();
    let _ = run.delivery.quit_requested();
    assert_eq!(app.calls(), [AppCall::Ready, AppCall::Quit]);
}

/// A question is asked at once, from inside a running method too, and its
/// answer comes back.
fn a_question_is_asked_at_once_even_from_inside_a_method() {
    let app = RecordingApp::new();
    let run = bound(app.clone());
    let deadline = web_time::Instant::now();
    app.answer_deadline(Some(deadline));
    app.answer_exit(false);
    app.answer_session_end(SessionEndAnswer::Block);
    let delivery = Arc::clone(&run.delivery);
    let answers = Rc::new(std::cell::RefCell::new(None));
    let seen = Rc::clone(&answers);
    app.react(move |call, _| {
        if call == SLEEP {
            seen.replace(Some((
                delivery.wake_deadline().or_resume(None),
                delivery.exit_policy().or_resume(true),
                delivery
                    .session_end(SessionEndPhase::Query)
                    .or_resume(SessionEndAnswer::Proceed),
            )));
        }
    });
    let _ = run.delivery.host_notice(SystemNotice::Sleep);
    assert_eq!(
        *answers.borrow(),
        Some((Some(deadline), false, SessionEndAnswer::Block))
    );
    assert_eq!(
        app.calls(),
        [
            AppCall::Ready,
            SLEEP,
            AppCall::WakeDeadline,
            AppCall::ExitPolicy,
            AppCall::SessionEnd(SessionEndPhase::Query),
        ]
    );
}

/// With no handler bound, after its release, or once the platform is gone,
/// a question is the host's to answer.
fn a_question_with_no_handler_is_vacant() {
    let delivery = Arc::new(AppDelivery::new());
    assert!(matches!(delivery.exit_policy(), Called::Vacant));

    let app = RecordingApp::new();
    let run = bound(app);
    run.delivery.release();
    assert!(matches!(run.delivery.wake_deadline(), Called::Vacant));

    let gone = bound(RecordingApp::new());
    let Bound {
        delivery,
        platform,
        hooks,
    } = gone;
    drop((platform, hooks));
    assert!(matches!(
        delivery.session_end(SessionEndPhase::Query),
        Called::Vacant
    ));
}

/// The released handler is dropped outside every lock: its destructor may
/// reach the delivery again.
fn a_released_handler_drops_outside_the_delivery_lock() {
    struct ReportsOnDrop(Rc<RecordingApp>, Arc<AppDelivery>);
    impl Drop for ReportsOnDrop {
        fn drop(&mut self) {
            let _ = self.1.host_notice(SystemNotice::Sleep);
            let _ = self.1.exit_policy();
        }
    }
    impl AppHandler for ReportsOnDrop {
        fn ready(&self, owner: OwnerPlatform) -> Result<(), BootstrapError> {
            self.0.ready(owner)
        }
        fn quit_requested(&self, _: &OwnerPlatform) {}
        fn reopen(&self, _: &OwnerPlatform) {}
        fn session_end(&self, _: &OwnerPlatform, _: SessionEndPhase) -> SessionEndAnswer {
            SessionEndAnswer::Proceed
        }
        fn host_notice(&self, _: &OwnerPlatform, _: SystemNotice) {}
        fn exit_policy(&self, _: &OwnerPlatform) -> bool {
            true
        }
        fn wake_deadline(&self, _: &OwnerPlatform) -> Option<web_time::Instant> {
            None
        }
    }
    let delivery = Arc::new(AppDelivery::new());
    let handler = Rc::new(ReportsOnDrop(RecordingApp::new(), Arc::clone(&delivery)));
    let run = bind(delivery, handler);
    run.delivery.ready(run.owner()).expect("bootstrap");
    run.delivery.release();
}

/// The delivery's contracts: order, waiting, containment, threads, the one
/// quit, questions and release.
#[test]
fn app_delivery_contracts() {
    crate::table_test::run_table(
        "app delivery",
        &[
            (
                "a report raised inside a method is heard after it",
                a_report_raised_inside_a_method_is_heard_after_it,
            ),
            (
                "a report before ready waits for it",
                a_report_before_ready_waits_for_it,
            ),
            (
                "a failed bootstrap keeps the reports",
                a_failed_bootstrap_keeps_the_reports,
            ),
            (
                "a panicking method loses no report and keeps the first panic",
                a_panicking_method_loses_no_report_and_keeps_the_first_panic,
            ),
            (
                "a report from another thread waits for an owner turn",
                a_report_from_another_thread_waits_for_an_owner_turn,
            ),
            ("the quit is heard once", the_quit_is_heard_once),
            (
                "a question is asked at once even from inside a method",
                a_question_is_asked_at_once_even_from_inside_a_method,
            ),
            (
                "a question with no handler is vacant",
                a_question_with_no_handler_is_vacant,
            ),
            (
                "a released handler drops outside the delivery lock",
                a_released_handler_drops_outside_the_delivery_lock,
            ),
        ],
    );
}
