//! `zwp_text_input_v3`'s rules over a field's store, on the reference store.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::Bounds;
use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::text_store::{
    CommitGate, Composition, CompositionEnd, Selection, TextInputPurpose, TextStore, TextStoreHost,
};
use flui_testing::text_store_kit::InMemoryTextStore;

use super::{
    ChangeCause, ContentType, MAX_SURROUNDING_BYTES, Surrounding, TextInputClient, TextInputWire,
};

/// One request the client sent on the wire.
#[derive(Clone, Debug, PartialEq)]
enum Sent {
    Enable,
    Disable,
    Surrounding(Surrounding),
    Cause(ChangeCause),
    Content(ContentType),
    Caret(Bounds<f64>),
    Commit,
}

/// A seat's text input as the client reaches it: what it was sent.
#[derive(Default)]
struct Wire {
    sent: RefCell<Vec<Sent>>,
    commits: Cell<u32>,
}

impl Wire {
    fn take(&self) -> Vec<Sent> {
        std::mem::take(&mut *self.sent.borrow_mut())
    }

    /// The surrounding texts sent since the last take, in order.
    fn surroundings(&self) -> Vec<Surrounding> {
        self.take()
            .into_iter()
            .filter_map(|sent| match sent {
                Sent::Surrounding(surrounding) => Some(surrounding),
                _ => None,
            })
            .collect()
    }
}

impl TextInputWire for Wire {
    fn enable(&self) {
        self.sent.borrow_mut().push(Sent::Enable);
    }

    fn disable(&self) {
        self.sent.borrow_mut().push(Sent::Disable);
    }

    fn set_surrounding_text(&self, text: &str, cursor: usize, anchor: usize) {
        self.sent.borrow_mut().push(Sent::Surrounding(Surrounding {
            text: text.to_owned(),
            cursor,
            anchor,
        }));
    }

    fn set_text_change_cause(&self, cause: ChangeCause) {
        self.sent.borrow_mut().push(Sent::Cause(cause));
    }

    fn set_content_type(&self, content: ContentType) {
        self.sent.borrow_mut().push(Sent::Content(content));
    }

    fn set_cursor_rectangle(&self, rect: Bounds<f64>) {
        self.sent.borrow_mut().push(Sent::Caret(rect));
    }

    fn commit(&self) {
        self.commits.set(self.commits.get() + 1);
        self.sent.borrow_mut().push(Sent::Commit);
    }

    fn commits(&self) -> u32 {
        self.commits.get()
    }
}

fn span(start: usize, end: usize) -> Utf16Range {
    Utf16Range::try_new(Utf16Offset::new(start), Utf16Offset::new(end)).expect("an ordered span")
}

fn caret(at: usize) -> Selection {
    Selection::collapsed(Utf16Offset::new(at))
}

fn around(text: &str, cursor: usize, anchor: usize) -> Surrounding {
    Surrounding {
        text: text.to_owned(),
        cursor,
        anchor,
    }
}

fn as_store(store: &Rc<InMemoryTextStore>) -> Rc<dyn TextStore> {
    Rc::clone(store) as Rc<dyn TextStore>
}

/// A client serving `store`, with a seat's text input in its window.
fn serving(store: &Rc<InMemoryTextStore>) -> (Rc<TextInputClient>, Rc<Wire>) {
    let client = TextInputClient::new();
    let wire = Rc::new(Wire::default());
    client.focus_store(Some(as_store(store)));
    client.entered(Rc::clone(&wire) as Rc<dyn TextInputWire>);
    (client, wire)
}

#[test]
fn a_field_with_a_seat_in_its_window_is_enabled_with_its_state() {
    let store = InMemoryTextStore::new("ab");
    let (_client, wire) = serving(&store);
    let sent = wire.take();
    assert_eq!(sent.first(), Some(&Sent::Enable));
    assert!(sent.contains(&Sent::Content(ContentType {
        hint: 0,
        purpose: ContentType::NORMAL,
    })));
    assert!(sent.contains(&Sent::Surrounding(around("ab", 2, 2))));
    assert!(sent.iter().any(|sent| matches!(sent, Sent::Caret(_))));
    assert_eq!(
        sent.last(),
        Some(&Sent::Commit),
        "the state is made current"
    );
}

