//! Persistence rows: what a restart over the same storage finds.

use flui::view::persist::Document;

use crate::app::{AcceptanceApp, Faults, Saved};
use crate::testing::AppRun;

fn app() -> AcceptanceApp {
    AcceptanceApp::new(Faults::NONE)
}

#[test]
fn saved_title_survives_restart_and_others_stay() {
    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");
    run.tap("Save note");
    assert!(run.shows("Saved note 2"));

    let mut run = run.restart();
    assert!(run.shows("Groceries"), "the saved title is read back");
    assert!(!run.shows("Note 2"));
    assert!(run.shows("Note 1"), "the other notes are unchanged");
    assert!(run.shows("Note 3"));

    run.tap("Groceries");
    assert_eq!(
        run.title(),
        "Groceries",
        "the editor opens on the stored title"
    );
}

#[test]
fn compact_rows_survive_restart() {
    let mut run = AppRun::start(app());
    run.tap("Compact rows: off");
    assert!(run.shows("Compact rows: on"));

    let mut run = run.restart();
    assert!(run.shows("Compact rows: on"), "the flag is read back");
    run.tap("Settings");
    assert!(run.shows("Compact rows are on"));
}

#[test]
fn first_launch_starts_from_initial_state_without_an_error() {
    let mut run = AppRun::start(app());
    assert!(run.shows("Note 1"), "Home lists the initial notes");
    assert!(run.shows("Compact rows: off"));
    assert!(
        !run.shows("Retry"),
        "an empty storage is a first launch, not a failure"
    );
    assert_eq!(
        run.storage().contents(&Saved::NAME),
        None,
        "reading the initial state writes nothing"
    );

    run.tap("Compact rows: off");
    let stored = run
        .storage()
        .contents(&Saved::NAME)
        .expect("the first edit writes the document");
    let stored = String::from_utf8_lossy(&stored);
    assert!(
        stored.contains(r#""title":"Note 40""#) && stored.contains(r#""compact":true"#),
        "the first write is the initial state with the edit: {stored}"
    );
}

#[test]
fn a_note_id_is_never_reused_across_a_restart() {
    let mut run = AppRun::start(app());
    run.tap("New note");
    assert!(run.shows("Editing note 41"));

    let mut run = run.restart();
    run.tap("New note");
    assert!(
        run.shows("Editing note 42"),
        "an id is never handed out twice, across runs too"
    );
}
