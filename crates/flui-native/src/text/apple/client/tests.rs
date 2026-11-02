//! The `NSTextInputClient` verbs over a field's store, and the host
//! operations around AppKit's input context, on the reference store.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::Point;
use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::text_store::{
    CommitGate, Composition, CompositionEnd, Selection, TextStore, TextStoreHost,
    TextStoreHostError, TextStoreObserver, TextStoreStatus, commit_composition_in_place,
};
use flui_testing::text_store_kit::InMemoryTextStore;

use super::super::secure_input::SecureHold;
use super::{DocumentChange, InputContext, MarkedText, TextClient, Typed};

/// The input method's context as the client reaches it: the calls it got,
/// what the input method does when told to discard its marked text, and
/// macOS's secure event input, held through [`SecureHold`] as the AppKit
/// context holds it.
struct Context {
    discards: Cell<usize>,
    invalidations: Cell<usize>,
    on_discard: RefCell<Option<Box<dyn Fn()>>>,
    /// Each status the client told, in order.
    statuses: RefCell<Vec<Option<TextStoreStatus>>>,
    /// Each application change the client told, in order.
    changes: RefCell<Vec<DocumentChange>>,
    will_changes: RefCell<Vec<DocumentChange>>,
    on_will_change: RefCell<Option<Box<dyn Fn()>>>,
    /// What the input method does when told of an application change.
    on_change: RefCell<Option<Box<dyn Fn()>>>,
    on_field: RefCell<Option<Box<dyn Fn()>>>,
    on_drop: Option<Box<dyn Fn()>>,
    secure: SecureHold<Box<dyn Fn(bool) -> bool>>,
    /// The holds on secure event input the client has taken and not given
    /// back.
    secure_holds: Rc<Cell<isize>>,
    /// Whether the view's window has resigned key status.
    resigned: Cell<bool>,
    /// Whether the system refuses the next change of secure event input.
    refuse_secure: Rc<Cell<bool>>,
}