#[test]
fn a_preedit_composes_and_a_commit_replaces_it() {
    let store = InMemoryTextStore::new("ab");
    let (client, wire) = serving(&store);
    let _ = wire.take();

    // "に" is three bytes: a cursor at its end.
    client.preedit(&*wire, Some("に".into()), 3, 3);
    client.done(&*wire, wire.commits());
    assert_eq!(store.text(), "abに");
    assert_eq!(
        store.composition(),
        Some(Composition {
            range: span(2, 3),
            hides_caret: false,
        })
    );
    assert_eq!(store.selection(), caret(3));
    assert_eq!(
        wire.surroundings(),
        [around("ab", 2, 2)],
        "the preedit is left out of the surrounding text"
    );

    client.preedit(&*wire, Some("にほ".into()), 3, 3);
    client.done(&*wire, wire.commits());
    assert_eq!(store.text(), "abにほ", "the preedit is replaced");
    assert_eq!(store.selection(), caret(3), "the caret sits after に");

    client.commit_string(&*wire, Some("日本".into()));
    client.done(&*wire, wire.commits());
    assert_eq!(store.text(), "ab日本");
    assert_eq!(store.composition(), None);
    assert_eq!(store.selection(), caret(4));
    let _ = wire.take();
}

#[test]
fn a_done_that_names_no_preedit_clears_it() {
    let store = InMemoryTextStore::new("");
    let (client, wire) = serving(&store);
    client.preedit(&*wire, Some("k".into()), -1, -1);
    client.done(&*wire, wire.commits());
    assert_eq!(
        store
            .composition()
            .map(|composition| composition.hides_caret),
        Some(true),
        "a negative cursor hides the caret"
    );
    client.done(&*wire, wire.commits());
    assert_eq!(store.text(), "");
    assert_eq!(store.composition(), None);
}

#[test]
fn a_deletion_counts_bytes_around_the_selection() {
    struct Case {
        name: &'static str,
        text: &'static str,
        selection: Selection,
        before: u32,
        after: u32,
        expected: &'static str,
        caret: usize,
    }
    let cases = [
        Case {
            name: "ascii before the caret",
            text: "hello",
            selection: caret(5),
            before: 3,
            after: 0,
            expected: "he",
            caret: 2,
        },
        Case {
            name: "a count ending inside a character stops short of it",
            text: "hé",
            selection: caret(2),
            before: 1,
            after: 0,
            expected: "hé",
            caret: 2,
        },
        Case {
            name: "a two-byte character takes two bytes",
            text: "hé",
            selection: caret(2),
            before: 2,
            after: 0,
            expected: "h",
            caret: 1,
        },
        Case {
            name: "both sides of a caret",
            text: "a😀b😀c",
            selection: caret(3),
            before: 4,
            after: 5,
            expected: "ac",
            caret: 1,
        },
        Case {
            name: "around a selection, which stays selected",
            text: "xabcy",
            selection: Selection {
                anchor: Utf16Offset::new(1),
                active: Utf16Offset::new(4),
            },
            before: 1,
            after: 1,
            expected: "abc",
            caret: 0,
        },
        Case {
            name: "counts past the document stop at its ends",
            text: "ab",
            selection: caret(1),
            before: 99,
            after: 99,
            expected: "",
            caret: 0,
        },
    ];
    for case in cases {
        let store = InMemoryTextStore::new(case.text);
        store.app_select(case.selection);
        let (client, wire) = serving(&store);
        client.delete_surrounding(&*wire, case.before, case.after);
        client.done(&*wire, wire.commits());
        assert_eq!(store.text(), case.expected, "{}", case.name);
        assert_eq!(
            store.selection().range().start(),
            Utf16Offset::new(case.caret),
            "{}",
            case.name
        );
    }
}

#[test]
fn a_stale_done_is_applied_and_sends_no_state() {
    let store = InMemoryTextStore::new("");
    let (client, wire) = serving(&store);
    let stale = wire.commits();
    // The application types: the client sends a newer state the compositor
    // had not seen when it answered.
    store.app_replace(span(0, 0), "a");
    assert!(wire.commits() > stale, "the application edit was told");
    let _ = wire.take();

    client.commit_string(&*wire, Some("b".into()));
    client.done(&*wire, stale);
    assert_eq!(store.text(), "ab", "the commit is not dropped");
    assert_eq!(wire.take(), [], "no state answers a stale done");

    client.done(&*wire, wire.commits());
    assert_eq!(wire.surroundings(), [around("ab", 2, 2)]);
}

#[test]
fn an_application_edit_is_told_as_another_cause() {
    let store = InMemoryTextStore::new("x");
    let (_client, wire) = serving(&store);
    let _ = wire.take();
    store.app_replace(span(1, 1), "y");
    let sent = wire.take();
    assert!(sent.contains(&Sent::Surrounding(around("xy", 2, 2))));
    assert!(sent.contains(&Sent::Cause(ChangeCause::Other)));
    assert_eq!(sent.last(), Some(&Sent::Commit));
}

