//! The application's own rows: it starts headless, walks its screens, saves
//! an edit and recovers from a failed load.

use std::cell::RefCell;
use std::future::ready;
use std::rc::Rc;

use crate::app::{AcceptanceApp, Faults};
use crate::testing::AppRun;

#[test]
fn acceptance_app_walks_home_note_settings_and_back() {
    let mut run = AppRun::start(AcceptanceApp::new(Faults::default()));
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
    let mut run = AppRun::start(AcceptanceApp::new(Faults::default()));
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
fn acceptance_app_retries_a_failed_load() {
    let attempts = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&attempts);
    let app = AcceptanceApp::new(Faults::default()).with_loader(Rc::new(move |attempt| {
        seen.borrow_mut().push(attempt);
        Box::pin(ready(if attempt == 0 {
            Err("offline".to_owned())
        } else {
            Ok(())
        }))
    }));
    let mut run = AppRun::start(app);
    assert!(run.shows("Load failed: offline"));
    assert!(!run.shows("Note 1"));

    run.tap("Retry");
    assert!(run.shows("Note 1"), "the retried load opens Home");
    assert!(!run.shows("Load failed: offline"));
    assert_eq!(*attempts.borrow(), [0, 1], "one load per attempt");
}