impl Default for Context {
    fn default() -> Self {
        let secure_holds = Rc::new(Cell::new(0_isize));
        let refuse_secure = Rc::new(Cell::new(false));
        let change: Box<dyn Fn(bool) -> bool> = Box::new({
            let holds = Rc::clone(&secure_holds);
            let refuse = Rc::clone(&refuse_secure);
            move |secure| {
                if refuse.replace(false) {
                    return false;
                }
                let count = holds.get() + if secure { 1 } else { -1 };
                assert!(count >= 0, "a hold was given back that was never taken");
                holds.set(count);
                true
            }
        });
        Self {
            discards: Cell::new(0),
            invalidations: Cell::new(0),
            on_discard: RefCell::new(None),
            statuses: RefCell::new(Vec::new()),
            changes: RefCell::new(Vec::new()),
            will_changes: RefCell::new(Vec::new()),
            on_will_change: RefCell::new(None),
            on_change: RefCell::new(None),
            on_field: RefCell::new(None),
            on_drop: None,
            secure: SecureHold::new(change),
            secure_holds,
            resigned: Cell::new(false),
            refuse_secure,
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if let Some(on_drop) = self.on_drop.take() {
            on_drop();
        }
    }
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

    fn field_changed(&self, status: Option<TextStoreStatus>) {
        let answer = self.on_field.borrow_mut().take();
        if let Some(answer) = answer {
            answer();
        }
        self.statuses.borrow_mut().push(status);
        let wanted = status.is_some_and(|status| status.protected) && !self.resigned.get();
        self.secure.sync(wanted);
    }

    fn document_changed(&self, change: DocumentChange, marked_text: MarkedText) {
        if marked_text == MarkedText::Discard {
            self.discard_marked_text();
        }
        self.changes.borrow_mut().push(change);
        let answer = self.on_change.borrow_mut().take();
        if let Some(answer) = answer {
            answer();
        }
    }

    fn document_will_change(&self, change: DocumentChange) {
        self.will_changes.borrow_mut().push(change);
        let answer = self.on_will_change.borrow_mut().take();
        if let Some(answer) = answer {
            answer();
        }
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
fn serving<S: TextStore + 'static>(store: &Rc<S>) -> (Rc<TextClient>, Rc<Context>) {
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

/// The hold follows the window's key status: another application's keyboard
/// utilities work while this window is in the background.
#[test]
fn secure_input_is_given_back_while_the_window_is_not_key() {
    let password = InMemoryTextStore::new("");
    password.set_protected(true);
    let (client, context) = serving(&password);
    assert_eq!(context.secure_holds.get(), 1);

    context.resigned.set(true);
    client.key_window_changed();
    assert_eq!(context.secure_holds.get(), 0, "the window resigned key");
    password.set_protected(false);
    password.set_protected(true);
    assert_eq!(
        context.secure_holds.get(),
        0,
        "no hold outside the key window"
    );

    context.resigned.set(false);
    client.key_window_changed();
    assert_eq!(context.secure_holds.get(), 1, "the window became key again");
}

/// Application code may reenter the host from a status or read request.
struct ReentrantStore {
    inner: Rc<InMemoryTextStore>,
    on_status: RefCell<Option<Box<dyn FnOnce()>>>,
    on_read: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl TextStore for ReentrantStore {
    fn status(&self) -> TextStoreStatus {
        let on_status = self.on_status.borrow_mut().take();
        if let Some(on_status) = on_status {
            on_status();
        }
        self.inner.status()
    }

    fn request_lock(
        &self,
        grant: flui_platform::text_store::LockGrant,
        timing: flui_platform::text_store::LockTiming,
    ) -> Result<flui_platform::text_store::LockOutcome, flui_platform::text_store::TextStoreError>
    {
        if matches!(&grant, flui_platform::text_store::LockGrant::Read(_)) {
            let on_read = self.on_read.borrow_mut().take();
            if let Some(on_read) = on_read {
                on_read();
            }
        }
        self.inner.request_lock(grant, timing)
    }

    fn run_deferred_grants(&self) -> usize {
        self.inner.run_deferred_grants()
    }

    fn set_commit_gate(&self, gate: CommitGate) {
        self.inner.set_commit_gate(gate);
    }

    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.inner.set_observer(observer);
    }
}

#[test]
fn a_focus_moved_by_a_status_query_is_held_for() {
    let password = InMemoryTextStore::new("");
    password.set_protected(true);
    let client = TextClient::new();
    let context = Rc::new(Context::default());
    client.bind(Rc::clone(&context) as Rc<dyn InputContext>);
    let plain = Rc::new(ReentrantStore {
        inner: InMemoryTextStore::new(""),
        on_read: RefCell::new(None),
        on_status: RefCell::new(Some(Box::new({
            let client = Rc::clone(&client);
            let password = as_store(&password);
            move || client.focus_store(Some(password))
        }))),
    });

    client.focus_store(Some(Rc::clone(&plain) as Rc<dyn TextStore>));
    assert_eq!(
        context.secure_holds.get(),
        1,
        "the protected field the status query focused is held"
    );
    client.focus_store(None);
    assert_eq!(context.secure_holds.get(), 0);
}

/// The input method hears of the application's edits, text and selection
/// apart, and never of its own: UIKit drops its marked text when told the
/// document changed under it, so an edit it made itself must not be told
/// back.
#[test]
fn the_input_method_hears_the_applications_edits_and_not_its_own() {
    let store = InMemoryTextStore::new("ab");
    store.app_select(caret(2));
    let (client, context) = serving(&store);

    client.set_marked_text("か".to_owned(), span(1, 1), None);
    client.insert_text("漢".to_owned(), None);
    client.replace(span(0, 1), "A".to_owned());
    client.select(span(1, 1));
    client.unmark_text();
    assert_eq!(store.text(), "Ab漢");
    assert_eq!(
        *context.changes.borrow(),
        [],
        "the input method's own edits are not told back"
    );

    store.app_replace(span(0, 1), "x");
    assert_eq!(
        context.changes.borrow().first(),
        Some(&DocumentChange::Text),
        "an application's edit of the text"
    );
    let told = context.changes.borrow().len();
    store.app_select(caret(0));
    assert_eq!(
        context.changes.borrow()[told..],
        [DocumentChange::Selection],
        "an application's move of the selection alone"
    );

    client.focus_store(None);
    let told = context.changes.borrow().len();
    store.app_replace(span(0, 1), "y");
    assert_eq!(
        context.changes.borrow().len(),
        told,
        "a field without focus is not the input method's"
    );
}

/// An input method that answers an application's edit by editing the field
/// (UIKit committing its marked text) edits it under that notice, and a
/// focus move it causes waits for the notice to return.
#[test]
fn an_edit_answering_the_applications_notice_lands() {
    let store = InMemoryTextStore::new("ab");
    store.app_select(caret(2));
    let other = InMemoryTextStore::new("");
    let (client, context) = serving(&store);
    *context.on_change.borrow_mut() = Some(Box::new({
        let client = Rc::clone(&client);
        let other = as_store(&other);
        let context = Rc::clone(&context);
        let store = Rc::clone(&store);
        move || {
            client.select(span(2, 2));
            client.insert_text("c".to_owned(), None);
            *context.on_field.borrow_mut() = Some(Box::new({
                let store = Rc::clone(&store);
                move || {
                    assert_eq!(
                        store.text(),
                        "abc",
                        "accepted edits precede the focus transition"
                    );
                }
            }));
            client.focus_store(Some(Rc::clone(&other)));
            assert!(
                client.document_len() == Some(Utf16Offset::new(2)),
                "focus moves only once the notice returns"
            );
        }
    }));
    store.app_select(caret(1));
    assert_eq!(store.text(), "abc");
    assert_eq!(
        client.document_len(),
        Some(Utf16Offset::new(0)),
        "then it moved"
    );
}

/// The field's status reaches the input method as focus, status, binding
/// and retirement change it; `None` once no field has focus.
#[test]
fn the_input_method_is_told_the_fields_status() {
    let password = InMemoryTextStore::new("");
    password.set_protected(true);
    let client = TextClient::new();
    let context = Rc::new(Context::default());
    client.bind(Rc::clone(&context) as Rc<dyn InputContext>);
    client.focus_store(Some(as_store(&password)));
    password.set_protected(false);
    client.focus_store(None);
    client.focus_store(Some(as_store(&password)));
    client.retire();
    let told = context
        .statuses
        .borrow()
        .iter()
        .map(|status| status.map(|status| status.protected))
        .collect::<Vec<Option<bool>>>();
    assert_eq!(
        told,
        [None, Some(true), Some(false), None, Some(false), None],
        "bind, focus, status, focus left, focus, retirement"
    );
}

/// UIKit's verbs beyond AppKit's: a replacement and a selection clamped to
/// the document, and the document's length.
#[test]
fn uikit_replaces_selects_and_measures_the_field() {
    let store = InMemoryTextStore::new("old cat");
    store.app_select(caret(7));
    let (client, _context) = serving(&store);
    assert_eq!(client.document_len(), Some(Utf16Offset::new(7)));
    client.replace(span(0, 3), "the".to_owned());
    assert_eq!(store.text(), "the cat");
    client.replace(span(4, 99), "dog".to_owned());
    assert_eq!(store.text(), "the dog");
    client.select(span(4, 99));
    assert_eq!(
        store.selection(),
        Selection {
            anchor: Utf16Offset::new(4),
            active: Utf16Offset::new(7),
        }
    );
    client.focus_store(None);
    assert_eq!(client.document_len(), None, "no field has focus");
}

#[test]
fn uikit_ranges_keep_supplementary_scalars_whole() {
    for protected in [false, true] {
        for (text, at, backward, expected) in [
            ("😀", 0, false, span(0, 2)),
            ("😀", 2, true, span(0, 2)),
            ("😀", 1, false, span(0, 2)),
            ("😀", 1, true, span(0, 0)),
            ("😀", 9, true, span(0, 2)),
            ("a😀b", 1, false, span(1, 3)),
            ("a😀b", 3, true, span(1, 3)),
            ("a😀b", 2, false, span(1, 3)),
            ("a", 0, false, span(0, 1)),
            ("a", 1, true, span(0, 1)),
            ("\n\0", 0, false, span(0, 1)),
            ("\n\0", 2, true, span(1, 2)),
            ("", 0, false, span(0, 0)),
            ("a", 0, true, span(0, 0)),
            ("a", 1, false, span(1, 1)),
        ] {
            let store = InMemoryTextStore::new(text);
            store.set_protected(protected);
            let (client, _) = serving(&store);
            let range = client.extended_range(Utf16Offset::new(at), backward);
            assert_eq!(range, Some(expected), "{text:?}, {at}, backward={backward}");
            assert_eq!(
                client.substring(expected).is_some(),
                !protected,
                "only a plain field exposes text"
            );
        }
        let advance = InMemoryTextStore::ADVANCE;
        for (text, point, expected) in [
            ("😀x", Point::new(advance * 0.25, 1.0), Some(span(0, 2))),
            ("😀x", Point::new(advance * 0.75, 1.0), Some(span(0, 2))),
            ("ab", Point::new(advance * 0.25, 1.0), Some(span(0, 1))),
            ("ab", Point::new(advance * 0.75, 1.0), Some(span(0, 1))),
            ("ab", Point::new(advance * 1.25, 1.0), Some(span(1, 2))),
            ("ab", Point::new(advance * 1.75, 1.0), Some(span(1, 2))),
            ("ab", Point::new(-1.0, 1.0), None),
            ("ab", Point::new(advance * 2.0, 1.0), None),
            ("ab", Point::new(1.0, -1.0), None),
            ("", Point::new(0.0, 1.0), None),
        ] {
            let store = InMemoryTextStore::new(text);
            store.set_protected(protected);
            let (client, _) = serving(&store);
            assert_eq!(
                client.character_range_at_point(point),
                expected,
                "{text:?}, {point:?}, protected={protected}"
            );
        }
        let store = InMemoryTextStore::new("😀x");
        let (client, _) = serving(&store);
        assert_eq!(
            client.character_index(Point::new(advance * 0.75, 1.0)),
            Some(Utf16Offset::new(2)),
            "nearest caret remains a distinct query"
        );
        client.focus_store(None);
        assert_eq!(client.extended_range(Utf16Offset::ZERO, false), None);
    }
}

#[test]
fn point_derived_ranges_stay_with_the_queried_field() {
    let store = Rc::new(ReentrantStore {
        inner: InMemoryTextStore::new("😀x"),
        on_status: RefCell::new(None),
        on_read: RefCell::new(None),
    });
    let other = InMemoryTextStore::new("other");
    let (client, _) = serving(&store);
    *store.on_read.borrow_mut() = Some(Box::new({
        let client = Rc::clone(&client);
        let other = as_store(&other);
        move || client.focus_store(Some(other))
    }));
    assert_eq!(
        client.character_range_at_point(Point::new(1.0, 1.0)),
        Some(span(0, 2))
    );
    assert_eq!(
        client.document_len(),
        Some(Utf16Offset::new(5)),
        "then focus moves"
    );
}

#[test]
fn a_failing_point_read_delivers_reentrant_focus_and_keeps_its_first_failure() {
    let store = Rc::new(ReentrantStore {
        inner: InMemoryTextStore::new("😀x"),
        on_status: RefCell::new(None),
        on_read: RefCell::new(None),
    });
    let other = InMemoryTextStore::new("other");
    let latest = InMemoryTextStore::new("latest");
    latest.set_protected(true);
    let (client, context) = serving(&store);
    *store.on_read.borrow_mut() = Some(Box::new({
        let client = Rc::clone(&client);
        let other = as_store(&other);
        move || {
            client.focus_store(Some(other));
            panic!("first point read failure");
        }
    }));
    *context.on_field.borrow_mut() = Some(Box::new({
        let client = Rc::clone(&client);
        let latest = as_store(&latest);
        move || {
            client.focus_store(Some(Rc::clone(&latest)));
            panic!("later focus notice failure");
        }
    }));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.character_range_at_point(Point::new(1.0, 1.0))
    }))
    .expect_err("the first read failure propagates after cleanup");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"first point read failure")
    );
    assert_eq!(
        context.secure_holds.get(),
        1,
        "the newest protected focus was delivered before returning"
    );
    client.insert_text("!".to_owned(), None);
    assert!(
        latest.text().contains('!'),
        "the next operation uses the newest field"
    );
}

