//! Teardown rows: what closing the application releases, and what a panic
//! in a screen's `build` leaves working.

use std::rc::Rc;

use flui::platform::StorageError;
use flui::testing::DropLedger;
use flui::view::ErrorView;

use crate::app::{AcceptanceApp, Fault, Faults, Kept, Planter};
use crate::testing::AppRun;

/// One drop ledger per kind of value the application keeps.
#[derive(Clone, Default)]
struct Ledgers {
    states: DropLedger,
    pages: DropLedger,
    listeners: DropLedger,
}

impl Ledgers {
    /// A planter that puts a sentinel of the kind's ledger in each value.
    fn planter(&self) -> Planter {
        let ledgers = self.clone();
        Rc::new(move |kind| {
            let ledger = match kind {
                Kept::State => &ledgers.states,
                Kept::Page => &ledgers.pages,
                Kept::Listener => &ledgers.listeners,
            };
            Box::new(ledger.plant())
        })
    }

    /// Each kind with its live sentinels, by name.
    fn live(&self) -> [(&'static str, usize); 3] {
        [
            ("states", self.states.live()),
            ("pages", self.pages.live()),
            ("listeners", self.listeners.live()),
        ]
    }
}

/// R1: after a walk through Home, a note and Settings, closing the application
/// drops every state, every page the router kept and the draft's listener.
#[test]
fn acceptance_app_close_releases_every_owner_after_navigation() {
    let ledgers = Ledgers::default();
    let mut run = AppRun::start(AcceptanceApp::new(Faults::NONE).with_planter(ledgers.planter()));
    run.tap("Note 1");
    run.tap("Settings");
    assert!(run.shows("Compact rows are off"), "Settings is open");
    for (kind, live) in ledgers.live() {
        assert!(live > 0, "the walk kept no {kind}");
    }

    run.close();
    for (kind, live) in ledgers.live() {
        assert_eq!(live, 0, "{live} {kind} outlived the close");
    }
}

/// R8: a panic in the Note editor's `build` replaces the editor with an error
/// view; the screen's header still takes input, its Back leaves the error
/// with its page, and the other screens work.
#[test]
fn a_panicking_note_build_shows_error_view_and_other_screens_work() {
    let mut run = AppRun::start(AcceptanceApp::new(Faults::from(Fault::Build)));
    run.tap("Note 1");
    assert!(
        run.laid().count_elements_by_view_type::<ErrorView>() > 0,
        "the failed editor shows an error view"
    );
    assert!(!run.shows("Editing note 1"));

    run.tap("Back");
    assert!(run.shows("Note 2"), "Home is back");
    assert_eq!(
        run.laid().count_elements_by_view_type::<ErrorView>(),
        0,
        "the error view left with its page"
    );
    run.tap("Settings");
    assert!(run.shows("Compact rows are off"), "Settings opens");
    run.tap("Back");
    run.tap("Compact rows: off");
    assert!(run.shows("Compact rows: on"), "Home takes input");
}

/// R23: a close held by a failed save drops nothing, kept open or asked
/// again; the user's Close without saving then closes the window, and the
/// exit drops every value the application kept.
#[test]
fn a_held_close_drops_nothing_until_the_user_decides() {
    let ledgers = Ledgers::default();
    let mut run = AppRun::start(AcceptanceApp::new(Faults::NONE).with_planter(ledgers.planter()));
    run.tap("Note 2");
    run.replace_title("Groceries");
    run.storage().fail_next(StorageError::Full);
    run.tap("Save note");
    let before = ledgers.live();
    for (kind, live) in before {
        assert!(live > 0, "the note's screen keeps no {kind}");
    }

    run.request_close();
    assert!(!run.is_closed(), "the failed save holds the close");
    assert_eq!(ledgers.live(), before, "a held close drops nothing");
    run.tap("Keep open");
    assert_eq!(ledgers.live(), before, "nor does keeping the window open");
    run.request_close();
    assert_eq!(ledgers.live(), before, "nor a close asked again");

    run.tap("Close without saving");
    assert!(run.is_closed(), "the user's decision closes the window");
    drop(run);
    for (kind, live) in ledgers.live() {
        assert_eq!(live, 0, "{live} {kind} outlived the exit");
    }
}
