//! The decidable half of the Android backend's input method
//! (`text::android::ime`, ADR-0349): how `FluiView`'s mirror
//! reports become ordered edits of the field's store, how the view takes a
//! snapshot that raced the input method's edits, when the input method
//! restarts, and what an editor action, a stylus press and a tap become. The
//! JNI side runs on a device.

use std::rc::Rc;

use super::{
    Change, CharacterBounds, EditorReport, FLAG_HAS_VISIBLE_REGION, IME_ACTION_DONE,
    IME_ACTION_NEXT, IME_ACTION_PREVIOUS, IME_ACTION_SEARCH, IME_FLAG_NO_ENTER_ACTION, ImeEdit,
    Install, Ledger, MAX_CHARACTER_BOUNDS, Mirror, Replayed, Snapshot, TYPE_CLASS_NUMBER,
    TYPE_CLASS_PHONE, TYPE_CLASS_TEXT, TYPE_TEXT_FLAG_MULTI_LINE,
    TYPE_TEXT_VARIATION_EMAIL_ADDRESS, TYPE_TEXT_VARIATION_PASSWORD, TYPE_TEXT_VARIATION_URI,
    character_bounds, cursor_anchor, editor_action_key, editor_info, starts_handwriting,
};
use crate::text::ime::PROTECTED_UNIT;
use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::keyboard::NamedKey;
use flui_platform::text_store::{
    CommitGate, Composition, LockGrant, LockTiming, Selection, TextInputPurpose, TextStore,
    TextStoreStatus, Utf16Offset, Utf16Range,
};
use flui_testing::text_store_kit::InMemoryTextStore;

fn offset(units: usize) -> Utf16Offset {
    Utf16Offset::new(units)
}

fn range(start: usize, end: usize) -> Utf16Range {
    Utf16Range::try_new(offset(start), offset(end)).expect("ordered")
}

/// What the view reports: its text, a caret or selection, and a composing
/// span (`None` for none).
fn state(text: &str, selection: (i32, i32), composition: Option<(i32, i32)>) -> EditorReport {
    let (composition_start, composition_end) = composition.unwrap_or((-1, -1));
    EditorReport {
        text: text.to_owned(),
        selection_start: selection.0,
        selection_end: selection.1,
        composition_start,
        composition_end,
    }
}

/// The snapshot the owner's `ledger` publishes of `store` for `change`.
fn publish(ledger: &Rc<Ledger>, store: &InMemoryTextStore, change: Change) -> Snapshot {
    let read = Rc::new(std::cell::RefCell::new(None));
    let (out, ledger) = (Rc::clone(&read), Rc::clone(ledger));
    let _granted = store
        .request_lock(
            LockGrant::read(move |session| {
                *out.borrow_mut() = Some(ledger.publish(session, change));
            }),
            LockTiming::Sync,
        )
        .expect("a free store grants a sync read");
    read.take().expect("the grant ran")
}

/// A field focused on a fresh owner and view: the owner's ledger, and the
/// view's mirror holding the focus snapshot.
fn focus(store: &InMemoryTextStore) -> (Rc<Ledger>, Mirror) {
    let ledger = Rc::new(Ledger::default());
    let mut mirror = Mirror::default();
    let focused = publish(&ledger, store, Change::Focus);
    assert_eq!(
        mirror.install(&focused),
        Install::Replace { restart: true },
        "a focus starts the input method"
    );
    (ledger, mirror)
}

/// Replay `edit` the way the host does: an asynchronous read-write lock, in
/// the order the view reported.
fn replay(
    store: &InMemoryTextStore,
    ledger: &Rc<Ledger>,
    edit: ImeEdit,
) -> Rc<std::cell::Cell<Option<Replayed>>> {
    let outcome = Rc::new(std::cell::Cell::new(None));
    let (out, ledger) = (Rc::clone(&outcome), Rc::clone(ledger));
    let _queued = store
        .request_lock(
            LockGrant::read_write(move |session| {
                out.set(Some(ledger.replay(&edit, session)));
            }),
            LockTiming::Async,
        )
        .expect("the store queues or grants an async edit");
    outcome
}