/// A lone line break from the keyboard is the Return key, which the field
/// answers as a press; any other text, a line break inside it included, is
/// inserted.
#[test]
fn a_lone_line_break_is_the_return_key() {
    let store = InMemoryTextStore::new("");
    let (client, _context) = serving(&store);
    for (typed, answer, text) in [
        ("a", Typed::Inserted, "a"),
        ("\n", Typed::Return, "a"),
        ("b\nc", Typed::Inserted, "ab\nc"),
        ("\n\n", Typed::Inserted, "ab\nc\n\n"),
    ] {
        assert_eq!(client.type_text(typed.to_owned()), answer, "{typed:?}");
        assert_eq!(store.text(), text, "{typed:?}");
    }
}

/// A synchronous keyboard callback cannot publish an older field after a
/// focus move or retirement it requested itself.
#[test]
fn a_field_notice_finishes_before_its_reentrant_focus_change() {
    for retire in [false, true] {
        let plain = InMemoryTextStore::new("");
        let password = InMemoryTextStore::new("");
        password.set_protected(true);
        let (client, context) = serving(&plain);
        *context.on_field.borrow_mut() = Some(Box::new({
            let client = Rc::clone(&client);
            move || {
                if retire {
                    client.retire();
                } else {
                    client.focus_store(None);
                }
            }
        }));
        client.focus_store(Some(as_store(&password)));
        assert_eq!(context.secure_holds.get(), 0, "retire={retire}");
        assert_eq!(client.document_len(), None);
    }
}

