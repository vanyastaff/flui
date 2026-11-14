use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_platform::{Launch, OpenTarget, Opened, SessionEndAnswer, SessionEndPhase, SystemNotice};

use super::testing::{AppCall, AppLog, Bound, bind, bound};
use super::*;

const SLEEP: AppCall = AppCall::Notice(SystemNotice::Sleep);
const WAKE: AppCall = AppCall::Notice(SystemNotice::Wake);
const LINK: AppCall = AppCall::Opened {
    urls: 1,
    files: 0,
    launch: Launch::Running,
};
const COLD_LINK: AppCall = AppCall::Opened {
    urls: 1,
    files: 0,
    launch: Launch::Cold,
};

/// A link the operating system opened the application with.
fn link(url: &str, launch: Launch) -> Opened {
    Opened::new([url.to_owned()], [], OpenTarget::Application, launch)
}

/// A report raised from inside a method is queued behind it and heard after
/// it returns, in order, never recursed into.
fn a_report_raised_inside_a_method_is_heard_after_it() {
    let app = AppLog::new();
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
            assert!(matches!(
                delivery.opened(link("myapp:///a", Launch::Running)),
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
            ("start", LINK),
            ("start", AppCall::Quit),
        ]
    );
}

/// A report raised while the handler answers a question waits for the
/// answer, as one raised inside any method does.
fn a_report_raised_inside_a_question_is_heard_after_the_answer() {
    let app = AppLog::new();
    let run = bound(app.clone());
    let delivery = Arc::clone(&run.delivery);
    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = Rc::clone(&log);
    app.react(move |call, _| {
        seen.borrow_mut().push(("start", call));
        if call == AppCall::ExitPolicy {
            let _ = delivery.host_notice(SystemNotice::Sleep);
            seen.borrow_mut().push(("end", call));
        }
    });
    assert!(matches!(
        run.delivery.exit_policy(),
        Called::Returned(false)
    ));
    assert_eq!(
        *log.borrow(),
        [
            ("start", AppCall::ExitPolicy),
            ("end", AppCall::ExitPolicy),
            ("start", SLEEP),
        ]
    );
}

/// What the platform reports before the run binds its handler, and while
/// `ready` runs, waits for `ready` and is heard once it returns.
fn a_report_before_ready_waits_for_it() {
    let app = AppLog::new();
    let delivery = Arc::new(AppDelivery::new());
    // A link that launched the application, reported before the run binds
    // its handler, as AppKit delivers `application:openURLs:` before
    // `applicationDidFinishLaunching:`.
    let cold = link("myapp:///note/3", Launch::Cold);
    assert!(matches!(delivery.opened(cold.clone()), Called::Vacant));
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
    assert_eq!(app.calls(), [AppCall::Ready, COLD_LINK, SLEEP, WAKE]);
    assert_eq!(app.opens(), [cold], "the link arrives whole");
}

/// Opened URLs that wait for the handler are bounded: past
/// `MAX_PENDING_OPENED` the newest is dropped, the others still arrive in
/// order, and the other reports are not counted against it. A host that
/// answers whoever handed it the launch gets the one past the bound back
/// unanswered, and answers one with room before the handler hears it.
fn opened_reports_waiting_for_ready_are_bounded() {
    let app = AppLog::new();
    let delivery = Arc::new(AppDelivery::new());
    let links = (0..=MAX_PENDING_OPENED)
        .map(|n| link(&format!("myapp:///note/{n}"), Launch::Cold))
        .collect::<Vec<_>>();
    for opened in &links {
        assert!(matches!(delivery.opened(opened.clone()), Called::Vacant));
    }
    // A host that answers whoever handed it a launch gets the one past the
    // bound back, and answers nothing.
    let mut answered = false;
    let refused = delivery.try_opened(link("myapp:///refused", Launch::Running), || {
        answered = true;
    });
    assert!(
        refused.is_err() && !answered,
        "past the bound, the launch is handed back unanswered"
    );
    assert!(matches!(
        delivery.host_notice(SystemNotice::Sleep),
        Called::Vacant
    ));
    let run = bind(delivery, app.clone());
    run.delivery.ready(run.owner()).expect("bootstrap");
    // A handler that did not take the launch hears it after `ready`, as one
    // report of what launched the application.
    let kept = links[..MAX_PENDING_OPENED]
        .iter()
        .flat_map(|opened| opened.urls.iter().cloned())
        .collect::<Vec<_>>();
    assert_eq!(
        app.opens(),
        [Opened::new(kept, [], OpenTarget::Application, Launch::Cold)],
        "the first, in order"
    );
    assert_eq!(app.calls().last(), Some(&SLEEP));
    // Once heard, the queue has room again.
    let next = link("myapp:///later", Launch::Running);
    assert!(matches!(
        run.delivery.opened(next.clone()),
        Called::Returned(())
    ));
    assert_eq!(app.opens().last(), Some(&next));
    // With room, the launch is answered once it is queued, before the
    // handler hears it.
    let answered_after = std::cell::Cell::new(None);
    let admitted = link("myapp:///admitted", Launch::Running);
    let called = run
        .delivery
        .try_opened(admitted.clone(), || {
            answered_after.set(Some(app.opens().len()));
        })
        .expect("room for the launch");
    assert!(matches!(called, Called::Returned(())));
    assert_eq!(
        answered_after.get(),
        Some(app.opens().len() - 1),
        "answered before the handler heard it"
    );
    assert_eq!(app.opens().last(), Some(&admitted));
}