/// A composition typed key by key and committed with a space reaches the
/// store as the same text, caret and (finally no) composition the view shows,
/// one ordered edit per report, each replayed in order once the frame
/// transaction that held them ends.
fn a_composition_typed_on_the_keyboard_reaches_the_store_in_order() {
    let store = InMemoryTextStore::new("say ");
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    let (ledger, mut mirror) = focus(&store);
    gate.set_open(false);
    let reports = [
        state("say h", (5, 5), Some((4, 5))),
        state("say ho", (6, 6), Some((4, 6))),
        state("say hop", (7, 7), Some((4, 7))),
        state("say hope ", (9, 9), None),
    ];
    let mut outcomes = Vec::new();
    for report in &reports {
        let edit = mirror.report(report).expect("the view holds a snapshot");
        outcomes.push(replay(&store, &ledger, edit));
    }
    assert_eq!(
        store.text(),
        "say ",
        "nothing commits inside the transaction"
    );
    gate.set_open(true);
    store.run_deferred_grants();
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.get() == Some(Replayed::Applied))
    );
    assert_eq!(store.text(), "say hope ");
    assert_eq!(store.selection(), Selection::collapsed(offset(9)));
    assert_eq!(store.composition(), None);
}

/// A report between two compositions carries the composing span, so the
/// store composes exactly what the view composes.
fn the_store_composes_what_the_view_composes() {
    let store = InMemoryTextStore::new("");
    let (ledger, mut mirror) = focus(&store);
    let edit = mirror
        .report(&state("東京", (2, 2), Some((0, 2))))
        .expect("current");
    assert_eq!(edit.replaced, Some((range(0, 0), "東京".to_owned())));
    assert!(edit.composes());
    let outcome = replay(&store, &ledger, edit);
    assert_eq!(outcome.get(), Some(Replayed::Applied));
    assert_eq!(
        store.composition().map(|composition| composition.range),
        Some(range(0, 2))
    );
}

/// The application moved the caret while the keyboard typed: the owner
/// published a selection-only snapshot that had not seen the character the
/// view already reported. The view keeps the keyboard's text and caret
/// instead of taking the snapshot, and the owner still replays the edit, so
/// the character reaches the field and the keyboard, the view and the store
/// hold one text.
fn a_selection_snapshot_that_raced_an_edit_keeps_it() {
    let store = InMemoryTextStore::new("abc");
    let (ledger, mut mirror) = focus(&store);
    store.app_select(Selection::collapsed(offset(1)));
    let moved = publish(&ledger, &store, Change::Selection);
    let typed = mirror
        .report(&state("abcd", (4, 4), None))
        .expect("the view reports before it takes the newer snapshot");
    assert!(typed.base < moved.publication, "publications only grow");
    assert_eq!(mirror.install(&moved), Install::Keep);
    assert_eq!(
        replay(&store, &ledger, typed).get(),
        Some(Replayed::Applied)
    );
    assert_eq!(store.text(), "abcd");
    assert_eq!(store.selection(), Selection::collapsed(offset(4)));
    // The view went on from its own text, which the store now holds too.
    let next = mirror
        .report(&state("abcde", (5, 5), None))
        .expect("current");
    assert_eq!(next.base, moved.publication);
    assert_eq!(next.replaced, Some((range(4, 4), "e".to_owned())));
    assert_eq!(replay(&store, &ledger, next).get(), Some(Replayed::Applied));
    assert_eq!(store.text(), "abcde");
}