/// Work accepted inside a notice is drained even when that notice fails;
/// a second failing notice cannot replace the original panic.
#[test]
fn a_panicking_document_notice_still_delivers_accepted_work() {
    let store = InMemoryTextStore::new("ab");
    let other = InMemoryTextStore::new("next");
    let (client, context) = serving(&store);
    *context.on_change.borrow_mut() = Some(Box::new({
        let client = Rc::clone(&client);
        let other = as_store(&other);
        let context = Rc::clone(&context);
        move || {
            client.insert_text("x".to_owned(), None);
            client.focus_store(Some(Rc::clone(&other)));
            *context.on_field.borrow_mut() = Some(Box::new(|| panic!("later field notice")));
            panic!("first document notice");
        }
    }));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.app_select(caret(1));
    }));
    let failure = failure.expect_err("the first notice must propagate");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"first document notice")
    );
    assert_eq!(client.document_len(), Some(Utf16Offset::new(4)));
    client.insert_text("!".to_owned(), None);
    assert!(other.text().contains('!'), "the next operation works");
}

#[test]
fn retirement_releases_the_store_even_when_the_keyboard_panics() {
    let store = InMemoryTextStore::new("ab");
    let (client, context) = serving(&store);
    *context.on_field.borrow_mut() = Some(Box::new(|| panic!("retirement notice")));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| client.retire()));
    assert!(failure.is_err());
    assert_eq!(client.document_len(), None, "no retained retired field");
    let notices = context.changes.borrow().len();
    store.app_select(caret(1));
    assert_eq!(context.changes.borrow().len(), notices, "observer detached");
    client.retire();
}

