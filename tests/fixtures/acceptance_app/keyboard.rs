//! The application driven from the keyboard alone: Tab walks every screen,
//! the notes list is one stop with its rows behind the arrow keys, a route
//! takes the focus when it opens and gives it back when it closes, and the
//! focus outlives a held key, a window switch and a retried load. Every read
//! of the focus checks that the accessibility tree reports the element the
//! keyboard holds.

use std::cell::Cell;
use std::future::ready;
use std::rc::Rc;

use flui::platform::keyboard::NamedKey;

use crate::app::{AcceptanceApp, Faults, Loader};
use crate::testing::AppRun;

fn app() -> AcceptanceApp {
    AcceptanceApp::new(Faults::NONE)
}

/// The labels Tab focuses, one press after another, `count` presses.
fn tabs(run: &mut AppRun, count: usize) -> Vec<String> {
    (0..count)
        .map(|_| {
            run.tab();
            run.focused().unwrap_or_default()
        })
        .collect()
}

/// The labels Shift+Tab focuses, `count` presses.
fn shift_tabs(run: &mut AppRun, count: usize) -> Vec<String> {
    (0..count)
        .map(|_| {
            run.shift_tab();
            run.focused().unwrap_or_default()
        })
        .collect()
}

/// `order` and then its first element again, as Tab wraps.
fn wrapped(order: &[&str]) -> Vec<String> {
    order
        .iter()
        .chain(order.first())
        .map(|label| (*label).to_owned())
        .collect()
}

/// `order` backwards from its last element, and the last again, as
/// Shift+Tab wraps.
fn wrapped_back(order: &[&str]) -> Vec<String> {
    order
        .iter()
        .rev()
        .chain(order.last())
        .map(|label| (*label).to_owned())
        .collect()
}

/// A window opens with nothing focused; the first Tab enters the current
/// route on its first control, the first Shift+Tab on its last.
#[test]
fn first_tab_enters_the_current_route() {
    let mut run = AppRun::start(app());
    assert_eq!(run.focused(), None, "a window opens with nothing focused");
    run.tab();
    assert_eq!(run.focused().as_deref(), Some("Settings"));

    let mut run = AppRun::start(app());
    run.shift_tab();
    assert_eq!(
        run.focused().as_deref(),
        Some("Note 1"),
        "the list is Home's last stop, entered on its first row"
    );
}

/// On every screen Tab visits each control once in reading order and wraps
/// within the route, the notes list being one stop; Shift+Tab walks the same
/// order back.
#[test]
fn tab_order_covers_every_screen() {
    let home = ["Settings", "Compact rows: off", "New note", "Note 1"];
    let note = ["Settings", "Back", "Title", "Save note"];
    let settings = ["Back"];

    let mut run = AppRun::start(app());
    assert_eq!(tabs(&mut run, home.len() + 1), wrapped(&home), "Home");
    run.unfocus();
    assert_eq!(
        shift_tabs(&mut run, home.len() + 1),
        wrapped_back(&home),
        "Home backwards"
    );

    run.tap("Note 1");
    run.unfocus();
    assert_eq!(tabs(&mut run, note.len() + 1), wrapped(&note), "Note");
    run.unfocus();
    assert_eq!(
        shift_tabs(&mut run, note.len() + 1),
        wrapped_back(&note),
        "Note backwards"
    );

    run.tap("Settings");
    run.unfocus();
    assert_eq!(
        tabs(&mut run, settings.len() + 1),
        wrapped(&settings),
        "Settings"
    );

    let failing: Loader = Rc::new(|_attempt| Box::pin(ready(Err("offline".to_owned()))));
    let mut run = AppRun::start(app().with_loader(failing));
    assert!(run.shows("Load failed: offline"));
    assert_eq!(tabs(&mut run, 2), wrapped(&["Retry"]), "the load error");
}