#[test]
fn leaving_the_window_resets_the_preedit() {
    let store = InMemoryTextStore::new("a");
    let (client, wire) = serving(&store);
    client.preedit(&*wire, Some("b".into()), 1, 1);
    client.done(&*wire, wire.commits());
    assert_eq!(store.text(), "ab");

    client.left(&*wire);
    assert_eq!(store.text(), "a", "the preedit's text leaves the field");
    assert_eq!(store.composition(), None);

    // A late event of the left window changes nothing.
    client.commit_string(&*wire, Some("z".into()));
    client.done(&*wire, wire.commits());
    assert_eq!(store.text(), "a");
}

#[test]
fn focus_moved_mid_composition_commits_it_where_it_was() {
    let first = InMemoryTextStore::new("");
    let second = InMemoryTextStore::new("");
    let (client, wire) = serving(&first);
    client.preedit(&*wire, Some("に".into()), 3, 3);
    client.done(&*wire, wire.commits());
    let _ = wire.take();

    client.focus_store(Some(as_store(&second)));
    assert_eq!(first.text(), "に", "the composed text stays");
    assert_eq!(first.composition(), None);
    let sent = wire.take();
    assert_eq!(
        sent[..3],
        [Sent::Disable, Sent::Commit, Sent::Enable],
        "the input method is reset for the next field"
    );

    client.commit_string(&*wire, Some("x".into()));
    client.done(&*wire, wire.commits());
    assert_eq!(second.text(), "x");
    assert_eq!(first.text(), "に");

    let _ = wire.take();
    client.focus_store(None);
    assert_eq!(wire.take(), [Sent::Disable, Sent::Commit]);
}

#[test]
fn a_protected_field_shows_the_input_method_no_text() {
    let store = InMemoryTextStore::new("secret");
    store.set_protected(true);
    let (_client, wire) = serving(&store);
    let sent = wire.take();
    assert!(sent.contains(&Sent::Content(ContentType {
        hint: ContentType::HINT_HIDDEN_TEXT | ContentType::HINT_SENSITIVE_DATA,
        purpose: ContentType::PASSWORD,
    })));
    assert!(
        !sent.iter().any(|sent| matches!(sent, Sent::Surrounding(_))),
        "{sent:?}"
    );
}

#[test]
fn each_purpose_names_its_content_type() {
    for (purpose, expected) in [
        (TextInputPurpose::Text, ContentType::NORMAL),
        (TextInputPurpose::Search, ContentType::NORMAL),
        (TextInputPurpose::Email, ContentType::EMAIL),
        (TextInputPurpose::Url, ContentType::URL),
        (TextInputPurpose::Phone, ContentType::PHONE),
        (TextInputPurpose::Number, ContentType::NUMBER),
    ] {
        assert_eq!(
            ContentType::of(purpose, false, true),
            ContentType {
                hint: 0,
                purpose: expected,
            },
            "{purpose:?}"
        );
    }
}

#[test]
fn a_long_document_sends_a_window_around_the_selection() {
    // Three-byte characters, so a cut must land on a character boundary.
    let text = "語".repeat(4000);
    let store = InMemoryTextStore::new(text.clone());
    store.app_select(Selection {
        anchor: Utf16Offset::new(2000),
        active: Utf16Offset::new(1990),
    });
    let (_client, wire) = serving(&store);
    let [surrounding] = wire.surroundings().try_into().expect("one state");
    assert!(surrounding.text.len() <= MAX_SURROUNDING_BYTES);
    assert!(surrounding.text.chars().all(|c| c == '語'));
    assert_eq!(
        surrounding.anchor - surrounding.cursor,
        30,
        "the selection is whole, its active end first"
    );
    let after = surrounding.text.len() - surrounding.anchor;
    assert!(
        surrounding.cursor.abs_diff(after) <= 3,
        "the rest is split evenly: {} before, {after} after",
        surrounding.cursor
    );

    // A selection too long to send is a caret at its active end.
    let store = InMemoryTextStore::new(text);
    store.app_select(Selection {
        anchor: Utf16Offset::new(0),
        active: Utf16Offset::new(4000),
    });
    let (_client, wire) = serving(&store);
    let [surrounding] = wire.surroundings().try_into().expect("one state");
    assert_eq!(surrounding.cursor, surrounding.anchor);
    assert_eq!(surrounding.cursor, surrounding.text.len(), "at the end");
}