#[test]
fn rebinding_gives_the_previous_context_its_secure_hold_back() {
    for refused_release in [false, true] {
        let password = InMemoryTextStore::new("");
        password.set_protected(true);
        let (client, previous) = serving(&password);
        let previous_holds = Rc::clone(&previous.secure_holds);
        previous.refuse_secure.set(refused_release);
        drop(previous); // The client owns the previous context's last reference.
        let next = Rc::new(Context::default());
        client.bind(Rc::clone(&next) as Rc<dyn InputContext>);
        assert_eq!(previous_holds.get(), 0, "refused_release={refused_release}");
        assert_eq!(next.secure_holds.get(), 1);
        client.retire();
        assert_eq!(next.secure_holds.get(), 0);
    }
}

#[test]
fn retirement_delivers_a_completion_accepted_inside_a_notice() {
    let store = InMemoryTextStore::new("ab");
    let (client, context) = serving(&store);
    client.set_marked_text("x".to_owned(), span(1, 1), None);
    *context.on_change.borrow_mut() = Some(Box::new({
        let client = Rc::clone(&client);
        let store = as_store(&store);
        move || {
            assert_eq!(
                client.complete_composition(&store),
                Ok(CompositionEnd::Deferred)
            );
            client.retire();
            panic!("notice after accepted completion");
        }
    }));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.app_select(caret(0));
    }));
    assert!(failure.is_err());
    assert_eq!(
        store.composition(),
        None,
        "accepted completion committed in place"
    );
    assert_eq!(client.document_len(), None);
}

