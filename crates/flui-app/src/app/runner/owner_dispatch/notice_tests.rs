//! System notices and session ends through the host (ADR-0237), driven end to
//! end through the headless platform: a real installed window, real owner turns,
//! the platform's own `simulate_*` entry points. Declared in `owner_dispatch.rs`
//! through `#[path]` because the host it exercises is private to the runner.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};
use flui_platform::traits::PlatformWindow;
use flui_platform::{
    HeadlessHostEvents, HeadlessPlatform, HostNotice, Platform as _, SessionEndAnswer,
    SessionEndBlock, SessionEndBlockError, SessionEndPhase, SystemNotice,
};
use flui_view::__runtime::BindingRuntime as _;
use flui_view::LifecycleHandle;

use super::super::host::{
    OwnerHostClearGuard, build_ui_runtime, install_owner_platform, with_owner_platform,
};
use super::*;

/// A booted headless host.
struct Rig {
    events: HeadlessHostEvents,
    _clear: OwnerHostClearGuard,
}

fn boot() -> Rig {
    let clear = OwnerHostClearGuard::arm();
    let platform = HeadlessPlatform::new();
    let events = platform.host_events();
    Box::new(platform)
        .run(Box::new(|owner| {
            install_owner_platform(owner)?;
            Ok(())
        }))
        .expect("headless run");
    Rig {
        events,
        _clear: clear,
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        teardown_platform_ui_runtime();
    }
}

/// An installed window and the lifecycle handle its widgets hear notices from.
struct Win {
    _window: Arc<dyn PlatformWindow>,
    dispatcher: PresentationDispatcher,
    lifecycle: LifecycleHandle,
}

fn open_window() -> Win {
    let host_window =
        with_owner_platform(|owner| owner.open_window(flui_platform::WindowOptions::default()))
            .expect("an installed owner")
            .and_then(flui_platform::WindowOpen::try_ready)
            .expect("headless windows are ready");
    let window: Arc<dyn PlatformWindow> = host_window;
    let runtime = build_ui_runtime(Arc::clone(&window), 1.0).expect("runtime");
    let installation = prepare_ui_runtime_alongside(runtime, Arc::clone(&window));
    let dispatcher = installation.dispatcher();
    installation
        .submit()
        .outcome()
        .expect("an idle host publishes synchronously")
        .expect("installed");
    let id = dispatcher.address.presentation_id;
    let handle = Rc::new(RefCell::new(None));
    let out = Rc::clone(&handle);
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |runtime| {
            *out.borrow_mut() = Some(
                runtime
                    .presentation_widgets_for_test(id)
                    .lifecycle_source()
                    .handle(),
            );
        })),
    )
    .expect("read the presentation");
    let lifecycle = handle.borrow_mut().take().expect("read synchronously");
    Win {
        _window: window,
        dispatcher,
        lifecycle,
    }
}

type Held = Rc<RefCell<Vec<SessionEndBlock>>>;

/// Subscribers that block a session end for as long as `held` keeps the block.
fn block_session_ends(win: &Win, held: &Held) -> flui_view::HostNoticeSubscription {
    let held = Rc::clone(held);
    win.lifecycle
        .subscribe_notices(move |notice| {
            if let HostNotice::SessionEnding(request) = notice {
                held.borrow_mut()
                    .push(request.block("unsaved work").expect("the book is open"));
            }
        })
        .expect("an open presentation")
}

fn a_query_is_answered_after_the_subscribers_ran_and_the_release_reaches_the_platform() {
    let rig = boot();
    let win = open_window();
    let held = Held::default();
    let _subscription = block_session_ends(&win, &held);

    assert_eq!(
        rig.events.session_end(SessionEndPhase::Query),
        SessionEndAnswer::Block,
        "the subscriber's block was known before the platform was answered"
    );
    assert_eq!(rig.events.session_end_releases(), 0, "still held");

    held.borrow_mut().clear();
    assert_eq!(
        rig.events.session_end_releases(),
        1,
        "dropping the last block tells the platform it may go on"
    );
}

fn a_query_with_nobody_blocking_proceeds_and_releases_nothing() {
    let rig = boot();
    let _win = open_window();
    assert_eq!(
        rig.events.session_end(SessionEndPhase::Query),
        SessionEndAnswer::Proceed
    );
    assert_eq!(rig.events.session_end_releases(), 0);
}