/// A selection-only snapshot that saw every report the view made is taken
/// as it is: only the selection moves, and the input method is told, not
/// restarted.
fn a_selection_snapshot_that_saw_every_edit_is_taken() {
    let store = InMemoryTextStore::new("abc");
    let (ledger, mut mirror) = focus(&store);
    let typed = mirror
        .report(&state("abcd", (4, 4), None))
        .expect("current");
    assert_eq!(
        replay(&store, &ledger, typed).get(),
        Some(Replayed::Applied)
    );
    store.app_select(Selection::collapsed(offset(0)));
    let moved = publish(&ledger, &store, Change::Selection);
    assert_eq!(mirror.install(&moved), Install::Replace { restart: false });
}

/// The application rewrote the text while the keyboard typed: its text
/// wins. The view takes the snapshot and restarts the input method, whose
/// copy holds the character, and the owner drops the edit made before it,
/// so the store keeps the application's text.
fn a_text_snapshot_that_raced_an_edit_wins_and_restarts() {
    let store = InMemoryTextStore::new("abc");
    let (ledger, mut mirror) = focus(&store);
    store.app_replace(range(0, 3), "xyz");
    let rewritten = publish(&ledger, &store, Change::Text);
    let typed = mirror
        .report(&state("abcd", (4, 4), None))
        .expect("the view reports before it takes the newer snapshot");
    assert_eq!(
        mirror.install(&rewritten),
        Install::Replace { restart: true }
    );
    assert_eq!(replay(&store, &ledger, typed).get(), Some(Replayed::Stale));
    assert!(
        !Replayed::Stale.restarts_the_view(),
        "the view restarted when it took the snapshot"
    );
    assert_eq!(store.text(), "xyz");
    let edit = mirror
        .report(&state("xyz!", (4, 4), None))
        .expect("current");
    assert_eq!(replay(&store, &ledger, edit).get(), Some(Replayed::Applied));
    assert_eq!(store.text(), "xyz!");
}

/// A report the view made before it took the application's text is on text
/// it no longer holds: a selection snapshot that comes next, which the owner
/// read before it dropped that report, is taken, not kept off as a race.
fn a_report_on_replaced_text_races_no_later_snapshot() {
    let store = InMemoryTextStore::new("abc");
    let (ledger, mut mirror) = focus(&store);
    store.app_replace(range(0, 3), "xyz");
    let rewritten = publish(&ledger, &store, Change::Text);
    let typed = mirror
        .report(&state("abcd", (4, 4), None))
        .expect("the view reports before it takes the newer snapshot");
    assert_eq!(
        mirror.install(&rewritten),
        Install::Replace { restart: true }
    );
    store.app_select(Selection::collapsed(offset(1)));
    let moved = publish(&ledger, &store, Change::Selection);
    assert!(
        moved.seen < Some(typed.report),
        "the owner has not replayed it"
    );
    assert_eq!(
        mirror.install(&moved),
        Install::Replace { restart: false },
        "the view holds the application's text, so the caret it moved applies"
    );
    assert_eq!(replay(&store, &ledger, typed).get(), Some(Replayed::Stale));
    let next = mirror.report(&state("xyz", (1, 1), None)).expect("current");
    assert_eq!(next.replaced, None, "the view and the store hold one text");
}

/// The application ended the composition while the keyboard went on
/// composing: the store's observer reports a selection change, but it
/// changed what the input method composes, so it is published as the
/// application's text and wins the race like one: the view restarts the input
/// method on it, and the report made before it is dropped instead of putting
/// the keyboard's composition back.
fn an_application_composition_end_wins_a_race() {
    let store = InMemoryTextStore::new("");
    let (ledger, mut mirror) = focus(&store);
    let composing = mirror
        .report(&state("か", (1, 1), Some((0, 1))))
        .expect("current");
    assert_eq!(
        replay(&store, &ledger, composing).get(),
        Some(Replayed::Applied)
    );
    let _ended = store
        .request_lock(
            LockGrant::read_write(|session| {
                session.set_composition(None).expect("a composition ends");
            }),
            LockTiming::Sync,
        )
        .expect("a free store grants a sync edit");
    let ended = publish(&ledger, &store, Change::Selection);
    assert_eq!(
        ended.change,
        Change::Text,
        "a composition change is no move"
    );
    let typed = mirror
        .report(&state("かな", (2, 2), Some((0, 2))))
        .expect("the view reports before it takes the snapshot");
    assert_eq!(mirror.install(&ended), Install::Replace { restart: true });
    assert_eq!(replay(&store, &ledger, typed).get(), Some(Replayed::Stale));
    assert_eq!(store.composition(), None);
    // A plain caret move with the composition as the owner last saw it
    // stays a selection change.
    store.app_select(Selection::collapsed(offset(0)));
    assert_eq!(
        publish(&ledger, &store, Change::Selection).change,
        Change::Selection
    );
}