#[test]
fn a_completed_composition_stays_and_resets_the_input_method() {
    let store = InMemoryTextStore::new("");
    let (client, wire) = serving(&store);
    client.preedit(&*wire, Some("に".into()), 3, 3);
    client.done(&*wire, wire.commits());
    let _ = wire.take();

    let end = client.complete_composition(&as_store(&store));
    assert_eq!(end, Ok(CompositionEnd::Committed));
    assert_eq!(store.text(), "に");
    assert_eq!(store.composition(), None);
    let sent = wire.take();
    assert_eq!(sent.first(), Some(&Sent::Enable));
    assert!(
        sent.contains(&Sent::Content(ContentType::of_store(&*as_store(&store)))),
        "enable resets the content type, which is sent again: {sent:?}"
    );
    assert!(sent.contains(&Sent::Surrounding(around("に", 3, 3))));
}

#[test]
fn an_edit_held_by_the_frame_is_applied_in_order() {
    let store = InMemoryTextStore::new("");
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    let (client, wire) = serving(&store);

    gate.set_open(false);
    client.commit_string(&*wire, Some("a".into()));
    client.done(&*wire, wire.commits());
    client.commit_string(&*wire, Some("b".into()));
    client.done(&*wire, wire.commits());
    assert_eq!(store.text(), "", "the frame transaction holds the edits");

    gate.set_open(true);
    let _ = store.run_deferred_grants();
    assert_eq!(store.text(), "ab", "neither commit is lost, nor reordered");
    assert_eq!(
        wire.surroundings().last(),
        Some(&around("ab", 2, 2)),
        "the input method holds the last edit's state"
    );
}

#[test]
fn a_retired_client_sends_nothing_more() {
    let store = InMemoryTextStore::new("");
    let (client, wire) = serving(&store);
    let _ = wire.take();
    client.retire();
    assert_eq!(wire.take(), [Sent::Disable, Sent::Commit]);
    client.entered(Rc::clone(&wire) as Rc<dyn TextInputWire>);
    client.focus_store(Some(as_store(&store)));
    assert_eq!(wire.take(), []);
}

#[test]
fn a_deletion_counts_from_the_caret_a_too_long_selection_is_told_as() {
    let text = format!("x{}", "a".repeat(4000));
    let store = InMemoryTextStore::new(text);
    store.app_select(Selection {
        anchor: Utf16Offset::new(1),
        active: Utf16Offset::new(4001),
    });
    let (client, wire) = serving(&store);
    let [told] = wire.surroundings().try_into().expect("one state");
    assert_eq!(told.cursor, told.anchor, "a caret at the active end");

    client.delete_surrounding(&*wire, 1, 0);
    client.done(&*wire, wire.commits());
    assert_eq!(
        store.text(),
        format!("x{}", "a".repeat(3999)),
        "the byte before the caret the input method saw goes"
    );
}

#[test]
fn a_nul_reaches_the_input_method_as_one_byte_of_space() {
    let store = InMemoryTextStore::new("a\0b");
    let (_client, wire) = serving(&store);
    assert_eq!(wire.surroundings(), [around("a b", 3, 3)]);
}

#[test]
fn a_completion_held_past_a_focus_change_leaves_the_next_field_alone() {
    let first = InMemoryTextStore::new("");
    let second = InMemoryTextStore::new("next");
    let gate = CommitGate::new();
    first.set_commit_gate(gate.clone());
    let (client, wire) = serving(&first);
    client.preedit(&*wire, Some("に".into()), 3, 3);
    client.done(&*wire, wire.commits());

    gate.set_open(false);
    let _ = client.complete_composition(&as_store(&first));
    client.focus_store(Some(as_store(&second)));
    let _ = wire.take();

    gate.set_open(true);
    let _ = first.run_deferred_grants();
    assert_eq!(first.text(), "に");
    assert_eq!(
        wire.take(),
        [],
        "the first field's late grant neither resets nor describes the second"
    );
}

#[test]
fn a_second_seat_leaving_keeps_the_seat_the_window_serves() {
    let store = InMemoryTextStore::new("");
    let (client, first) = serving(&store);
    let second = Rc::new(Wire::default());
    let _ = first.take();
    client.entered(Rc::clone(&second) as Rc<dyn TextInputWire>);
    assert_eq!(
        first.take(),
        [Sent::Disable, Sent::Commit],
        "the seat no longer served lets the field go"
    );

    assert!(!client.left(&*first), "the first seat is no longer served");
    client.commit_string(&*first, Some("x".into()));
    client.done(&*first, first.commits());
    assert_eq!(store.text(), "", "a seat not served edits nothing");

    client.commit_string(&*second, Some("y".into()));
    client.done(&*second, second.commits());
    assert_eq!(store.text(), "y", "the served seat still edits");
}

