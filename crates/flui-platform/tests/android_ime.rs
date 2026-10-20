//! The decidable half of the Android backend's input method
//! (`shared::android_ime`, ADR-0247): how `FluiView`'s mirror reports become
//! ordered edits of the field's store, which edits a newer owner snapshot
//! drops, and what a key, an editor action and a stylus press become. The JNI
//! side runs on a device (`cargo xtask device android-input`).

use std::rc::Rc;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::shared::android_ime::{
    EditorState, IME_ACTION_DONE, IME_ACTION_NEXT, IME_ACTION_PREVIOUS, IME_FLAG_NO_ENTER_ACTION,
    ImeEdit, Mirror, PROTECTED_UNIT, Replayed, Snapshot, TYPE_CLASS_TEXT,
    TYPE_TEXT_FLAG_MULTI_LINE, TYPE_TEXT_VARIATION_PASSWORD, cursor_anchor, editor_action_key,
    editor_info, key_composition, replay_current, starts_handwriting,
};
use flui_platform_api::keyboard::{ImeComposition, NamedKey};
use flui_platform_api::text_store::{
    CommitGate, InMemoryTextStore, LockGrant, LockTiming, Selection, TextStore, TextStoreStatus,
    Utf16Offset, Utf16Range,
};

fn offset(units: usize) -> Utf16Offset {
    Utf16Offset::new(units)
}

fn range(start: usize, end: usize) -> Utf16Range {
    Utf16Range::new(offset(start), offset(end)).expect("ordered")
}

/// What the view reports: its text, a caret or selection, and a composing
/// span (`None` for none).
fn state(text: &str, selection: (i32, i32), composition: Option<(i32, i32)>) -> EditorState {
    let (composition_start, composition_end) = composition.unwrap_or((-1, -1));
    EditorState {
        text: text.to_owned(),
        selection_start: selection.0,
        selection_end: selection.1,
        composition_start,
        composition_end,
    }
}

/// The snapshot `store` publishes as `generation`.
fn snapshot(store: &InMemoryTextStore, generation: u32) -> Snapshot {
    let read = Rc::new(std::cell::RefCell::new(None));
    let out = Rc::clone(&read);
    let _granted = store
        .request_lock(
            LockGrant::read(move |session| {
                *out.borrow_mut() = Some(Snapshot::read(session, generation));
            }),
            LockTiming::Sync,
        )
        .expect("a free store grants a sync read");
    read.take().expect("the grant ran")
}

