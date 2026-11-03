//! The Title field under a push-model platform input method, as winit, the
//! web and Android's `InputConnection` deliver one: the composition shows
//! where the caret was, a commit replaces it with the conversion and changes
//! the form's value once, a cancel leaves the committed text alone, and a tap
//! elsewhere or a route change commits what the user sees.

use std::cell::Cell;
use std::rc::Rc;

use flui::platform::ImeEvent;

use crate::app::{AcceptanceApp, Faults};
use crate::testing::AppRun;

/// Note 2 open, its Title replaced by `title` and focused, with the form's
/// changes counted from here on.
fn editing(title: &str) -> (AppRun, Rc<Cell<u32>>) {
    let edits = Rc::new(Cell::new(0));
    let mut run = AppRun::start(AcceptanceApp::new(Faults::NONE).with_edit_counter(Rc::clone(&edits)));
    run.tap("Note 2");
    run.replace_title(title);
    edits.set(0);
    run.ime(ImeEvent::Enabled);
    (run, edits)
}

fn preedit(text: &str, cursor: Option<(usize, usize)>) -> ImeEvent {
    ImeEvent::Preedit {
        text: text.to_owned(),
        cursor,
    }
}

/// The composition is inserted at the caret and marked as composing, which
/// the field underlines; a composition without a cursor hides the caret.
#[test]
fn ime_preedit_is_underlined_at_the_caret_and_hides_it_on_request() {
    let (mut run, edits) = editing("Trip to ");
    let kana = "とうきょう";
    run.ime(preedit(kana, Some((kana.len(), kana.len()))));
    assert_eq!(run.title(), "Trip to とうきょう");
    assert_eq!(
        run.title_composing(),
        Some(8..23),
        "the composition is marked where the caret was"
    );
    assert_eq!(run.title_caret(), 23, "the caret is at the composition's cursor");
    assert!(run.title_shows_caret());

    run.ime(preedit(kana, None));
    assert!(!run.title_shows_caret(), "the input method hid the caret");
    assert_eq!(run.title_composing(), Some(8..23));
    assert_eq!(edits.get(), 0, "a composition is no change of the value");
}

/// `toukyou` composes to `東京` and commits: the field holds the conversion
/// with the caret after it and no composition, the form's value changed
/// once, and Save stores it.
#[test]
fn ime_commit_inserts_the_conversion_and_notifies_once() {
    let (mut run, edits) = editing("");
    for step in ["と", "とう", "とうきょ", "とうきょう"] {
        run.ime(preedit(step, Some((step.len(), step.len()))));
    }
    run.ime(preedit("東京", Some((6, 6))));
    assert_eq!(edits.get(), 0, "nothing is committed while composing");
    run.ime(ImeEvent::Commit("東京".to_owned()));
    run.ime(ImeEvent::Disabled);
    assert_eq!(run.title(), "東京");
    assert_eq!(run.title_composing(), None, "the composition is gone");
    assert_eq!(run.title_caret(), 6, "the caret is after the conversion");
    assert_eq!(edits.get(), 1, "the commit changed the value once");

    run.tap("Save note");
    run.tap("Back");
    assert!(run.shows("東京"), "the list shows the saved conversion");
}

/// An empty composition cancels: the composed text goes, the committed text
/// and the caret stay where the composition began, and the value never
/// changed.
#[test]
fn ime_cancel_removes_the_preedit_and_keeps_the_committed_text() {
    let (mut run, edits) = editing("Trip to ");
    run.ime(preedit("とうきょう", Some((15, 15))));
    assert_eq!(run.title(), "Trip to とうきょう", "the composition shows");
    run.ime(preedit("", None));
    run.ime(ImeEvent::Disabled);
    assert_eq!(run.title(), "Trip to ");
    assert_eq!(run.title_composing(), None);
    assert_eq!(run.title_caret(), 8, "the caret stays where the composition began");
    assert_eq!(edits.get(), 0, "a cancelled composition changes nothing");
}

/// A tap on Save while composing commits what the user sees before the
/// button's handler runs, so the saved title holds the composition.
#[test]
fn pointer_down_mid_composition_commits_before_the_save_handler() {
    let (mut run, edits) = editing("Trip to ");
    run.ime(preedit("東京", Some((6, 6))));
    run.tap("Save note");
    assert!(run.shows("Saved note 2"), "the save ran");
    assert_eq!(run.title(), "Trip to 東京");
    assert_eq!(run.title_composing(), None, "the tap ended the composition");
    assert_eq!(edits.get(), 1, "the commit changed the value once");
    run.tap("Back");
    assert!(run.shows("Trip to 東京"), "the saved title holds the composition");
}

/// The Back chord pops the Note screen while its field composes: the field
/// is unmounted without a tap or a blur of its own, and the composition is
/// committed into the draft the Note screen reopens with.
#[test]
fn route_change_mid_composition_commits_the_preedit_into_the_draft() {
    let (mut run, _edits) = editing("Trip to ");
    run.ime(preedit("東京", Some((6, 6))));
    run.back_key();
    assert!(!run.shows("Editing note 2"), "the Note screen is gone");
    run.tap("Note 2");
    assert_eq!(run.title(), "Trip to 東京", "the draft holds the composition");
    assert_eq!(run.title_composing(), None, "committed, not composing");
}