#[test]
fn a_focus_moved_by_the_losing_fields_listener_is_the_one_kept() {
    let first = InMemoryTextStore::new("");
    let second = InMemoryTextStore::new("");
    let third = InMemoryTextStore::new("third");
    let (client, wire) = serving(&first);
    client.preedit(&*wire, Some("に".into()), 3, 3);
    client.done(&*wire, wire.commits());

    // Committing the composition where it was tells the field's owner, whose
    // code moves focus on.
    let host = Rc::downgrade(&client);
    let next = as_store(&third);
    first.set_owner_listener(Some(Rc::new(move || {
        if let Some(host) = host.upgrade() {
            host.focus_store(Some(Rc::clone(&next)));
        }
    })));
    client.focus_store(Some(as_store(&second)));

    let _ = wire.take();
    client.commit_string(&*wire, Some("!".into()));
    client.done(&*wire, wire.commits());
    assert_eq!(third.text(), "third!", "the newer focus request holds");
    assert_eq!(second.text(), "");
}

#[test]
fn a_field_its_frame_holds_is_described_once_it_may_be_read() {
    let store = InMemoryTextStore::new("held");
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    gate.set_open(false);
    let (_client, wire) = serving(&store);
    assert_eq!(wire.surroundings(), [], "the frame holds the store");

    gate.set_open(true);
    let _ = store.run_deferred_grants();
    assert_eq!(wire.surroundings(), [around("held", 4, 4)]);
}

#[test]
fn a_completion_the_frame_held_sends_the_status_it_runs_under() {
    let store = InMemoryTextStore::new("");
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    let (client, wire) = serving(&store);
    client.preedit(&*wire, Some("に".into()), 3, 3);
    client.done(&*wire, wire.commits());

    gate.set_open(false);
    let _ = client.complete_composition(&as_store(&store));
    store.set_protected(true);
    let _ = wire.take();
    gate.set_open(true);
    let _ = store.run_deferred_grants();
    let last = wire.take().into_iter().rev().find_map(|sent| match sent {
        Sent::Content(content) => Some(content),
        _ => None,
    });
    assert_eq!(
        last,
        Some(ContentType::of_store(&*as_store(&store))),
        "the password hints are the last the input method hears"
    );
    assert_eq!(
        last.map(|content| content.purpose),
        Some(ContentType::PASSWORD)
    );
}

#[test]
fn a_deletion_around_a_backward_selection_keeps_its_direction() {
    let store = InMemoryTextStore::new("xabcy");
    store.app_select(Selection {
        anchor: Utf16Offset::new(4),
        active: Utf16Offset::new(1),
    });
    let (client, wire) = serving(&store);
    client.delete_surrounding(&*wire, 1, 1);
    client.done(&*wire, wire.commits());
    assert_eq!(store.text(), "abc");
    assert_eq!(
        store.selection(),
        Selection {
            anchor: Utf16Offset::new(3),
            active: Utf16Offset::new(0),
        }
    );
}

#[test]
fn a_field_turned_protected_is_enabled_afresh_with_password_hints() {
    let store = InMemoryTextStore::new("abc");
    let (_client, wire) = serving(&store);
    let _ = wire.take();

    // A status change that keeps the content type sends nothing.
    store.set_purpose(TextInputPurpose::Search);
    assert_eq!(wire.take(), []);

    store.set_protected(true);
    let sent = wire.take();
    assert_eq!(
        sent.first(),
        Some(&Sent::Enable),
        "a fresh enablement: {sent:?}"
    );
    assert!(sent.contains(&Sent::Content(ContentType {
        hint: ContentType::HINT_HIDDEN_TEXT | ContentType::HINT_SENSITIVE_DATA,
        purpose: ContentType::PASSWORD,
    })));
    assert!(
        !sent.iter().any(|sent| matches!(sent, Sent::Surrounding(_))),
        "{sent:?}"
    );
    assert_eq!(sent.last(), Some(&Sent::Commit));
}

#[test]
fn a_deletion_count_past_what_the_input_method_saw_stops_at_it() {
    let store = InMemoryTextStore::new("x".repeat(5000));
    let (client, wire) = serving(&store);
    client.delete_surrounding(&*wire, u32::MAX, 0);
    client.done(&*wire, wire.commits());
    assert_eq!(store.text().len(), 5000 - MAX_SURROUNDING_BYTES);
}