/// What launched the application is the handler's to take inside `ready`
/// (`OwnerPlatform::take_launch_opened`): the cold reports for the first
/// one's target, joined in order. Taken, they are not reported again; the
/// cold report for another target and the reports of the running
/// application still are, after `ready`.
fn ready_takes_what_launched_the_application() {
    let app = AppLog::new();
    let delivery = Arc::new(AppDelivery::new());
    let window = OpenTarget::Window(
        flui_platform::WindowIdMint::new()
            .next()
            .expect("a fresh mint issues an id"),
    );
    let reports = [
        link("myapp:///note/1", Launch::Cold),
        Opened::new(["myapp:///note/2".to_owned()], [], window, Launch::Cold),
        link("myapp:///later", Launch::Running),
        link("myapp:///note/3", Launch::Cold),
    ];
    for opened in &reports {
        assert!(matches!(delivery.opened(opened.clone()), Called::Vacant));
    }
    let taken = Rc::new(std::cell::RefCell::new(Vec::new()));
    let into = Rc::clone(&taken);
    app.react(move |call, owner| {
        if call == AppCall::Ready {
            into.borrow_mut().extend(owner.take_launch_opened());
            assert_eq!(owner.take_launch_opened(), None, "taken once");
        }
    });
    let run = bind(delivery, app.clone());
    run.delivery.ready(run.owner()).expect("bootstrap");
    assert_eq!(
        *taken.borrow(),
        [Opened::new(
            ["myapp:///note/1".to_owned(), "myapp:///note/3".to_owned()],
            [],
            OpenTarget::Application,
            Launch::Cold,
        )]
    );
    assert_eq!(app.opens(), [reports[1].clone(), reports[2].clone()]);
    // Outside `ready` there is no launch to take.
    assert_eq!(run.owner().take_launch_opened(), None);
}
/// A bootstrap that fails leaves what was reported queued, not dropped.
fn a_failed_bootstrap_keeps_the_reports() {
    struct Failing(Rc<AppLog>);
    impl AppHandler for Failing {
        fn ready(&self, _: OwnerPlatform) -> Result<(), BootstrapError> {
            Err("no window".into())
        }
        fn quit_requested(&self, owner: &OwnerPlatform) {
            self.0.quit_requested(owner);
        }
        fn opened(&self, owner: &OwnerPlatform, opened: Opened) {
            self.0.opened(owner, opened);
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
        fn exit_policy(&self, owner: &OwnerPlatform) -> ExitAnswer {
            self.0.exit_policy(owner)
        }
        fn wake_deadline(&self, owner: &OwnerPlatform) -> Option<web_time::Instant> {
            self.0.wake_deadline(owner)
        }
    }
    let app = AppLog::new();
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
    let app = AppLog::new();
    let run = bound(app.clone());
    let delivery = Arc::clone(&run.delivery);
    app.react(move |call, _| match call {
        SLEEP => {
            let _ = delivery.host_notice(SystemNotice::Wake);
            let _ = delivery.opened(link("myapp:///a", Launch::Running));
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
    assert_eq!(
        app.calls(),
        [AppCall::Ready, SLEEP, WAKE, LINK, AppCall::Quit],
        "the link queued behind two panics is heard"
    );
    // The delivery carries on.
    assert!(matches!(run.delivery.reopen(), Called::Returned(())));
}

/// A report from another thread is queued for the owner and asks for an
/// owner turn, which delivers it on the owner thread.
fn a_report_from_another_thread_waits_for_an_owner_turn() {
    let app = AppLog::new();
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
        // A second instance's arguments arrive on a reader thread.
        assert!(matches!(
            delivery.opened(link("myapp:///a", Launch::Running)),
            Called::Vacant
        ));
        assert!(matches!(delivery.exit_policy(), Called::Vacant));
    })
    .join()
    .expect("the other thread reports");
    assert_eq!(
        turns.load(Ordering::SeqCst),
        2,
        "each report asked for an owner turn"
    );
    assert_eq!(app.calls(), [AppCall::Ready]);
    assert!(matches!(
        run.delivery.deliver_queued(),
        Called::Returned(())
    ));
    assert_eq!(app.calls(), [AppCall::Ready, SLEEP, LINK]);
}

/// The quit is one per run, however many times the platform reports it.
fn the_quit_is_heard_once() {
    let app = AppLog::new();
    let run = bound(app.clone());
    let _ = run.delivery.quit_requested();
    let _ = run.delivery.quit_requested();
    assert_eq!(app.calls(), [AppCall::Ready, AppCall::Quit]);
}

/// A question is asked at once, from inside a running method too, and its
/// answer comes back.
fn a_question_is_asked_at_once_even_from_inside_a_method() {
    let app = AppLog::new();
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

    let app = AppLog::new();
    let run = bound(app);
    run.delivery.release();
    assert!(matches!(run.delivery.wake_deadline(), Called::Vacant));

    let gone = bound(AppLog::new());
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
    struct ReportsOnDrop(Rc<AppLog>, Arc<AppDelivery>);
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
        fn opened(&self, _: &OwnerPlatform, _: Opened) {}
        fn session_end(&self, _: &OwnerPlatform, _: SessionEndPhase) -> SessionEndAnswer {
            SessionEndAnswer::Proceed
        }
        fn host_notice(&self, _: &OwnerPlatform, _: SystemNotice) {}
        fn exit_policy(&self, _: &OwnerPlatform) -> ExitAnswer {
            ExitAnswer::End
        }
        fn wake_deadline(&self, _: &OwnerPlatform) -> Option<web_time::Instant> {
            None
        }
    }
    let delivery = Arc::new(AppDelivery::new());
    let handler = Rc::new(ReportsOnDrop(AppLog::new(), Arc::clone(&delivery)));
    let run = bind(delivery, handler);
    run.delivery.ready(run.owner()).expect("bootstrap");
    run.delivery.release();
}

/// A loop that ends from inside a method — the web page and the Android
/// Activity quit, then release the handler — still delivers the quit raised
/// there, once: the release waits for the running delivery.
fn a_quit_raised_as_the_loop_ends_inside_a_method_is_heard_once() {
    // The way the web and Android loops end: report the quit, let go of the
    // handler.
    fn end_loop(delivery: &AppDelivery) {
        let _ = delivery.quit_requested();
        delivery.release();
    }

    let app = AppLog::new();
    let run = bound(app.clone());
    let delivery = Arc::clone(&run.delivery);
    app.react(move |call, _| {
        if call == SLEEP {
            end_loop(&delivery);
        }
    });
    let _ = run.delivery.host_notice(SystemNotice::Sleep);
    assert_eq!(app.calls(), [AppCall::Ready, SLEEP, AppCall::Quit]);
    end_loop(&run.delivery);
    assert_eq!(app.count(AppCall::Quit), 1);
    assert!(
        matches!(run.delivery.wake_deadline(), Called::Vacant),
        "the handler was released once the delivery ended"
    );

    let app = AppLog::new();
    let delivery = Arc::new(AppDelivery::new());
    let inside = Arc::clone(&delivery);
    app.react(move |call, _| {
        if call == AppCall::Ready {
            end_loop(&inside);
        }
    });
    let run = bind(delivery, app.clone());
    run.delivery.ready(run.owner()).expect("bootstrap");
    assert_eq!(app.calls(), [AppCall::Ready, AppCall::Quit]);
    assert!(matches!(run.delivery.exit_policy(), Called::Vacant));
}

/// A handler that leaves the exit to the host is answered for as if no
/// handler were bound: the host's own default decides.
fn an_exit_policy_left_to_the_host_is_the_hosts() {
    let app = AppLog::new();
    let run = bound(app.clone());
    app.answer_exit_policy(ExitAnswer::HostDecides);
    assert!(matches!(run.delivery.exit_policy(), Called::Vacant));
    app.answer_exit_policy(ExitAnswer::End);
    assert!(matches!(run.delivery.exit_policy(), Called::Returned(true)));
    app.answer_exit_policy(ExitAnswer::KeepRunning);
    assert!(matches!(
        run.delivery.exit_policy(),
        Called::Returned(false)
    ));
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
                "a report raised inside a question is heard after the answer",
                a_report_raised_inside_a_question_is_heard_after_the_answer,
            ),
            (
                "a report before ready waits for it",
                a_report_before_ready_waits_for_it,
            ),
            (
                "opened reports waiting for ready are bounded",
                opened_reports_waiting_for_ready_are_bounded,
            ),
            (
                "ready takes what launched the application",
                ready_takes_what_launched_the_application,
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
            (
                "a quit raised as the loop ends inside a method is heard once",
                a_quit_raised_as_the_loop_ends_inside_a_method_is_heard_once,
            ),
            (
                "an exit policy left to the host is the host's",
                an_exit_policy_left_to_the_host_is_the_hosts,
            ),
        ],
    );
}
