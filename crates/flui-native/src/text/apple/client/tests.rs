//! The `NSTextInputClient` verbs over a field's store, and the host
//! operations around AppKit's input context, on the reference store.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::Point;
use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::text_store::{
    CommitGate, Composition, CompositionEnd, Selection, TextStore, TextStoreHost,
    TextStoreHostError, commit_composition_in_place,
};
use flui_testing::text_store_kit::InMemoryTextStore;

use super::{InputContext, TextClient};

/// AppKit's input context as the client reaches it: the calls it got, and
/// what the input method does when told to discard its marked text.
#[derive(Default)]
struct Context {
    discards: Cell<usize>,
    invalidations: Cell<usize>,
    on_discard: RefCell<Option<Box<dyn Fn()>>>,
    /// The holds on secure event input the client has taken and not given
    /// back.
    secure_holds: Cell<isize>,
    /// Whether the system refuses the next change of secure event input.
    refuse_secure: Cell<bool>,
}

impl InputContext for Context {
    fn discard_marked_text(&self) {
        self.discards.set(self.discards.get() + 1);
        let answer = self.on_discard.borrow_mut().take();
        if let Some(answer) = answer {
            answer();
        }
    }

    fn invalidate_character_coordinates(&self) {
        self.invalidations.set(self.invalidations.get() + 1);
    }

    fn set_secure_input(&self, secure: bool) -> bool {
        if self.refuse_secure.replace(false) {
            return false;
        }
        let holds = self.secure_holds.get() + if secure { 1 } else { -1 };
        assert!(holds >= 0, "a hold was given back that was never taken");
        self.secure_holds.set(holds);
        true
    }
}

fn span(start: usize, end: usize) -> Utf16Range {
    Utf16Range::try_new(Utf16Offset::new(start), Utf16Offset::new(end)).expect("an ordered span")
}

fn caret(at: usize) -> Selection {
    Selection::collapsed(Utf16Offset::new(at))
}

fn composing(range: Utf16Range) -> Composition {
    Composition {
        range,
        hides_caret: false,
    }
}

/// A client bound to a fresh context, serving `store`.
fn serving(store: &Rc<InMemoryTextStore>) -> (Rc<TextClient>, Rc<Context>) {
    let client = TextClient::new();
    let context = Rc::new(Context::default());
    client.bind(Rc::clone(&context) as Rc<dyn InputContext>);
    client.focus_store(Some(Rc::clone(store) as Rc<dyn TextStore>));
    (client, context)
}

fn as_store(store: &Rc<InMemoryTextStore>) -> Rc<dyn TextStore> {
    Rc::clone(store) as Rc<dyn TextStore>
}

#[test]
fn kana_composed_then_converted_is_committed_in_the_field() {
    let store = InMemoryTextStore::new("ab");
    let (client, _) = serving(&store);

    client.set_marked_text("に".into(), span(1, 1), None);
    assert_eq!(store.text(), "abに");
    assert_eq!(store.composition(), Some(composing(span(2, 3))));
    assert_eq!(store.selection(), caret(3));
    assert!(
        !client.reports_key_release(),
        "the press went to the input method"
    );

    client.set_marked_text("にほん".into(), span(3, 3), None);
    assert_eq!(
        store.text(),
        "abにほん",
        "the composition is replaced, not appended to"
    );
    assert_eq!(client.marked_range(), Some(span(2, 5)));

    client.insert_text("日本".into(), None);
    assert_eq!(store.text(), "ab日本");
    assert_eq!(store.composition(), None);
    assert_eq!(store.selection(), caret(4));
    assert_eq!(client.marked_range(), None);
    assert!(client.reports_key_release());
}

#[test]
fn an_edit_replaces_the_range_appkit_names() {
    struct Case {
        name: &'static str,
        text: &'static str,
        replacement: Option<Utf16Range>,
        expected: &'static str,
    }
    let cases = [
        Case {
            // Press-and-hold `e`, then pick `é`: AppKit replaces the `e`.
            name: "an accent replaces the letter it was held on",
            text: "é",
            replacement: Some(span(3, 4)),
            expected: "café",
        },
        Case {
            name: "no range inserts at the caret",
            text: "!",
            replacement: None,
            expected: "cafe!",
        },
        Case {
            name: "a range past the end is clamped to the document",
            text: "s",
            replacement: Some(span(9, 12)),
            expected: "cafes",
        },
    ];
    for case in cases {
        let store = InMemoryTextStore::new("cafe");
        let (client, _) = serving(&store);
        client.insert_text(case.text.into(), case.replacement);
        assert_eq!(store.text(), case.expected, "{}", case.name);
    }
}

