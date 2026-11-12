//! A window another thread asks for through `PlatformProxy::open_window`
//! opens in the owner's next turn, on the owner, and its requester hears the
//! window's identity or why it did not open, on the headless host, whose owner
//! turns a test drives by hand.

use std::rc::Rc;
use std::sync::Arc;
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use flui_native::{
    HeadlessOpenWindows, HeadlessOwnerTurns, HeadlessPlatform, OpenWindowError, OwnerPlatform,
    PendingWindow, Platform as _, ProxySendError, WindowRequest,
};
use flui_platform::{WindowHandler, WindowOptions};
use flui_testing::window_handler::RecordingWindowHandler;
use parking_lot::Mutex;

#[test]
fn proxy_open_window_contract() {
    crate::run_table(
        "proxy_open_window_contract",
        &[
            (
                "a_window_asked_for_on_another_thread_opens_in_the_owners_next_turn",
                a_window_asked_for_on_another_thread_opens_in_the_owners_next_turn,
            ),
            (
                "requests_open_in_order_and_a_full_queue_hands_the_request_back",
                requests_open_in_order_and_a_full_queue_hands_the_request_back,
            ),
            (
                "a_factory_that_panics_fails_its_open_and_the_next_one_opens",
                a_factory_that_panics_fails_its_open_and_the_next_one_opens,
            ),
            (
                "a_request_its_requester_let_go_of_opens_nothing",
                a_request_its_requester_let_go_of_opens_nothing,
            ),
            (
                "a_quit_refuses_the_opens_still_queued",
                a_quit_refuses_the_opens_still_queued,
            ),
        ],
    );
}

struct Session {
    owner: OwnerPlatform,
    turns: HeadlessOwnerTurns,
    windows: HeadlessOpenWindows,
}

fn session() -> Session {
    let platform = HeadlessPlatform::new();
    let turns = platform.owner_turns();
    let windows = platform.open_windows();
    let saved = Rc::new(std::cell::RefCell::new(None));
    let installed = Rc::clone(&saved);
    Box::new(platform)
        .run(crate::app::on_ready(move |owner| {
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    let owner = saved.borrow_mut().take().expect("started owner");
    Session {
        owner,
        turns,
        windows,
    }
}

/// The threads each request's factory ran on, by the request's number.
type Built = Arc<Mutex<Vec<(usize, ThreadId)>>>;

/// A request whose factory records `label` and the thread it runs on.
fn request(label: usize, built: &Built) -> WindowRequest {
    let built = Arc::clone(built);
    WindowRequest::new(WindowOptions::default(), move || {
        built.lock().push((label, thread::current().id()));
        RecordingWindowHandler::new() as Rc<dyn WindowHandler>
    })
}

fn a_window_asked_for_on_another_thread_opens_in_the_owners_next_turn() {
    let session = session();
    let built = Built::default();
    let proxy = session.owner.proxy();
    let asked = request(0, &built);
    let worker = thread::spawn(move || {
        let pending = proxy.open_window(asked).expect("the owner takes the open");
        pending.wait()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !worker.is_finished() {
        assert!(
            Instant::now() < deadline,
            "the worker never heard its window"
        );
        session.turns.drive();
        thread::sleep(Duration::from_millis(1));
    }
    let id = worker
        .join()
        .expect("the worker")
        .expect("the window opened");
    assert_eq!(
        *built.lock(),
        [(0, thread::current().id())],
        "the factory ran once, on the owner"
    );
    assert!(
        session.windows.get(id).is_some(),
        "the window the worker heard of is open"
    );
}

fn requests_open_in_order_and_a_full_queue_hands_the_request_back() {
    let session = session();
    let built = Built::default();
    let proxy = session.owner.proxy();
    let mut pending: Vec<PendingWindow> = (0..64)
        .map(|label: usize| {
            proxy
                .open_window(request(label, &built))
                .expect("the queue has room")
        })
        .collect();

    let refused = proxy
        .open_window(WindowRequest::new(
            WindowOptions::default().title("one too many"),
            || RecordingWindowHandler::new() as Rc<dyn WindowHandler>,
        ))
        .expect_err("a full queue refuses");
    let ProxySendError::Full { capacity, rejected } = refused else {
        panic!("a full queue answers Full, not {refused:?}");
    };
    assert_eq!(capacity, 64);
    assert_eq!(
        rejected.options.title, "one too many",
        "the request comes back whole"
    );

    session.turns.drive();
    let order: Vec<usize> = built.lock().iter().map(|(label, _)| *label).collect();
    assert_eq!(
        order,
        (0..64).collect::<Vec<usize>>(),
        "the factories ran in the order asked"
    );
    let opened: Vec<_> = pending
        .iter_mut()
        .map(|pending| {
            pending
                .try_take()
                .expect("answered in that turn")
                .expect("opened")
                .id()
        })
        .collect();
    let open: Vec<_> = session
        .windows
        .all()
        .iter()
        .map(flui_platform::PlatformWindow::id)
        .collect();
    assert_eq!(opened, open, "the windows opened in the order asked");

    let _again = proxy
        .open_window(request(64, &built))
        .expect("the drained queue has room again");
}

fn a_factory_that_panics_fails_its_open_and_the_next_one_opens() {
    let session = session();
    let built = Built::default();
    let proxy = session.owner.proxy();
    let mut panicking = proxy
        .open_window(WindowRequest::new(WindowOptions::default(), || {
            panic!("the factory gave up")
        }))
        .expect("the owner takes the open");
    let mut next = proxy
        .open_window(request(1, &built))
        .expect("the owner takes the open");

    session.turns.drive();
    match panicking.try_take().expect("answered") {
        Err(OpenWindowError::Backend { message }) => assert!(
            message.contains("the factory gave up"),
            "the failure says what the factory said: {message}"
        ),
        other => panic!("a panicking factory fails its open, not {other:?}"),
    }
    let id = next.try_take().expect("answered").expect("opened").id();
    assert_eq!(session.windows.all().len(), 1, "only the next one opened");
    assert!(session.windows.get(id).is_some());
}

fn a_request_its_requester_let_go_of_opens_nothing() {
    let session = session();
    let built = Built::default();
    let pending = session
        .owner
        .proxy()
        .open_window(request(0, &built))
        .expect("the owner takes the open");
    drop(pending);

    session.turns.drive();
    assert!(built.lock().is_empty(), "the factory never ran");
    assert!(session.windows.all().is_empty(), "no window opened");
}

fn a_quit_refuses_the_opens_still_queued() {
    let session = session();
    let built = Built::default();
    let proxy = session.owner.proxy();
    let mut pending = proxy
        .open_window(request(0, &built))
        .expect("the owner takes the open");
    proxy.request_quit().expect("the quit is taken");

    session.turns.drive();
    assert!(
        matches!(
            pending.try_take(),
            Some(Err(OpenWindowError::OwnerGone { .. }))
        ),
        "the queued open hears that the owner is gone"
    );
    assert!(built.lock().is_empty(), "the factory never ran");
    assert!(session.windows.all().is_empty(), "no window opened");
    assert!(
        matches!(
            proxy.open_window(request(1, &built)),
            Err(ProxySendError::OwnerGone { .. })
        ),
        "a later open is refused"
    );
}