/// The view decides how it takes a snapshot before its editor takes it, and
/// the mirror follows only once the editor did: a snapshot whose call to the
/// view failed leaves the reports diffed against the text the view still
/// holds.
fn a_snapshot_the_view_did_not_take_leaves_the_mirror() {
    let store = InMemoryTextStore::new("abc");
    let (ledger, mut mirror) = focus(&store);
    store.app_replace(range(0, 3), "xyz");
    let rewritten = publish(&ledger, &store, Change::Text);
    assert_eq!(
        mirror.decide(&rewritten),
        Install::Replace { restart: false }
    );
    let typed = mirror
        .report(&state("abcd", (4, 4), None))
        .expect("current");
    assert_eq!(
        typed.replaced,
        Some((range(3, 3), "d".to_owned())),
        "diffed against the text the view holds"
    );
    assert!(typed.base < rewritten.publication);
}

/// An application edit outside the input method's composition, or with no
/// composition at all (a formatter rewriting the text on every key), only
/// tells the input method, as does one that only moves the composition; one
/// that ends or rewrites the composed text restarts it.
fn the_input_method_restarts_only_when_its_composition_is_touched() {
    let store = InMemoryTextStore::new("ab");
    let (ledger, mut mirror) = focus(&store);
    store.app_replace(range(0, 2), "AB");
    let formatted = publish(&ledger, &store, Change::Text);
    assert_eq!(
        mirror.install(&formatted),
        Install::Replace { restart: false },
        "no composition: a text change is told"
    );
    let composing = mirror
        .report(&state("ABか", (3, 3), Some((2, 3))))
        .expect("current");
    assert_eq!(
        replay(&store, &ledger, composing).get(),
        Some(Replayed::Applied)
    );
    store.app_replace(range(0, 1), "Z");
    let outside = publish(&ledger, &store, Change::Text);
    assert_eq!(
        mirror.install(&outside),
        Install::Replace { restart: false },
        "an edit before the composition leaves it alone"
    );
    store.app_replace(range(0, 0), "-");
    let shifted = publish(&ledger, &store, Change::Text);
    assert_eq!(shifted.composition, Some(range(3, 4)));
    assert_eq!(
        mirror.install(&shifted),
        Install::Replace { restart: false },
        "a composition moved by text inserted before it is told its new bounds"
    );
    store.app_replace(range(3, 4), "カ");
    let inside = publish(&ledger, &store, Change::Text);
    assert_eq!(
        mirror.install(&inside),
        Install::Replace { restart: true },
        "an edit of the composed text restarts"
    );
}

/// The owner ended the input method's composition (a commit the field asked
/// for, an editor action): the input method restarts on the committed text.
fn a_composition_the_owner_ends_restarts_the_input_method() {
    let store = InMemoryTextStore::new("");
    let (ledger, mut mirror) = focus(&store);
    let composing = mirror
        .report(&state("か", (1, 1), Some((0, 1))))
        .expect("current");
    assert_eq!(
        replay(&store, &ledger, composing).get(),
        Some(Replayed::Applied)
    );
    let ended = Rc::new(std::cell::RefCell::new(None));
    let (out, publisher) = (Rc::clone(&ended), Rc::clone(&ledger));
    let _granted = store
        .request_lock(
            LockGrant::read_write(move |session| {
                session.set_composition(None).expect("a composition ends");
                *out.borrow_mut() = Some(publisher.publish(&*session, Change::Text));
            }),
            LockTiming::Sync,
        )
        .expect("a free store grants a sync edit");
    let ended = ended.take().expect("the grant ran");
    assert_eq!(ended.composition, None);
    assert_eq!(mirror.install(&ended), Install::Replace { restart: true });
}