#[test]
fn a_reconversion_reads_the_field_as_it_stands() {
    let store = InMemoryTextStore::new("hello");
    let (client, _) = serving(&store);

    assert_eq!(client.selected_range(), Some(span(5, 5)));
    assert_eq!(
        client.substring(span(1, 3)),
        Some(("el".to_owned(), span(1, 3)))
    );
    assert_eq!(
        client.substring(span(3, 40)),
        Some(("lo".to_owned(), span(3, 5))),
        "the range answered is the one that exists"
    );
    let (bounds, range) = client
        .first_rect(Some(span(1, 2)))
        .expect("a laid-out field");
    assert_eq!(range, span(1, 2));
    assert_eq!(bounds.origin.x, InMemoryTextStore::ADVANCE);
    assert_eq!(
        client.character_index(Point::new(InMemoryTextStore::ADVANCE * 2.0 + 1.0, 5.0)),
        Some(Utf16Offset::new(2))
    );

    store.set_protected(true);
    assert_eq!(
        client.substring(span(0, 2)),
        None,
        "a password is not read out"
    );
}

#[test]
fn the_candidate_window_follows_the_composition() {
    let store = InMemoryTextStore::new("ab");
    let (client, context) = serving(&store);
    let focused = context.invalidations.get();
    client.set_marked_text("か".into(), span(1, 1), None);

    let (_, range) = client.first_rect(None).expect("a laid-out field");
    assert_eq!(range, span(2, 3), "an unnamed range is the composition");

    store.screen_moved();
    assert_eq!(context.invalidations.get(), focused + 1);
}

#[test]
fn completing_a_composition_makes_appkit_forget_its_marked_text() {
    // The input method either commits its marked text when told to discard
    // it, or drops it and leaves the store composing.
    for commits in [true, false] {
        let store = InMemoryTextStore::new("");
        let (client, context) = serving(&store);
        client.set_marked_text("にほん".into(), span(3, 3), None);
        if commits {
            let client = Rc::downgrade(&client);
            *context.on_discard.borrow_mut() = Some(Box::new(move || {
                if let Some(client) = client.upgrade() {
                    client.unmark_text();
                }
            }));
        }

        let end = client.complete_composition(&as_store(&store));

        assert_eq!(context.discards.get(), 1, "commits: {commits}");
        let expected = if commits {
            CompositionEnd::Committed
        } else {
            CompositionEnd::Abandoned
        };
        assert_eq!(end, Ok(expected), "commits: {commits}");
        if !commits {
            // What the presentation does with `Abandoned`.
            let _ = commit_composition_in_place(&*store);
        }
        assert_eq!(store.composition(), None, "commits: {commits}");
        assert_eq!(store.text(), "にほん", "commits: {commits}");
        // The Enter that follows commits nothing a second time.
        client.insert_text(String::new(), None);
        assert_eq!(store.text(), "にほん", "commits: {commits}");
    }
}

#[test]
fn an_application_edit_ending_the_composition_ends_appkits() {
    let store = InMemoryTextStore::new("ab");
    let (client, context) = serving(&store);
    client.set_marked_text("にほん".into(), span(3, 3), None);

    store.app_replace(span(0, 5), "x");

    assert_eq!(store.composition(), None);
    assert_eq!(context.discards.get(), 1);
    assert!(client.reports_key_release());
}

#[test]
fn an_application_edit_beside_the_composition_keeps_it() {
    let store = InMemoryTextStore::new("ab");
    let (client, context) = serving(&store);
    client.set_marked_text("に".into(), span(1, 1), None);

    store.app_replace(span(0, 1), "");

    assert_eq!(store.composition(), Some(composing(span(1, 2))));
    assert_eq!(context.discards.get(), 0);
}

#[test]
fn a_field_losing_focus_keeps_what_was_composed_in_it() {
    let first = InMemoryTextStore::new("");
    let second = InMemoryTextStore::new("");
    let (client, context) = serving(&first);
    client.set_marked_text("にほん".into(), span(3, 3), None);

    client.focus_store(Some(as_store(&second)));

    assert_eq!(context.discards.get(), 1);
    assert_eq!(first.text(), "にほん");
    assert_eq!(first.composition(), None, "committed in place");
    client.insert_text("x".into(), None);
    assert_eq!(second.text(), "x");
    assert_eq!(first.text(), "にほん");
}

