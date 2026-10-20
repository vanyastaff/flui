//! A headless window's frame request runs the frame callback on the owner
//! thread, whichever thread asks: `PlatformWindow::request_redraw` is called
//! from async wakers, and the callback is owner code.

use std::sync::Arc;
use std::thread::{self, ThreadId};

use flui_platform::{
    HeadlessOwnerTurns, HeadlessPlatform, HostWindow, OwnerPlatform, Platform as _, WindowOptions,
};
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
        .run(Box::new(move |owner| {
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    let owner = saved.borrow_mut().take().expect("started owner");
    Session { owner, turns }
}

impl Session {
    fn open(&self) -> Arc<dyn HostWindow> {
        self.owner
            .open_window(WindowOptions::default())
            .expect("open")
            .try_ready()
            .expect("ready")
    }
}

/// The thread each frame callback ran on.
fn frame_threads(window: &Arc<dyn HostWindow>) -> Arc<Mutex<Vec<ThreadId>>> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    window.on_request_frame(Box::new(move || sink.lock().push(thread::current().id())));
    log
}

fn a_redraw_requested_off_the_owner_runs_in_the_owners_next_turn() {
    let session = session();
    let window = session.open();
    let frames = frame_threads(&window);

    let requester = Arc::clone(&window);
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
    let window = session.open();
    let frames = frame_threads(&window);

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