/// Replay `edit` the way the host does: an asynchronous read-write lock,
/// checked against the generation the owner published last.
fn replay(
    store: &InMemoryTextStore,
    edit: ImeEdit,
    published: u32,
) -> Rc<std::cell::Cell<Option<Replayed>>> {
    let outcome = Rc::new(std::cell::Cell::new(None));
    let out = Rc::clone(&outcome);
    let _queued = store
        .request_lock(
            LockGrant::read_write(move |session| {
                out.set(Some(replay_current(&edit, published, session)));
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
    let mut mirror = Mirror::default();
    mirror.install(&snapshot(&store, 1));
    gate.set_open(false);
    let reports = [
        state("say h", (5, 5), Some((4, 5))),
        state("say ho", (6, 6), Some((4, 6))),
        state("say hop", (7, 7), Some((4, 7))),
        state("say hope ", (9, 9), None),
    ];
    let mut outcomes = Vec::new();
    for report in &reports {
        let edit = mirror
            .report(1, report)
            .expect("a report on the current snapshot");
        outcomes.push(replay(&store, edit, 1));
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
    let mut mirror = Mirror::default();
    mirror.install(&snapshot(&store, 1));
    let edit = mirror
        .report(1, &state("東京", (2, 2), Some((0, 2))))
        .expect("current");
    assert_eq!(edit.replaced, Some((range(0, 0), "東京".to_owned())));
    assert!(edit.composes());
    let outcome = replay(&store, edit, 1);
    assert_eq!(outcome.get(), Some(Replayed::Applied));
    assert_eq!(
        store.composition().map(|composition| composition.range),
        Some(range(0, 2))
    );
}

/// The owner published a newer snapshot (the application edited the field):
/// a report the view made before it installed that snapshot is no edit at
/// all, and an edit queued against the older one is dropped when it runs, so
/// the store keeps the application's text and the newer snapshot resets the
/// view.
fn an_edit_made_on_a_superseded_snapshot_is_dropped() {
    let store = InMemoryTextStore::new("abc");
    let mut mirror = Mirror::default();
    mirror.install(&snapshot(&store, 1));
    let queued = mirror
        .report(1, &state("abcd", (4, 4), None))
        .expect("current");
    store.app_replace(range(0, 3), "xyz");
    let newer = snapshot(&store, 2);
    mirror.install(&newer);
    assert_eq!(
        mirror.report(1, &state("abcde", (5, 5), None)),
        None,
        "a report on the older snapshot after the newer one was published"
    );
    let outcome = replay(&store, queued, 2);
    assert_eq!(outcome.get(), Some(Replayed::Stale));
    // The newer snapshot reached the view as a selection-only update, so the
    // dropped edit's character is still in the input method's text: the view
    // must restart on the store's.
    assert!(Replayed::Stale.restarts_the_view());
    assert!(Replayed::Refused.restarts_the_view());
    assert!(!Replayed::Applied.restarts_the_view());
    assert_eq!(store.text(), "xyz");
    let edit = mirror
        .report(2, &state("xyz!", (4, 4), None))
        .expect("current");
    assert_eq!(replay(&store, edit, 2).get(), Some(Replayed::Applied));
    assert_eq!(store.text(), "xyz!");
}

/// A step the store refuses tells the owner the view no longer mirrors it.
fn a_refused_step_asks_for_a_new_snapshot() {
    let store = InMemoryTextStore::new("ab");
    let edit = ImeEdit {
        base: 1,
        replaced: Some((range(5, 6), "x".to_owned())),
        selection: Selection::collapsed(offset(1)),
        composition: None,
    };
    assert_eq!(replay(&store, edit, 1).get(), Some(Replayed::Refused));
}

/// Offsets the view reports inside a surrogate pair, or past the text, land
/// on the scalar boundary before them, which the store accepts.
fn offsets_never_split_a_surrogate_pair() {
    let store = InMemoryTextStore::new("");
    let mut mirror = Mirror::default();
    mirror.install(&snapshot(&store, 1));
    // "a😀b": the emoji is two code units, 1..3.
    let edit = mirror
        .report(1, &state("a😀b", (2, 9), Some((2, 3))))
        .expect("current");
    assert_eq!(
        edit.selection,
        Selection {
            anchor: offset(1),
            active: offset(4),
        }
    );
    assert_eq!(edit.composition, Some(range(1, 3)));
    assert_eq!(replay(&store, edit, 1).get(), Some(Replayed::Applied));
    // A delete of the emoji is one replacement of both units.
    let edit = mirror
        .report(1, &state("ab", (1, 1), None))
        .expect("current");
    assert_eq!(edit.replaced, Some((range(1, 3), String::new())));
    assert_eq!(replay(&store, edit, 1).get(), Some(Replayed::Applied));
    assert_eq!(store.text(), "ab");
}

/// A report that changes nothing but the caret is an edit with no
/// replacement; one with no selection span puts the caret at the end.
fn a_caret_move_replaces_nothing() {
    let store = InMemoryTextStore::new("hello");
    let mut mirror = Mirror::default();
    mirror.install(&snapshot(&store, 3));
    let moved = mirror
        .report(3, &state("hello", (1, 1), None))
        .expect("current");
    assert_eq!(moved.replaced, None);
    assert_eq!(moved.selection, Selection::collapsed(offset(1)));
    let unset = mirror
        .report(3, &state("hello", (-1, -1), None))
        .expect("current");
    assert_eq!(unset.selection, Selection::collapsed(offset(5)));
}

/// A protected field's snapshot keeps its length in bullets, so the input
/// method's offsets mean what they mean in the field, and its typing lands
/// in the real text.
fn a_protected_field_is_mirrored_as_bullets() {
    let store = InMemoryTextStore::new("pw");
    store.set_protected(true);
    let published = snapshot(&store, 1);
    assert_eq!(published.text, PROTECTED_UNIT.to_string().repeat(2));
    let mut mirror = Mirror::default();
    mirror.install(&published);
    let typed = format!("{}!", published.text);
    let edit = mirror
        .report(1, &state(&typed, (3, 3), None))
        .expect("current");
    assert_eq!(edit.replaced, Some((range(2, 2), "!".to_owned())));
    assert_eq!(replay(&store, edit, 1).get(), Some(Replayed::Applied));
    assert_eq!(store.text(), "pw!");
}

/// The snapshot's offsets are what `FluiView` takes, `-1` for no composition.
fn a_snapshot_hands_the_view_its_offsets() {
    let store = InMemoryTextStore::new("abcd");
    assert_eq!(snapshot(&store, 1).java_offsets(), [4, 4, -1, -1]);
    let mut mirror = Mirror::default();
    mirror.install(&snapshot(&store, 1));
    let edit = mirror
        .report(1, &state("abcd", (3, 1), Some((1, 3))))
        .expect("current");
    assert_eq!(replay(&store, edit, 1).get(), Some(Replayed::Applied));
    assert_eq!(snapshot(&store, 2).java_offsets(), [3, 1, 1, 3]);
}

/// A key the native queue delivers while the input method composes is the
/// input method's: Escape cancels the composition, not a dialog (P14).
fn a_key_during_composition_is_the_input_methods() {
    assert_eq!(key_composition(true), ImeComposition::Active);
    assert_eq!(key_composition(false), ImeComposition::Inactive);
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

#[test]
fn android_ime_adapter() {
    super::run_table(
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
                "an_edit_made_on_a_superseded_snapshot_is_dropped",
                an_edit_made_on_a_superseded_snapshot_is_dropped,
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
                "a_key_during_composition_is_the_input_methods",
                a_key_during_composition_is_the_input_methods,
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
        ],
    );
}