#[test]
fn an_edit_inside_a_frame_waits_for_it_instead_of_being_dropped() {
    let store = InMemoryTextStore::new("");
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    let (client, _) = serving(&store);

    gate.set_open(false);
    client.insert_text("a".into(), None);
    assert_eq!(store.text(), "", "the frame transaction holds the edit");
    assert_eq!(
        client.selected_range(),
        None,
        "a query is refused, not answered stale"
    );

    gate.set_open(true);
    store.run_deferred_grants();
    assert_eq!(store.text(), "a");
}

#[test]
fn focus_moved_by_the_input_method_waits_for_its_call_to_return() {
    let first = InMemoryTextStore::new("");
    let (client, context) = serving(&first);
    client.set_marked_text("か".into(), span(1, 1), None);
    let inner = Rc::downgrade(&client);
    *context.on_discard.borrow_mut() = Some(Box::new(move || {
        if let Some(client) = inner.upgrade() {
            client.insert_text("火".into(), None);
            // The commit's application code moves focus away.
            client.focus_store(None);
            assert!(client.takes_key_presses(), "applied once this call returns");
        }
    }));

    assert_eq!(
        client.complete_composition(&as_store(&first)),
        Ok(CompositionEnd::Committed)
    );
    assert_eq!(first.text(), "火");
    assert!(!client.takes_key_presses());
}

#[test]
fn a_retired_client_reaches_neither_appkit_nor_the_field() {
    let store = InMemoryTextStore::new("ab");
    let (client, context) = serving(&store);
    client.set_marked_text("か".into(), span(1, 1), None);
    client.retire();

    assert!(matches!(
        client.complete_composition(&as_store(&store)),
        Err(TextStoreHostError::Unsupported(_))
    ));
    client.insert_text("x".into(), None);
    assert_eq!(store.text(), "abか");
    assert_eq!(context.discards.get(), 0);
    assert!(!client.takes_key_presses());
}

#[test]
fn completing_another_fields_composition_is_refused() {
    let focused = InMemoryTextStore::new("");
    let other = InMemoryTextStore::new("");
    let (client, _) = serving(&focused);
    assert_eq!(
        client.complete_composition(&as_store(&other)),
        Err(TextStoreHostError::NotFocused)
    );
}

/// Secure event input is held exactly while a protected field has focus:
/// taken as it gains focus or becomes protected, given back as it loses
/// either, and given back when the view goes, never twice.
#[test]
fn a_protected_field_holds_secure_input_while_it_has_focus() {
    let password = InMemoryTextStore::new("");
    password.set_protected(true);
    let plain = InMemoryTextStore::new("");
    let (client, context) = serving(&password);
    assert_eq!(
        context.secure_holds.get(),
        1,
        "a protected field took focus"
    );

    client.focus_store(Some(as_store(&plain)));
    assert_eq!(context.secure_holds.get(), 0, "a plain field took focus");

    plain.set_protected(true);
    assert_eq!(
        context.secure_holds.get(),
        1,
        "the focused field became protected"
    );
    plain.set_protected(false);
    assert_eq!(
        context.secure_holds.get(),
        0,
        "the focused field stopped being protected"
    );

    client.focus_store(Some(as_store(&password)));
    client.focus_store(Some(as_store(&password)));
    assert_eq!(
        context.secure_holds.get(),
        1,
        "focus given twice holds once"
    );
    client.focus_store(None);
    assert_eq!(context.secure_holds.get(), 0, "focus left");

    client.focus_store(Some(as_store(&password)));
    client.retire();
    client.retire();
    assert_eq!(context.secure_holds.get(), 0, "the view went");
    password.set_protected(false);
    password.set_protected(true);
    assert_eq!(
        context.secure_holds.get(),
        0,
        "a retired client takes nothing"
    );
}

/// A field focused before the view exists is held once the view binds, and
/// a hold the system refused is not given back.
#[test]
fn secure_input_follows_the_bind_and_the_systems_refusal() {
    let password = InMemoryTextStore::new("");
    password.set_protected(true);
    let client = TextClient::new();
    client.focus_store(Some(as_store(&password)));
    let context = Rc::new(Context::default());
    client.bind(Rc::clone(&context) as Rc<dyn InputContext>);
    assert_eq!(context.secure_holds.get(), 1, "held once the view bound");

    client.focus_store(None);
    context.refuse_secure.set(true);
    client.focus_store(Some(as_store(&password)));
    assert_eq!(context.secure_holds.get(), 0, "the system refused the hold");
    client.focus_store(None);
    assert_eq!(context.secure_holds.get(), 0, "nothing to give back");
}