/// Focus moved to another field: an edit the view made for the field it
/// left is dropped, not replayed into the new one.
fn an_edit_for_the_field_focus_left_is_dropped() {
    let left = InMemoryTextStore::new("left");
    let (ledger, mut mirror) = focus(&left);
    let typed = mirror
        .report(&state("left!", (5, 5), None))
        .expect("current");
    let taken = InMemoryTextStore::new("taken");
    let focused = publish(&ledger, &taken, Change::Focus);
    assert_eq!(mirror.install(&focused), Install::Replace { restart: true });
    assert_eq!(replay(&taken, &ledger, typed).get(), Some(Replayed::Stale));
    assert_eq!(taken.text(), "taken");
}

/// A step the store refuses tells the owner the view no longer mirrors it,
/// and only that asks the owner to restart the view.
fn a_refused_step_asks_for_a_new_snapshot() {
    let store = InMemoryTextStore::new("ab");
    let (ledger, mut mirror) = focus(&store);
    let mut edit = mirror.report(&state("ab", (1, 1), None)).expect("current");
    edit.replaced = Some((range(5, 6), "x".to_owned()));
    assert_eq!(replay(&store, &ledger, edit).get(), Some(Replayed::Refused));
    assert!(Replayed::Refused.restarts_the_view());
    assert!(!Replayed::Applied.restarts_the_view());
    // The owner publishes the store again, which the view takes over its own
    // text, restarting the input method, even though it saw every report.
    let resync = publish(&ledger, &store, Change::Resync);
    assert_eq!(mirror.install(&resync), Install::Replace { restart: true });
}

/// A view that holds no snapshot yet has nothing to diff a report against.
fn a_report_before_any_snapshot_is_no_edit() {
    let mut mirror = Mirror::default();
    assert_eq!(mirror.report(&state("a", (1, 1), None)), None);
}

/// Offsets the view reports inside a surrogate pair, or past the text, land
/// on the scalar boundary before them, which the store accepts.
fn offsets_never_split_a_surrogate_pair() {
    let store = InMemoryTextStore::new("");
    let (ledger, mut mirror) = focus(&store);
    // "a😀b": the emoji is two code units, 1..3.
    let edit = mirror
        .report(&state("a😀b", (2, 9), Some((2, 3))))
        .expect("current");
    assert_eq!(
        edit.selection,
        Selection {
            anchor: offset(1),
            active: offset(4),
        }
    );
    assert_eq!(edit.composition, Some(range(1, 3)));
    assert_eq!(replay(&store, &ledger, edit).get(), Some(Replayed::Applied));
    // A delete of the emoji is one replacement of both units.
    let edit = mirror.report(&state("ab", (1, 1), None)).expect("current");
    assert_eq!(edit.replaced, Some((range(1, 3), String::new())));
    assert_eq!(replay(&store, &ledger, edit).get(), Some(Replayed::Applied));
    assert_eq!(store.text(), "ab");
}

/// A report that changes nothing but the caret is an edit with no
/// replacement; one with no selection span puts the caret at the end.
fn a_caret_move_replaces_nothing() {
    let store = InMemoryTextStore::new("hello");
    let (_ledger, mut mirror) = focus(&store);
    let moved = mirror
        .report(&state("hello", (1, 1), None))
        .expect("current");
    assert_eq!(moved.replaced, None);
    assert_eq!(moved.selection, Selection::collapsed(offset(1)));
    let unset = mirror
        .report(&state("hello", (-1, -1), None))
        .expect("current");
    assert_eq!(unset.selection, Selection::collapsed(offset(5)));
}