fn a_query_that_arrives_while_the_owner_is_busy_proceeds_and_closes_the_book() {
    let rig = boot();
    let win = open_window();
    let late = Rc::new(RefCell::new(Vec::new()));
    let heard = Rc::clone(&late);
    let _subscription = win
        .lifecycle
        .subscribe_notices(move |notice| {
            if let HostNotice::SessionEnding(request) = notice {
                heard.borrow_mut().push(request.block("late").map(|_| ()));
            }
        })
        .expect("an open presentation");

    // The query arrives from inside an owner turn, as a session end in a nested
    // native loop does: the runtime's turn is busy, so its subscribers cannot run
    // before the answer.
    let answer = Rc::new(Cell::new(None));
    let given = Rc::clone(&answer);
    let events = rig.events.clone();
    dispatch_platform_ui_runtime(
        win.dispatcher,
        RuntimeTask::TestCallback(Box::new(move |_| {
            given.set(Some(events.session_end(SessionEndPhase::Query)));
        })),
    )
    .expect("a nested query");

    assert_eq!(
        answer.get(),
        Some(SessionEndAnswer::Proceed),
        "an answer given before anyone could object is not a Block"
    );
    assert_eq!(
        *late.borrow(),
        [Err(SessionEndBlockError::Answered)],
        "the subscriber heard it afterwards, and its block was refused, not silently too late"
    );
    assert_eq!(rig.events.session_end_releases(), 0);
}

fn a_panicking_subscriber_still_gets_the_platform_answered_and_the_book_closed() {
    let rig = boot();
    let win = open_window();
    let seen: Rc<RefCell<Option<flui_platform::SessionEndRequest>>> = Rc::default();
    let kept = Rc::clone(&seen);
    let _subscription = win
        .lifecycle
        .subscribe_notices(move |notice| {
            if let HostNotice::SessionEnding(request) = notice {
                *kept.borrow_mut() = Some(request.clone());
                panic!("a subscriber fails");
            }
        })
        .expect("an open presentation");

    assert_eq!(
        rig.events.session_end(SessionEndPhase::Query),
        SessionEndAnswer::Proceed,
        "a panicking subscriber counts as proceed, and the platform is answered"
    );
    let request = seen.borrow_mut().take().expect("the subscriber ran");
    assert_eq!(
        request.block("late").map(|_| ()),
        Err(SessionEndBlockError::Answered),
        "the book closed with the answer"
    );
    assert_eq!(
        rig.events.session_end(SessionEndPhase::Query),
        SessionEndAnswer::Proceed,
        "the platform's callback survived the failure"
    );
}

struct Counting(Arc<AtomicUsize>);

impl ReclaimMemory for Counting {
    fn reclaim(&self, _level: MemoryPressureLevel) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn the_shared_memory_is_reclaimed_once_however_many_windows_there_are() {
    let rig = boot();
    let reclaimed = Arc::new(AtomicUsize::new(0));
    APP_RUNTIME.with(|slot| {
        slot.borrow()
            .host_memory
            .replace_receivers(vec![Arc::new(Counting(Arc::clone(&reclaimed)))]);
    });
    let first = open_window();
    let second = open_window();
    let third = open_window();

    let seen_by_subscribers = Rc::new(RefCell::new(Vec::new()));
    let subscriptions: Vec<_> = [&first, &second, &third]
        .into_iter()
        .map(|win| {
            let log = Rc::clone(&seen_by_subscribers);
            let counter = Arc::clone(&reclaimed);
            win.lifecycle
                .subscribe_notices(move |notice| {
                    if matches!(notice, HostNotice::MemoryPressure(_)) {
                        log.borrow_mut().push(counter.load(Ordering::SeqCst));
                    }
                })
                .expect("an open presentation")
        })
        .collect();

    rig.events
        .notice(SystemNotice::MemoryPressure(MemoryPressureLevel::Critical));

    assert_eq!(
        reclaimed.load(Ordering::SeqCst),
        1,
        "three windows are one host: its caches are reclaimed once"
    );
    assert_eq!(
        *seen_by_subscribers.borrow(),
        [1, 1, 1],
        "every window's subscriber heard it after the reclaim, never before"
    );
    drop(subscriptions);
}

#[test]
fn host_notices() {
    crate::table_test::run_table(
        "host_notices",
        &[
            (
                "a_query_is_answered_after_the_subscribers_ran_and_the_release_reaches_the_platform",
                a_query_is_answered_after_the_subscribers_ran_and_the_release_reaches_the_platform
                    as fn(),
            ),
            (
                "a_query_with_nobody_blocking_proceeds_and_releases_nothing",
                a_query_with_nobody_blocking_proceeds_and_releases_nothing as fn(),
            ),
            (
                "a_query_that_arrives_while_the_owner_is_busy_proceeds_and_closes_the_book",
                a_query_that_arrives_while_the_owner_is_busy_proceeds_and_closes_the_book as fn(),
            ),
            (
                "a_panicking_subscriber_still_gets_the_platform_answered_and_the_book_closed",
                a_panicking_subscriber_still_gets_the_platform_answered_and_the_book_closed
                    as fn(),
            ),
            (
                "the_shared_memory_is_reclaimed_once_however_many_windows_there_are",
                the_shared_memory_is_reclaimed_once_however_many_windows_there_are as fn(),
            ),
        ],
    );
}
