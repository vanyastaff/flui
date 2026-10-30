//! The application's handler, passed to `run` (ADR-0254 §4): one row per
//! obligation, each heard once, on the headless host and, for the wake
//! deadline the headless host never asks, on Win32. What the platform reports
//! before the handler is bound waits for it; a panicking method is contained
//! and the first panic kept.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_native::{AppHandler, HeadlessPlatform, Platform, PlatformError};
use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice, WindowOptions};
use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::RecordingWindowHandler;

use crate::app::{AppCall, AppLog, run_keeping_owner};

/// `ready` runs once, with the owner capability, before anything else; its
/// failure is `run`'s.
fn ready_bootstraps_once_and_its_failure_is_runs() {
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &log);
    assert_eq!(owner.shared().name(), "Headless");
    assert_eq!(log.calls(), [AppCall::Ready]);

    let failing = AppLog::new();
    failing.bootstrap(|_| Err("no window".into()));
    let ran = Box::new(HeadlessPlatform::new()).run(Rc::clone(&failing) as Rc<dyn AppHandler>);
    assert!(matches!(ran, Err(PlatformError::Bootstrap { .. })));
    assert_eq!(failing.calls(), [AppCall::Ready]);
}

/// The quit is heard once, however many times the platform quits.
fn the_quit_is_heard_once() {
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &log);
    owner.quit();
    owner.quit();
    assert_eq!(log.calls(), [AppCall::Ready, AppCall::Quit]);
}

/// A reopen reaches the handler once per report.
fn a_reopen_is_heard_once() {
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let log = AppLog::new();
    let _owner = run_keeping_owner(Box::new(platform), &log);
    events.reopen();
    assert_eq!(log.calls(), [AppCall::Ready, AppCall::Reopen]);
}

/// A session end asks the handler once per phase and returns its answer.
fn a_session_end_returns_the_handlers_answer() {
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let log = AppLog::new();
    log.answer_session_end(SessionEndAnswer::Block);
    let _owner = run_keeping_owner(Box::new(platform), &log);
    assert_eq!(
        events.session_end(SessionEndPhase::Query),
        SessionEndAnswer::Block
    );
    assert_eq!(
        log.calls(),
        [AppCall::Ready, AppCall::SessionEnd(SessionEndPhase::Query)]
    );
}

/// A host notice reaches the handler once.
fn a_host_notice_is_heard_once() {
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let log = AppLog::new();
    let _owner = run_keeping_owner(Box::new(platform), &log);
    events.notice(SystemNotice::SessionLocked);
    assert_eq!(
        log.calls(),
        [AppCall::Ready, AppCall::Notice(SystemNotice::SessionLocked)]
    );
}

/// The last window closing asks the exit policy once; consent quits.
fn the_exit_policy_is_asked_when_the_last_window_closes() {
    let log = AppLog::new();
    log.answer_exit(true);
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &log);
    owner
        .open_window(WindowOptions::default(), RecordingWindowHandler::new())
        .expect("a window")
        .try_ready()
        .expect("at once")
        .close_now();
    assert_eq!(
        log.calls(),
        [AppCall::Ready, AppCall::ExitPolicy, AppCall::Quit]
    );
}

/// What the platform reports before `run` binds the handler, and while its
/// bootstrap runs, waits and is heard after `ready`, once, in order.
fn a_report_before_the_handler_is_heard_after_ready() {
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    events.notice(SystemNotice::Sleep);
    let log = AppLog::new();
    let inside = events;
    let kept = Rc::new(Cell::new(None));
    let keep = Rc::clone(&kept);
    log.bootstrap(move |owner| {
        inside.notice(SystemNotice::Wake);
        keep.set(Some(Rc::new(owner)));
        Ok(())
    });
    Box::new(platform)
        .run(Rc::clone(&log) as Rc<dyn AppHandler>)
        .expect("the platform runs");
    assert_eq!(
        log.calls(),
        [
            AppCall::Ready,
            AppCall::Notice(SystemNotice::Sleep),
            AppCall::Notice(SystemNotice::Wake),
        ]
    );
    drop(kept.take());
}

/// A notice that panics is contained: the report queued behind it, which
/// panics too, is still heard, and the caller gets the first panic.
fn a_panicking_method_keeps_the_first_panic() {
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    let log = AppLog::new();
    let _owner = run_keeping_owner(Box::new(platform), &log);
    let inside = events.clone();
    log.react(move |call, _| match call {
        AppCall::Notice(SystemNotice::Sleep) => {
            inside.notice(SystemNotice::Wake);
            std::panic::panic_any("first");
        }
        AppCall::Notice(SystemNotice::Wake) => std::panic::panic_any("second"),
        _ => {}
    });
    let failure = catch_unwind(AssertUnwindSafe(|| events.notice(SystemNotice::Sleep)))
        .expect_err("the method's panic reaches the caller");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"first"));
    assert_eq!(
        log.calls(),
        [
            AppCall::Ready,
            AppCall::Notice(SystemNotice::Sleep),
            AppCall::Notice(SystemNotice::Wake),
        ]
    );
}

/// The Win32 loop asks the wake deadline once per idle iteration, and a
/// deadline that comes due wakes the loop with nothing else to do.
#[cfg(target_os = "windows")]
fn the_wake_deadline_is_asked_and_kept() {
    let log = AppLog::new();
    // The deadline is 30 ms after the first question, not after the test
    // starts: on a loaded host, making the platform and starting its loop
    // takes longer than that, and a deadline already due when first asked
    // quits the loop on that one question.
    let due = Rc::new(std::cell::OnceCell::new());
    let answer = Rc::clone(&due);
    log.answer_deadline_with(move || {
        Some(
            *answer.get_or_init(|| web_time::Instant::now() + std::time::Duration::from_millis(30)),
        )
    });
    // The reaction runs before the answer, so the first question finds no
    // deadline yet and never quits.
    log.react(move |call, owner| {
        if call == AppCall::WakeDeadline
            && due
                .get()
                .is_some_and(|due| web_time::Instant::now() >= *due)
        {
            let _ = owner.proxy().request_quit();
        }
    });
    Box::new(flui_native::WindowsPlatform::try_new().expect("native Windows platform"))
        .run(Rc::clone(&log) as Rc<dyn AppHandler>)
        .expect("the loop ends on the quit asked once the deadline came");
    assert!(
        log.count(AppCall::WakeDeadline) >= 2,
        "asked again at the deadline"
    );
    assert_eq!(log.count(AppCall::Quit), 1);
}

#[test]
fn app_handler_obligation_table() {
    crate::run_table(
        "app_handler_obligation_table",
        &[
            (
                "ready bootstraps once and its failure is run's",
                ready_bootstraps_once_and_its_failure_is_runs,
            ),
            ("the quit is heard once", the_quit_is_heard_once),
            ("a reopen is heard once", a_reopen_is_heard_once),
            (
                "a session end returns the handler's answer",
                a_session_end_returns_the_handlers_answer,
            ),
            ("a host notice is heard once", a_host_notice_is_heard_once),
            (
                "the exit policy is asked when the last window closes",
                the_exit_policy_is_asked_when_the_last_window_closes,
            ),
            (
                "a report before the handler is heard after ready",
                a_report_before_the_handler_is_heard_after_ready,
            ),
            (
                "a panicking method keeps the first panic",
                a_panicking_method_keeps_the_first_panic,
            ),
            #[cfg(target_os = "windows")]
            (
                "the wake deadline is asked and kept",
                the_wake_deadline_is_asked_and_kept,
            ),
        ],
    );
}