/// A protected field's snapshot keeps its length in bullets, so the input
/// method's offsets mean what they mean in the field, and its typing lands
/// in the real text.
fn a_protected_field_is_mirrored_as_bullets() {
    let store = InMemoryTextStore::new("pw");
    store.set_protected(true);
    let (ledger, mut mirror) = focus(&store);
    let bullets = PROTECTED_UNIT.to_string().repeat(2);
    let typed = format!("{bullets}!");
    let edit = mirror
        .report(&state(&typed, (3, 3), None))
        .expect("current");
    assert_eq!(edit.replaced, Some((range(2, 2), "!".to_owned())));
    assert_eq!(replay(&store, &ledger, edit).get(), Some(Replayed::Applied));
    assert_eq!(store.text(), "pw!");
}

/// The snapshot's offsets are what `FluiView` takes, `-1` for no composition.
fn a_snapshot_hands_the_view_its_offsets() {
    let store = InMemoryTextStore::new("abcd");
    let (ledger, mut mirror) = focus(&store);
    assert_eq!(
        publish(&ledger, &store, Change::Selection).java_offsets(),
        [4, 4, -1, -1]
    );
    let edit = mirror
        .report(&state("abcd", (3, 1), Some((1, 3))))
        .expect("current");
    assert_eq!(replay(&store, &ledger, edit).get(), Some(Replayed::Applied));
    assert_eq!(
        publish(&ledger, &store, Change::Selection).java_offsets(),
        [3, 1, 1, 3]
    );
}

fn the_editor_info_follows_the_store_status() {
    let single = TextStoreStatus::EDITABLE_SINGLE_LINE;
    assert_eq!(editor_info(single), (TYPE_CLASS_TEXT, IME_ACTION_DONE));
    let (password, options) = editor_info(single.with_protected(true));
    assert_eq!(password, TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_PASSWORD);
    assert_eq!(options & 0xff, IME_ACTION_DONE);
    assert_eq!(
        (password | options) & (TYPE_TEXT_FLAG_MULTI_LINE | IME_FLAG_NO_ENTER_ACTION),
        0,
        "a single-line field is not multi-line"
    );
}

/// The purpose a field was built with picks the keyboard; a protected field
/// stays a password field whatever its purpose, and a number or phone field
/// has no multi-line flag, which belongs to the text class.
fn the_purpose_picks_the_keyboard() {
    let single = TextStoreStatus::EDITABLE_SINGLE_LINE;
    let with = |purpose| editor_info(single.with_purpose(purpose));
    assert_eq!(
        with(TextInputPurpose::Email),
        (
            TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            IME_ACTION_DONE
        )
    );
    assert_eq!(
        with(TextInputPurpose::Url),
        (TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_URI, IME_ACTION_DONE)
    );
    assert_eq!(
        with(TextInputPurpose::Number),
        (TYPE_CLASS_NUMBER, IME_ACTION_DONE)
    );
    assert_eq!(
        with(TextInputPurpose::Phone),
        (TYPE_CLASS_PHONE, IME_ACTION_DONE)
    );
    assert_eq!(
        with(TextInputPurpose::Search),
        (TYPE_CLASS_TEXT, IME_ACTION_SEARCH)
    );
    assert_eq!(with(TextInputPurpose::Text), editor_info(single));
    let (protected, _) = editor_info(
        single
            .with_purpose(TextInputPurpose::Email)
            .with_protected(true),
    );
    assert_eq!(
        protected,
        TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_PASSWORD,
        "protection wins over the purpose"
    );
    let mut multi_line_number = single.with_purpose(TextInputPurpose::Number);
    multi_line_number.single_line = false;
    let (input_type, options) = editor_info(multi_line_number);
    assert_eq!(
        input_type, TYPE_CLASS_NUMBER,
        "no multi-line flag outside the text class"
    );
    assert_eq!(options & IME_FLAG_NO_ENTER_ACTION, 0);
}

