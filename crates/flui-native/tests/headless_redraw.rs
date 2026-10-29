//! A headless window's frame request runs the frame callback on the owner
//! thread, whichever thread asks: `PlatformWindow::request_redraw` is called
//! from async wakers, and the callback is owner code.

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
        ],
    );
}