#[test]
fn a_replaced_context_can_drop_with_reentry_and_a_panic() {
    let password = InMemoryTextStore::new("");
    password.set_protected(true);
    let client = TextClient::new();
    let dropped = Rc::new(Cell::new(false));
    let mut previous = Context::default();
    previous.on_drop = Some(Box::new({
        let client = Rc::downgrade(&client);
        let dropped = Rc::clone(&dropped);
        move || {
            client.upgrade().expect("live client").focus_store(None);
            dropped.set(true);
            panic!("replaced context drop");
        }
    }));
    client.bind(Rc::new(previous) as Rc<dyn InputContext>);
    client.focus_store(Some(as_store(&password)));
    let next = Rc::new(Context::default());
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.bind(Rc::clone(&next) as Rc<dyn InputContext>);
    }));
    let failure = failure.expect_err("drop failure propagated");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"replaced context drop")
    );
    assert!(dropped.get());
    assert_eq!(client.document_len(), None);
    assert_eq!(next.secure_holds.get(), 0);
    client.focus_store(Some(as_store(&password)));
    assert_eq!(next.secure_holds.get(), 1, "the next operation works");
    client.retire();
}

#[test]
fn application_notices_bracket_the_document_the_keyboard_reads() {
    let store = InMemoryTextStore::new("ab");
    store.app_select(caret(2));
    let (client, context) = serving(&store);
    let documents = Rc::new(RefCell::new(Vec::new()));
    *context.on_will_change.borrow_mut() = Some(Box::new({
        let client = Rc::clone(&client);
        let documents = Rc::clone(&documents);
        move || {
            documents.borrow_mut().push((
                "will",
                client.substring(span(0, 99)),
                client.selected_range(),
            ));
        }
    }));
    *context.on_change.borrow_mut() = Some(Box::new({
        let client = Rc::clone(&client);
        let documents = Rc::clone(&documents);
        move || {
            documents.borrow_mut().push((
                "did",
                client.substring(span(0, 99)),
                client.selected_range(),
            ));
        }
    }));
    store.app_replace(span(0, 1), "XYZ");
    assert_eq!(
        *context.will_changes.borrow(),
        [DocumentChange::Text, DocumentChange::Selection]
    );
    assert_eq!(
        *documents.borrow(),
        [
            (
                "will",
                Some(("ab".to_owned(), span(0, 2))),
                Some(span(2, 2))
            ),
            (
                "did",
                Some(("XYZb".to_owned(), span(0, 4))),
                Some(store.selection().range())
            ),
        ]
    );
    context.will_changes.borrow_mut().clear();
    client.select(span(0, 0));
    assert!(
        context.will_changes.borrow().is_empty(),
        "platform edits are not echoed"
    );
}