fn editor_actions_arrive_as_keys() {
    assert_eq!(editor_action_key(IME_ACTION_DONE), (NamedKey::Enter, false));
    assert_eq!(editor_action_key(IME_ACTION_NEXT), (NamedKey::Tab, false));
    assert_eq!(
        editor_action_key(IME_ACTION_PREVIOUS),
        (NamedKey::Tab, true)
    );
    assert_eq!(editor_action_key(3), (NamedKey::Enter, false), "search");
}

fn field() -> Bounds<f64> {
    Bounds::new(Point::new(10.0, 20.0), Size::new(100.0, 30.0))
}

/// Handwriting starts for a pen tip, inside the focused field.
fn handwriting_starts_for_a_pen_inside_the_focused_field() {
    let inside = Point::new(50.0, 30.0);
    assert!(starts_handwriting(true, Some(field()), inside));
    assert!(
        !starts_handwriting(false, Some(field()), inside),
        "a finger"
    );
    assert!(!starts_handwriting(true, None, inside), "no field");
    assert!(
        !starts_handwriting(true, Some(field()), Point::new(5.0, 30.0)),
        "outside the field"
    );
}

/// The cursor goes to the input method in the view's physical pixels, and
/// never as a non-finite coordinate.
fn the_cursor_anchor_is_in_physical_pixels() {
    let caret = Bounds::new(Point::new(12.0, 22.0), Size::new(0.0, 20.0));
    assert_eq!(
        cursor_anchor(caret, Some(field()), 2.0),
        Some([24.0, 44.0, 84.0, 20.0, 40.0, 220.0, 100.0])
    );
    assert_eq!(
        cursor_anchor(caret, None, 2.0).map(|anchor| anchor[5] < anchor[3]),
        Some(true),
        "no field bounds is a negative width"
    );
    let huge = Bounds::new(Point::new(f64::MAX, 0.0), Size::new(0.0, 1.0));
    assert_eq!(cursor_anchor(huge, None, 2.0), None);
}

/// The boxes of the characters `store` composes over `composing`, as the host
/// sends them at a scale of 2.
fn composing_boxes(
    store: &InMemoryTextStore,
    composing: Option<Utf16Range>,
) -> Option<CharacterBounds> {
    let _composed = store
        .request_lock(
            LockGrant::read_write(move |session| {
                session
                    .set_composition(composing.map(|range| Composition {
                        range,
                        hides_caret: false,
                    }))
                    .expect("the composition lies in the text");
            }),
            LockTiming::Sync,
        )
        .expect("a free store grants a sync edit");
    let found = Rc::new(std::cell::RefCell::new(None));
    let out = Rc::clone(&found);
    let _read = store
        .request_lock(
            LockGrant::read(move |session| *out.borrow_mut() = character_bounds(session, 2.0)),
            LockTiming::Sync,
        )
        .expect("a free store grants a sync read");
    found.take()
}

/// The composing text's characters go to the input method one box per UTF-16
/// index, in physical pixels: both halves of a surrogate pair take the pair's
/// box, the boxes carry the text they were measured on, nothing goes without
/// a composition, and a long composition sends its
/// first boxes only.
fn the_composing_characters_have_their_boxes() {
    let store = InMemoryTextStore::new("xa😀b");
    let boxes = composing_boxes(&store, Some(range(1, 5))).expect("a composition has boxes");
    assert_eq!(boxes.start, 1);
    assert_eq!(
        boxes.text, "a😀b",
        "the boxes name the text they were measured on"
    );
    assert_eq!(
        boxes.rects,
        [
            20.0, 0.0, 40.0, 40.0, // a
            40.0, 0.0, 60.0, 40.0, // 😀, its first half
            40.0, 0.0, 60.0, 40.0, // and its second
            60.0, 0.0, 80.0, 40.0, // b
        ]
    );
    assert_eq!(boxes.flags, [FLAG_HAS_VISIBLE_REGION; 4]);
    assert_eq!(composing_boxes(&store, None), None);

    let long = InMemoryTextStore::new("a".repeat(MAX_CHARACTER_BOUNDS + 10));
    let boxes = composing_boxes(&long, Some(range(0, MAX_CHARACTER_BOUNDS + 10)))
        .expect("a composition has boxes");
    assert_eq!(boxes.flags.len(), MAX_CHARACTER_BOUNDS);
    assert_eq!(boxes.rects.len(), MAX_CHARACTER_BOUNDS * 4);
}