/// Inside the notes list the arrow keys, Page Down, End and Home move between
/// rows, building the ones not on screen; Tab leaves the list for the header
/// and Tab back returns to the row it left.
#[test]
fn the_notes_list_moves_by_row_page_and_end() {
    let mut run = AppRun::start(app());
    run.shift_tab();
    assert_eq!(run.focused().as_deref(), Some("Note 1"));

    run.key(NamedKey::ArrowDown);
    assert_eq!(run.focused().as_deref(), Some("Note 2"));
    run.key(NamedKey::ArrowUp);
    run.key(NamedKey::ArrowUp);
    assert_eq!(
        run.focused().as_deref(),
        Some("Note 1"),
        "the first row stays focused at the edge"
    );

    run.key(NamedKey::PageDown);
    let page = run.focused().expect("Page Down lands on a row");
    let page: u64 = page
        .strip_prefix("Note ")
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("Page Down lands on a note row, not {page:?}"));
    assert!(
        page > 2 && run.shows(&format!("Note {page}")),
        "Page Down lands on the last row wholly on screen, not Note {page}"
    );

    run.key(NamedKey::End);
    assert_eq!(run.focused().as_deref(), Some("Note 40"));
    assert!(run.shows("Note 40"), "End scrolls the last row on screen");
    run.key(NamedKey::Home);
    assert_eq!(run.focused().as_deref(), Some("Note 1"));
    assert!(run.shows("Note 1"));

    run.key(NamedKey::ArrowDown);
    run.tab();
    assert_eq!(
        run.focused().as_deref(),
        Some("Settings"),
        "Tab leaves the list and wraps to the header"
    );
    run.shift_tab();
    assert_eq!(
        run.focused().as_deref(),
        Some("Note 2"),
        "the list remembers the row it left"
    );
}

/// Opening a note from its row puts the focus in the Title field; Back, from
/// the field, returns it to the row of the same note, which a saved title
/// has rebuilt under a new label.
#[test]
fn a_note_route_takes_the_focus_and_gives_it_back() {
    let mut run = AppRun::start(app());
    run.shift_tab();
    run.key(NamedKey::ArrowDown);
    run.key(NamedKey::ArrowDown);
    assert_eq!(run.focused().as_deref(), Some("Note 3"));

    run.key(NamedKey::Enter);
    assert!(run.shows("Editing note 3"));
    assert_eq!(
        run.focused().as_deref(),
        Some("Title"),
        "the Title field's autofocus takes the new route's focus"
    );

    run.back_key();
    assert!(run.shows("Compact rows: off"), "Back from the field pops");
    assert_eq!(
        run.focused().as_deref(),
        Some("Note 3"),
        "the focus returns to the row that opened the note"
    );

    run.key(NamedKey::Enter);
    run.replace_title("Groceries");
    run.tap("Save note");
    assert!(run.shows("Saved note 3"));
    run.tap("Back");
    assert_eq!(
        run.focused().as_deref(),
        Some("Groceries"),
        "the row of the same note, rebuilt with its saved title"
    );
}

/// Enter held on Settings opens one Settings page: its repeats activate
/// nothing.
#[test]
fn held_enter_activates_once() {
    let mut run = AppRun::start(app());
    run.tab();
    assert_eq!(run.focused().as_deref(), Some("Settings"));

    run.hold(NamedKey::Enter, 5);
    assert!(run.shows("Compact rows are off"));
    run.tap("Back");
    assert!(
        run.shows("Compact rows: off"),
        "one Back returns Home: one page was pushed"
    );
}

/// A window that loses its OS focus and gains it back holds the focus where
/// it was, with the indicator as it was, and the keys work on from there.
#[test]
fn window_reactivation_restores_focus() {
    let mut run = AppRun::start(app());
    run.shift_tab();
    run.key(NamedKey::ArrowDown);
    assert_eq!(run.focused().as_deref(), Some("Note 2"));
    let indicator = run.shows_focus_indicator();
    assert!(indicator, "keyboard focus shows the indicator");

    run.set_window_focused(false);
    run.set_window_focused(true);
    assert_eq!(run.focused().as_deref(), Some("Note 2"));
    assert_eq!(run.shows_focus_indicator(), indicator);

    run.key(NamedKey::ArrowDown);
    assert_eq!(run.focused().as_deref(), Some("Note 3"));
}

/// Retry, activated from the keyboard, disappears with the error it
/// answered; the focus moves on to the loaded Home instead of to nothing.
#[test]
fn retry_from_the_keyboard_hands_the_focus_on() {
    let attempts = Rc::new(Cell::new(0_u32));
    let seen = Rc::clone(&attempts);
    let loader: Loader = Rc::new(move |attempt| {
        seen.set(seen.get() + 1);
        Box::pin(ready(if attempt == 0 {
            Err("offline".to_owned())
        } else {
            Ok(())
        }))
    });
    let mut run = AppRun::start(app().with_loader(loader));
    run.tab();
    assert_eq!(run.focused().as_deref(), Some("Retry"));

    run.key(NamedKey::Enter);
    assert_eq!(attempts.get(), 2, "Enter retried once");
    assert!(run.shows("Note 1"));
    assert!(
        run.focused().is_some(),
        "the focus moves on with the error gone"
    );
}
