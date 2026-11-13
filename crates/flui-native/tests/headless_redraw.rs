//! A headless window's frame request runs the frame callback on the owner
//! thread, whichever thread asks: `PlatformWindow::request_redraw` is called
//! from async wakers, and the callback is owner code. So is the application's
//! owner-turn callback, which a worker's wake runs on the owner.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::thread::{self, ThreadId};

use flui_native::{HeadlessOwnerTurns, HeadlessPlatform, HostWindow, OwnerPlatform, Platform as _};
use flui_platform::WindowOptions;
use flui_testing::window_handler::RecordingWindowHandler;
use parking_lot::Mutex;

struct Session {
    owner: OwnerPlatform,
    turns: HeadlessOwnerTurns,
}

fn session() -> Session {
    let platform = HeadlessPlatform::new();
    let turns = platform.owner_turns();
    let saved = std::rc::Rc::new(std::cell::RefCell::new(None));
    let installed = std::rc::Rc::clone(&saved);
    Box::new(platform)
        .run(crate::app::on_ready(move |owner| {
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    let owner = saved.borrow_mut().take().expect("started owner");
    Session { owner, turns }
}

impl Session {
    /// A window, and the threads its handler's `frame_requested` ran on.
    fn open(&self) -> (Rc<dyn HostWindow>, Arc<Mutex<Vec<ThreadId>>>) {
        let handler = RecordingWindowHandler::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&log);
        handler.react_to_frame(move |_| sink.lock().push(thread::current().id()));
        let window = self
            .owner
            .open_window(WindowOptions::default(), handler)
            .expect("open")
            .try_ready()
            .expect("ready");
        (window, log)
    }
}

fn a_redraw_requested_off_the_owner_runs_in_the_owners_next_turn() {
    let session = session();
    let (window, frames) = session.open();

    let requester = window.redraw_requester();
    thread::spawn(move || {
        requester.request_redraw();
        requester.request_redraw();
    })
    .join()
    .expect("requesting thread");
    assert!(
        frames.lock().is_empty(),
        "the requesting thread runs no owner callback"
    );

    session.turns.drive();
    assert_eq!(
        *frames.lock(),
        [thread::current().id()],
        "one coalesced frame, on the owner"
    );
}

fn a_redraw_requested_on_the_owner_dispatches_at_once() {
    let session = session();
    let (window, frames) = session.open();

    window.request_redraw();
    assert_eq!(*frames.lock(), [thread::current().id()]);
}

/// The application's owner-turn callback is owner code too: it keeps
/// owner-local state (`Rc`, `Cell`) the turn a worker's wake posts reads,
/// and the platform's quit drops it on the owner thread.
fn an_owner_turn_callback_keeps_owner_local_state_until_the_quit() {
    struct Captured(Rc<Cell<bool>>);
    impl Drop for Captured {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    let session = session();
    let runs = Rc::new(Cell::new(0_usize));
    let dropped = Rc::new(Cell::new(false));
    let counted = Rc::clone(&runs);
    let captured = Captured(Rc::clone(&dropped));
    session
        .owner
        .on_wake(Box::new(move || {
            let _held = &captured;
            counted.set(counted.get() + 1);
        }))
        .expect("register the owner's turn callback");

    let proxy = session.owner.proxy();
    thread::spawn(move || proxy.wake().expect("a worker wakes the owner"))
        .join()
        .expect("waking thread");
    assert_eq!(runs.get(), 0, "the waking thread runs no owner callback");
    session.turns.drive();
    assert_eq!(runs.get(), 1, "the owner's turn runs the callback");
    assert!(!dropped.get(), "a turn keeps the callback for the next one");

    session.owner.quit();
    assert!(
        dropped.get(),
        "the quit drops the callback and its captures on the owner"
    );
}
#[test]
fn headless_redraw_contracts() {
    super::run_table(
        "headless_redraw",
        &[
            (
                "a_redraw_requested_off_the_owner_runs_in_the_owners_next_turn",
                a_redraw_requested_off_the_owner_runs_in_the_owners_next_turn,
            ),
            (
                "a_redraw_requested_on_the_owner_dispatches_at_once",
                a_redraw_requested_on_the_owner_dispatches_at_once,
            ),
            (
                "an_owner_turn_callback_keeps_owner_local_state_until_the_quit",
                an_owner_turn_callback_keeps_owner_local_state_until_the_quit,
            ),
        ],
    );
}