#[test]
fn android_ime_adapter() {
    crate::table_test::run_table(
        "android_ime_adapter",
        &[
            (
                "a_composition_typed_on_the_keyboard_reaches_the_store_in_order",
                a_composition_typed_on_the_keyboard_reaches_the_store_in_order,
            ),
            (
                "the_store_composes_what_the_view_composes",
                the_store_composes_what_the_view_composes,
            ),
            (
                "a_selection_snapshot_that_raced_an_edit_keeps_it",
                a_selection_snapshot_that_raced_an_edit_keeps_it,
            ),
            (
                "a_selection_snapshot_that_saw_every_edit_is_taken",
                a_selection_snapshot_that_saw_every_edit_is_taken,
            ),
            (
                "a_text_snapshot_that_raced_an_edit_wins_and_restarts",
                a_text_snapshot_that_raced_an_edit_wins_and_restarts,
            ),
            (
                "a_report_on_replaced_text_races_no_later_snapshot",
                a_report_on_replaced_text_races_no_later_snapshot,
            ),
            (
                "an_application_composition_end_wins_a_race",
                an_application_composition_end_wins_a_race,
            ),
            (
                "a_snapshot_the_view_did_not_take_leaves_the_mirror",
                a_snapshot_the_view_did_not_take_leaves_the_mirror,
            ),
            (
                "the_input_method_restarts_only_when_its_composition_is_touched",
                the_input_method_restarts_only_when_its_composition_is_touched,
            ),
            (
                "a_composition_the_owner_ends_restarts_the_input_method",
                a_composition_the_owner_ends_restarts_the_input_method,
            ),
            (
                "an_edit_for_the_field_focus_left_is_dropped",
                an_edit_for_the_field_focus_left_is_dropped,
            ),
            (
                "a_report_before_any_snapshot_is_no_edit",
                a_report_before_any_snapshot_is_no_edit,
            ),
            (
                "a_refused_step_asks_for_a_new_snapshot",
                a_refused_step_asks_for_a_new_snapshot,
            ),
            (
                "offsets_never_split_a_surrogate_pair",
                offsets_never_split_a_surrogate_pair,
            ),
            (
                "a_caret_move_replaces_nothing",
                a_caret_move_replaces_nothing,
            ),
            (
                "a_protected_field_is_mirrored_as_bullets",
                a_protected_field_is_mirrored_as_bullets,
            ),
            (
                "a_snapshot_hands_the_view_its_offsets",
                a_snapshot_hands_the_view_its_offsets,
            ),
            (
                "the_purpose_picks_the_keyboard",
                the_purpose_picks_the_keyboard,
            ),
            (
                "the_editor_info_follows_the_store_status",
                the_editor_info_follows_the_store_status,
            ),
            (
                "editor_actions_arrive_as_keys",
                editor_actions_arrive_as_keys,
            ),
            (
                "handwriting_starts_for_a_pen_inside_the_focused_field",
                handwriting_starts_for_a_pen_inside_the_focused_field,
            ),
            (
                "the_cursor_anchor_is_in_physical_pixels",
                the_cursor_anchor_is_in_physical_pixels,
            ),
            (
                "the_composing_characters_have_their_boxes",
                the_composing_characters_have_their_boxes,
            ),
        ],
    );
}