#[test]
fn a_will_notice_keeps_its_recipient_until_publication_or_retirement() {
    #[derive(Clone, Copy, Debug)]
    enum DuringWill {
        Focus,
        Bind,
        Retire,
    }
    for action in [DuringWill::Focus, DuringWill::Bind, DuringWill::Retire] {
        let store = InMemoryTextStore::new("ab");
        let other = InMemoryTextStore::new("next");
        let (client, context) = serving(&store);
        let replacement = Rc::new(Context::default());
        *context.on_will_change.borrow_mut() = Some(Box::new({
            let client = Rc::clone(&client);
            let other = as_store(&other);
            let replacement = Rc::clone(&replacement);
            move || match action {
                DuringWill::Focus => {
                    client.focus_store(Some(Rc::clone(&other)));
                    assert_eq!(client.document_len(), Some(Utf16Offset::new(2)));
                }
                DuringWill::Bind => client.bind(Rc::clone(&replacement) as Rc<dyn InputContext>),
                DuringWill::Retire => client.retire(),
            }
        }));
        store.app_replace(span(0, 1), "XYZ");
        assert_eq!(store.text(), "XYZb", "published despite {action:?}");
        match action {
            DuringWill::Focus => {
                assert_eq!(
                    *context.will_changes.borrow(),
                    [DocumentChange::Text, DocumentChange::Selection]
                );
                assert_eq!(
                    *context.changes.borrow(),
                    [DocumentChange::Text, DocumentChange::Selection]
                );
                assert_eq!(client.document_len(), Some(Utf16Offset::new(4)));
            }
            DuringWill::Bind | DuringWill::Retire => {
                assert_eq!(*context.will_changes.borrow(), [DocumentChange::Text]);
                assert!(context.changes.borrow().is_empty(), "superseded recipient");
                assert!(
                    replacement.changes.borrow().is_empty(),
                    "new recipient had no Will"
                );
                if matches!(action, DuringWill::Retire) {
                    assert_eq!(client.document_len(), None);
                }
            }
        }
    }
}

#[test]
fn a_will_failure_still_publishes_and_preserves_its_first_failure() {
    let store = InMemoryTextStore::new("ab");
    let other = InMemoryTextStore::new("next");
    let (client, context) = serving(&store);
    *context.on_will_change.borrow_mut() = Some(Box::new({
        let client = Rc::clone(&client);
        let other = as_store(&other);
        move || {
            client.focus_store(Some(Rc::clone(&other)));
            panic!("first Will failure");
        }
    }));
    *context.on_change.borrow_mut() = Some(Box::new(|| panic!("later Did failure")));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.app_replace(span(0, 1), "XYZ");
    }))
    .expect_err("the Will failure propagates after publication");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"first Will failure"));
    assert_eq!(store.text(), "XYZb");
    assert_eq!(client.document_len(), Some(Utf16Offset::new(4)));
    client.insert_text("!".to_owned(), None);
    assert!(other.text().contains('!'), "the next operation works");
}

#[test]
fn an_application_commit_with_identical_text_still_ends_marked_text() {
    let store = InMemoryTextStore::new("");
    let (client, context) = serving(&store);
    client.set_marked_text("x".to_owned(), span(1, 1), None);
    assert!(!client.reports_key_release());
    store.app_replace(span(0, 1), "x");
    assert_eq!(store.text(), "x");
    assert_eq!(store.composition(), None);
    assert_eq!(
        *context.will_changes.borrow(),
        [DocumentChange::Composition, DocumentChange::Selection]
    );
    assert_eq!(
        *context.changes.borrow(),
        [DocumentChange::Composition, DocumentChange::Selection]
    );
    assert_eq!(context.discards.get(), 1);
    assert!(client.reports_key_release());
}
