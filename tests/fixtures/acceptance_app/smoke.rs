//! The application's own rows: it starts headless, walks its screens, edits,
//! adds and saves notes, and recovers from a failed load.

use std::cell::RefCell;
use std::future::ready;
use std::rc::Rc;

use crate::app::{AcceptanceApp, Faults, Loader};
use crate::testing::AppRun;

fn app() -> AcceptanceApp {
    AcceptanceApp::new(Faults::NONE)
}

#[test]
fn acceptance_app_walks_home_note_settings_and_back() {
    let mut run = AppRun::start(app());
    assert!(run.shows("Note 1"), "Home lists the notes");
    assert!(!run.shows("Back"), "Home is the bottom of the stack");

    run.tap("Note 1");
    assert!(run.shows("Editing note 1"));
    assert_eq!(
        run.title(),
        "Note 1",
        "the draft starts from the saved title"
    );
    assert!(!run.shows("Compact rows: off"), "Home is covered");

    run.tap("Settings");
    assert!(run.shows("Compact rows are off"));
    assert!(!run.shows("Editing note 1"), "the editor is covered");

    run.tap("Back");
    assert!(run.shows("Editing note 1"), "Back returns to the editor");

    run.tap("Back");
    assert!(run.shows("Compact rows: off"), "and then to Home");
    assert!(!run.shows("Back"));

    run.tap("Compact rows: off");
    assert!(run.shows("Compact rows: on"));
    run.tap("Settings");
    assert!(
        run.shows("Compact rows are on"),
        "Settings reads Home's toggle"
    );
}

#[test]
fn acceptance_app_saves_an_edited_title() {
    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");
    assert_eq!(run.title(), "Groceries");

    run.tap("Save note");
    assert!(run.shows("Saved note 2"));

    run.tap("Back");
    assert!(run.shows("Groceries"), "Home lists the saved title");
    assert!(!run.shows("Note 2"));
    assert!(run.shows("Note 3"), "the other notes are unchanged");
}

#[test]
fn acceptance_app_keeps_the_draft_after_back() {
    let mut run = AppRun::start(app());
    run.tap("Note 3");
    run.replace_title("Unsaved");
    run.tap("Back");
    assert!(run.shows("Note 3"), "Back saves nothing");
    assert!(!run.shows("Unsaved"));

    run.tap("Note 3");
    assert_eq!(run.title(), "Unsaved", "the editor reopens on the draft");
}

#[test]
fn acceptance_app_adds_notes_and_refuses_an_empty_title() {
    let mut run = AppRun::start(app());
    run.tap("New note");
    assert!(run.shows("Editing note 41"), "a new note takes the next id");
    assert_eq!(run.title(), "Note 41");

    run.replace_title("");
    run.tap("Save note");
    assert!(run.shows("Fix the title"), "the save is refused");
    // The error is checked as laid-out text: the decoration that shows it
    // is not a tap target.
    assert!(
        !run.laid().find_all_text("Enter a title").is_empty(),
        "the field explains the refusal"
    );

    run.tap("Back");
    run.tap("New note");
    assert!(
        run.shows("Editing note 42"),
        "an id is never handed out twice"
    );
}

/// A loader that records each attempt and fails the attempt `failing`.
fn recording_loader(failing: u32) -> (Loader, Rc<RefCell<Vec<u32>>>) {
    let attempts = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&attempts);
    let loader: Loader = Rc::new(move |attempt| {
        seen.borrow_mut().push(attempt);
        Box::pin(ready(if attempt == failing {
            Err("offline".to_owned())
        } else {
            Ok(())
        }))
    });
    (loader, attempts)
}

#[test]
fn acceptance_app_retries_a_failed_load() {
    let (loader, attempts) = recording_loader(0);
    let mut run = AppRun::start(app().with_loader(loader));
    assert!(run.shows("Load failed: offline"));
    assert!(!run.shows("Note 1"));

    run.rebuild();
    assert_eq!(*attempts.borrow(), [0], "a rebuild keeps the load it has");
    assert!(run.shows("Load failed: offline"));

    run.tap("Retry");
    assert!(run.shows("Note 1"), "the retried load opens Home");
    assert!(!run.shows("Load failed: offline"));
    assert_eq!(*attempts.borrow(), [0, 1], "Retry starts one new load");

    run.rebuild();
    assert_eq!(*attempts.borrow(), [0, 1]);
}

#[test]
fn retry_counter_overflow_still_starts_a_load() {
    let (loader, attempts) = recording_loader(u32::MAX);
    let mut run = AppRun::start(app().with_loader(loader).first_attempt(u32::MAX));
    assert!(run.shows("Load failed: offline"));

    run.tap("Retry");
    assert_eq!(*attempts.borrow(), [u32::MAX, 0], "Retry wraps the attempt");
    assert!(run.shows("Note 1"));
}
