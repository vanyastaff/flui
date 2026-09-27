//! The kit's cases, version 1.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use flui_platform_api::text_store::{
    Composition, LockGrant, LockOutcome, LockTiming, OffsetError, PointMode, Selection, TextChange,
    TextStore, TextStoreEdit, TextStoreError, TextStoreObserver, TextStoreRead, Utf16Offset,
    Utf16Range, utf16,
};
use flui_types::geometry::{Bounds, Pixels, Point, px};

use super::{Case, TextStoreFixture};

/// "a", a supplementary emoji (two units), "e" and a combining acute
/// (one unit each, one grapheme), a ZWJ family (eight units, one grapheme)
/// and a flag (four units, one grapheme): 17 UTF-16 units.
const CORPUS: &str = "a😀e\u{301}👨‍👩‍👧🇯🇵";
const CORPUS_LEN: usize = 17;

type Outcome = Result<(), String>;

macro_rules! case {
    ($name:ident) => {
        Case {
            name: stringify!($name),
            since: 1,
            check: $name,
        }
    };
}

pub(super) static CASES: &[Case] = &[
    case!(length_counts_utf16_units),
    case!(text_reads_utf16_ranges),
    case!(offset_inside_a_surrogate_pair_is_refused),
    case!(offset_past_the_end_is_refused),
    case!(backward_selection_round_trips),
    case!(selection_inside_a_grapheme_is_kept_exactly),
    case!(replace_reports_ts_textchange_and_collapses_the_selection),
    case!(insert_at_selection_replaces_the_selection),
    case!(edit_shifts_an_untouched_composition_and_clears_an_overlapped_one),
    case!(composition_round_trips_with_hides_caret),
    case!(tsf_style_conversion_script),
    case!(sync_request_inside_a_session_is_refused),
    case!(async_request_inside_a_session_runs_after_it_closes_before_request_returns),
    case!(sync_request_inside_a_transaction_is_refused),
    case!(async_request_inside_a_transaction_waits_for_the_next_anchor),
    case!(deferred_grants_run_in_request_order),
    case!(a_panicking_grant_releases_the_lock),
    case!(platform_edits_are_not_echoed_to_the_observer),
    case!(app_edits_reach_the_observer_with_utf16_ranges),
    case!(one_session_is_one_owner_notification),
    case!(rects_advance_left_to_right),
    case!(index_at_point_round_trips_rect_for_range),
    case!(index_at_point_never_splits_a_surrogate_pair),
    case!(protected_store_refuses_text_reads),
];

// ============================================================================
// Helpers
// ============================================================================

fn at(units: usize) -> Utf16Offset {
    Utf16Offset::new(units)
}

fn range(start: usize, end: usize) -> Utf16Range {
    Utf16Range::new(at(start), at(end)).expect("BUG: the kit only builds ordered ranges")
}

fn ensure(condition: bool, message: impl FnOnce() -> String) -> Outcome {
    if condition { Ok(()) } else { Err(message()) }
}

fn ensure_eq<T: PartialEq + std::fmt::Debug>(actual: T, expected: T, what: &str) -> Outcome {
    ensure(actual == expected, || {
        format!("{what}: expected {expected:?}, got {actual:?}")
    })
}

/// Reset the fixture to `text` and hand back its store.
fn fresh(fixture: &mut dyn TextStoreFixture, text: &str) -> Rc<dyn TextStore> {
    fixture.reset(text);
    fixture.store()
}

/// Run `body` under a synchronous read lock and return what it returned.
fn read<R: 'static>(
    store: &Rc<dyn TextStore>,
    body: impl FnOnce(&dyn TextStoreRead) -> R + 'static,
) -> Result<R, String> {
    let slot = Rc::new(RefCell::new(None));
    let sink = Rc::clone(&slot);
    let outcome = store.request_lock(
        LockGrant::read(move |session| *sink.borrow_mut() = Some(body(session))),
        LockTiming::Sync,
    );
    ensure_eq(
        outcome,
        Ok(LockOutcome::Granted),
        "a sync read lock on an idle store",
    )?;
    slot.take()
        .ok_or_else(|| "the store reported Granted without running the grant".to_owned())
}

/// Run `body` under a synchronous read-write lock and return what it
/// returned.
fn edit<R: 'static>(
    store: &Rc<dyn TextStore>,
    body: impl FnOnce(&mut dyn TextStoreEdit) -> R + 'static,
) -> Result<R, String> {
    let slot = Rc::new(RefCell::new(None));
    let sink = Rc::clone(&slot);
    let outcome = store.request_lock(
        LockGrant::read_write(move |session| *sink.borrow_mut() = Some(body(session))),
        LockTiming::Sync,
    );
    ensure_eq(
        outcome,
        Ok(LockOutcome::Granted),
        "a sync read-write lock on an idle store",
    )?;
    slot.take()
        .ok_or_else(|| "the store reported Granted without running the grant".to_owned())
}

/// Check `store`'s text in `range` is `expected` — or, on a protected
/// fixture, that the read is refused.
fn expect_text(
    fixture: &dyn TextStoreFixture,
    store: &Rc<dyn TextStore>,
    within: Utf16Range,
    expected: &str,
) -> Outcome {
    let actual = read(store, move |session| session.text(within))?;
    if fixture.capabilities().protected {
        ensure_eq(
            actual,
            Err(TextStoreError::Protected),
            "a protected text read",
        )
    } else {
        ensure_eq(
            actual,
            Ok(expected.to_owned()),
            &format!("text in {within:?}"),
        )
    }
}

/// The UTF-16 offset of every scalar boundary of `text`, end included.
fn scalar_boundaries(text: &str) -> Vec<Utf16Offset> {
    let mut boundaries: Vec<Utf16Offset> = text
        .char_indices()
        .map(|(byte, _)| utf16::utf16_offset(text, byte).expect("BUG: char_indices are boundaries"))
        .collect();
    boundaries.push(utf16::utf16_len(text));
    boundaries
}

fn centre(bounds: Bounds<Pixels>) -> Point<Pixels> {
    Point::new(
        bounds.origin.x + bounds.size.width / 2.0,
        bounds.origin.y + bounds.size.height / 2.0,
    )
}

fn right(bounds: Bounds<Pixels>) -> f32 {
    (bounds.origin.x + bounds.size.width).get()
}

/// What an observer heard, and whether the store was lockable each time.
#[derive(Default)]
struct Heard {
    changes: RefCell<Vec<TextChange>>,
    selections: Cell<usize>,
    /// Each notification requests a sync read lock: TSF sinks do, and a
    /// store that notifies from inside a lock refuses it.
    refused_inside_notification: Cell<usize>,
    store: RefCell<Option<Weak<dyn TextStore>>>,
}

struct Observer(Rc<Heard>);

impl Observer {
    fn probe(&self) {
        let store = self.0.store.borrow().as_ref().and_then(Weak::upgrade);
        if let Some(store) = store {
            let outcome = store.request_lock(LockGrant::read(|_| {}), LockTiming::Sync);
            if outcome != Ok(LockOutcome::Granted) {
                self.0
                    .refused_inside_notification
                    .set(self.0.refused_inside_notification.get() + 1);
            }
        }
    }
}

impl TextStoreObserver for Observer {
    fn text_changed(&self, change: TextChange) {
        self.0.changes.borrow_mut().push(change);
        self.probe();
    }

    fn selection_changed(&self) {
        self.0.selections.set(self.0.selections.get() + 1);
        self.probe();
    }

    fn layout_changed(&self) {
        self.probe();
    }

    fn status_changed(&self) {
        self.probe();
    }
}

fn observe(store: &Rc<dyn TextStore>) -> Rc<Heard> {
    let heard = Rc::new(Heard::default());
    *heard.store.borrow_mut() = Some(Rc::downgrade(store));
    store.set_observer(Some(Rc::new(Observer(Rc::clone(&heard)))));
    heard
}

// ============================================================================
// Lengths and reads
// ============================================================================

fn length_counts_utf16_units(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let len = read(&store, |session| session.document_len())?;
    ensure_eq(len, at(CORPUS_LEN), "document_len of the corpus")
}

fn text_reads_utf16_ranges(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    expect_text(fixture, &store, range(1, 3), "😀")?;
    expect_text(fixture, &store, range(5, 13), "👨‍👩‍👧")?;
    expect_text(fixture, &store, range(0, CORPUS_LEN), CORPUS)
}

// ============================================================================
// Offset refusals
// ============================================================================

fn offset_inside_a_surrogate_pair_is_refused(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let split = Some(TextStoreError::Offset(OffsetError::SplitsSurrogatePair(2)));
    let read_result = read(&store, |session| session.text(range(0, 2)))?;
    ensure_eq(read_result.err(), split, "text(0..2)")?;
    let (replaced, selected) = edit(&store, |session| {
        (
            session.replace(range(2, 2), "x").err(),
            session.set_selection(Selection::collapsed(at(2))).err(),
        )
    })?;
    ensure_eq(replaced, split, "replace(2..2)")?;
    ensure_eq(selected, split, "set_selection(2)")?;
    let len = read(&store, |session| session.document_len())?;
    ensure_eq(len, at(CORPUS_LEN), "the length after refused edits")
}

fn offset_past_the_end_is_refused(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let past = Some(TextStoreError::Offset(OffsetError::PastEnd {
        offset: CORPUS_LEN + 1,
        len: CORPUS_LEN,
    }));
    let read_result = read(&store, |session| session.text(range(0, CORPUS_LEN + 1)))?;
    ensure_eq(read_result.err(), past, "text past the end")?;
    let selected = edit(&store, |session| {
        session
            .set_selection(Selection::collapsed(at(CORPUS_LEN + 1)))
            .err()
    })?;
    ensure_eq(selected, past, "set_selection past the end")
}

// ============================================================================
// Selection
// ============================================================================

fn backward_selection_round_trips(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let backward = Selection {
        anchor: at(13),
        active: at(1),
    };
    let set = edit(&store, move |session| session.set_selection(backward))?;
    ensure_eq(set, Ok(()), "set_selection(13 -> 1)")?;
    let seen = read(&store, |session| session.selection())?;
    ensure_eq(seen, backward, "the selection in a later session")?;
    ensure_eq(seen.range(), range(1, 13), "the backward selection's range")
}

fn selection_inside_a_grapheme_is_kept_exactly(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    // Offset 4 lies between "e" and its combining acute: a scalar boundary,
    // not a grapheme boundary. The platform's selection is kept as given.
    let set = edit(&store, |session| {
        session.set_selection(Selection::collapsed(at(4)))
    })?;
    ensure_eq(set, Ok(()), "set_selection(4)")?;
    fixture.pump();
    let seen = read(&store, |session| session.selection())?;
    ensure_eq(
        seen,
        Selection::collapsed(at(4)),
        "the selection after a commit anchor",
    )
}

// ============================================================================
// Edits
// ============================================================================

fn replace_reports_ts_textchange_and_collapses_the_selection(
    fixture: &mut dyn TextStoreFixture,
) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let (change, selection) = edit(&store, |session| {
        (session.replace(range(1, 3), "xy"), session.selection())
    })?;
    ensure_eq(
        change,
        Ok(TextChange {
            start: at(1),
            old_end: at(3),
            new_end: at(3),
        }),
        "replace(1..3, \"xy\")",
    )?;
    ensure_eq(
        selection,
        Selection::collapsed(at(3)),
        "the selection after replace",
    )?;
    expect_text(fixture, &store, range(0, 4), "axye")
}

fn insert_at_selection_replaces_the_selection(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let (change, selection, len) = edit(&store, |session| {
        let _ = session.set_selection(Selection {
            anchor: at(1),
            active: at(0),
        });
        let change = session.insert_at_selection("Z😀");
        (change, session.selection(), session.document_len())
    })?;
    ensure_eq(
        change,
        Ok(TextChange {
            start: at(0),
            old_end: at(1),
            new_end: at(3),
        }),
        "insert_at_selection over 0..1",
    )?;
    ensure_eq(
        selection,
        Selection::collapsed(at(3)),
        "the caret after the insertion",
    )?;
    ensure_eq(len, at(CORPUS_LEN + 2), "the length after the insertion")?;
    expect_text(fixture, &store, range(0, 5), "Z😀😀")
}

fn edit_shifts_an_untouched_composition_and_clears_an_overlapped_one(
    fixture: &mut dyn TextStoreFixture,
) -> Outcome {
    let store = fresh(fixture, "abcdef");
    let composing = |start, end| {
        Some(Composition {
            range: range(start, end),
            hides_caret: false,
        })
    };
    let (before, at_end, inside) = edit(&store, move |session| {
        let _ = session.set_composition(composing(2, 4));
        let _ = session.replace(range(0, 1), "xyz");
        let before = session.composition();
        let _ = session.replace(range(6, 6), "!");
        let at_end = session.composition();
        let _ = session.replace(range(5, 5), "?");
        (before, at_end, session.composition())
    })?;
    ensure_eq(
        before,
        composing(4, 6),
        "a composition after an edit before it",
    )?;
    ensure_eq(
        at_end,
        composing(4, 6),
        "a composition after an insertion at its end (never extended)",
    )?;
    ensure_eq(inside, None, "a composition after an insertion inside it")
}

// ============================================================================
// Composition
// ============================================================================

fn composition_round_trips_with_hides_caret(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    for composition in [
        Some(Composition {
            range: range(1, 3),
            hides_caret: true,
        }),
        Some(Composition {
            range: range(5, 13),
            hides_caret: false,
        }),
        None,
    ] {
        let set = edit(&store, move |session| session.set_composition(composition))?;
        ensure_eq(set, Ok(()), &format!("set_composition({composition:?})"))?;
        let seen = read(&store, |session| session.composition())?;
        ensure_eq(seen, composition, "the composition in a later session")?;
    }
    Ok(())
}

fn tsf_style_conversion_script(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, "");
    let before = fixture.owner_notifications();
    let steps = edit(&store, |session| -> Result<(), TextStoreError> {
        let typed = session.insert_at_selection("とうきょう")?;
        session.set_composition(Some(Composition {
            range: range(0, 5),
            hides_caret: false,
        }))?;
        session.set_selection(Selection::collapsed(typed.new_end))?;
        let converted = session.replace(range(0, 5), "東京")?;
        session.set_composition(Some(Composition {
            range: range(0, 2),
            hides_caret: false,
        }))?;
        session.set_composition(None)?;
        session.set_selection(Selection::collapsed(converted.new_end))
    })?;
    ensure_eq(steps, Ok(()), "the conversion script's edits")?;
    let (composition, selection, len) = read(&store, |session| {
        (
            session.composition(),
            session.selection(),
            session.document_len(),
        )
    })?;
    ensure_eq(composition, None, "the composition after the conversion")?;
    ensure_eq(
        selection,
        Selection::collapsed(at(2)),
        "the caret after the conversion",
    )?;
    ensure_eq(len, at(2), "the length after the conversion")?;
    expect_text(fixture, &store, range(0, 2), "東京")?;
    ensure_eq(
        fixture.owner_notifications(),
        before + 1,
        "owner notifications for the one conversion session",
    )
}

// ============================================================================
// Locks
// ============================================================================

fn sync_request_inside_a_session_is_refused(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let inner_store = Rc::clone(&store);
    let nested = read(&store, move |_| {
        inner_store.request_lock(LockGrant::read(|_| {}), LockTiming::Sync)
    })?;
    ensure_eq(
        nested,
        Err(TextStoreError::SyncLockUnavailable),
        "a sync request inside a session",
    )
}

fn async_request_inside_a_session_runs_after_it_closes_before_request_returns(
    fixture: &mut dyn TextStoreFixture,
) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let log = Rc::new(RefCell::new(Vec::new()));
    let (outer_log, inner_store) = (Rc::clone(&log), Rc::clone(&store));
    let nested = edit(&store, move |_| {
        outer_log.borrow_mut().push("outer");
        let inner_log = Rc::clone(&outer_log);
        let nested = inner_store.request_lock(
            LockGrant::read(move |_| inner_log.borrow_mut().push("nested")),
            LockTiming::Async,
        );
        outer_log.borrow_mut().push("outer ends");
        nested
    })?;
    ensure_eq(
        nested,
        Ok(LockOutcome::Deferred),
        "an async request inside a session",
    )?;
    ensure_eq(
        log.borrow().clone(),
        vec!["outer", "outer ends", "nested"],
        "the order the grants ran in",
    )
}

fn sync_request_inside_a_transaction_is_refused(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let ran = Rc::new(Cell::new(false));
    let mut outcome = None;
    fixture.within_transaction(&mut || {
        let ran = Rc::clone(&ran);
        outcome =
            Some(store.request_lock(LockGrant::read(move |_| ran.set(true)), LockTiming::Sync));
    });
    ensure_eq(
        outcome,
        Some(Err(TextStoreError::SyncLockUnavailable)),
        "a sync request inside a transaction",
    )?;
    fixture.pump();
    ensure(!ran.get(), || "a refused sync grant ran later".to_owned())
}

fn async_request_inside_a_transaction_waits_for_the_next_anchor(
    fixture: &mut dyn TextStoreFixture,
) -> Outcome {
    let store = fresh(fixture, "abc");
    let edits = Rc::new(Cell::new(0));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut outcomes = Vec::new();
    let mut ran_inside = None;
    fixture.within_transaction(&mut || {
        let edits_run = Rc::clone(&edits);
        outcomes.push(store.request_lock(
            LockGrant::read_write(move |session| {
                edits_run.set(edits_run.get() + 1);
                let _ = session.insert_at_selection("X");
            }),
            LockTiming::Async,
        ));
        let sink = Rc::clone(&seen);
        outcomes.push(store.request_lock(
            LockGrant::read(move |session| {
                sink.borrow_mut()
                    .push((session.document_len(), session.selection()));
            }),
            LockTiming::Async,
        ));
        ran_inside = Some(edits.get() + seen.borrow().len());
    });
    ensure_eq(
        outcomes,
        vec![Ok(LockOutcome::Deferred); 2],
        "async requests inside a transaction",
    )?;
    ensure_eq(ran_inside, Some(0), "grants run inside the transaction")?;
    fixture.pump();
    fixture.pump();
    ensure_eq(edits.get(), 1, "runs of the deferred edit")?;
    ensure_eq(
        seen.borrow().clone(),
        vec![(at(4), Selection::collapsed(at(4)))],
        "what the deferred read saw (once, after the deferred edit)",
    )
}

fn deferred_grants_run_in_request_order(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let log = Rc::new(RefCell::new(Vec::new()));
    fixture.within_transaction(&mut || {
        for label in ["first", "second", "third"] {
            let sink = Rc::clone(&log);
            let _ = store.request_lock(
                LockGrant::read(move |_| sink.borrow_mut().push(label)),
                LockTiming::Async,
            );
        }
    });
    fixture.pump();
    ensure_eq(
        log.borrow().clone(),
        vec!["first", "second", "third"],
        "the order deferred grants ran in",
    )
}

fn a_panicking_grant_releases_the_lock(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        let _ = store.request_lock(
            LockGrant::read_write(|_| panic!("the kit's deliberately failing grant")),
            LockTiming::Sync,
        );
    }));
    ensure(unwound.is_err(), || {
        "a panicking grant did not unwind out of request_lock".to_owned()
    })?;
    let len = read(&store, |session| session.document_len())?;
    ensure_eq(len, at(CORPUS_LEN), "the length after a panicking grant")
}

// ============================================================================
// Observer and notifications
// ============================================================================

fn platform_edits_are_not_echoed_to_the_observer(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let heard = observe(&store);
    edit(&store, |session| {
        let _ = session.replace(range(0, 1), "b");
        let _ = session.set_selection(Selection::collapsed(at(1)));
        let _ = session.set_composition(Some(Composition {
            range: range(1, 3),
            hides_caret: false,
        }));
    })?;
    fixture.pump();
    store.set_observer(None);
    ensure_eq(
        heard.changes.borrow().clone(),
        Vec::new(),
        "text changes echoed back for the platform's own edits",
    )?;
    ensure_eq(
        heard.selections.get(),
        0,
        "selection changes echoed back for the platform's own edits",
    )
}

fn app_edits_reach_the_observer_with_utf16_ranges(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, CORPUS);
    let heard = observe(&store);
    fixture.app_replace_all("x😀");
    store.set_observer(None);
    ensure_eq(
        heard.changes.borrow().clone(),
        vec![TextChange {
            start: at(0),
            old_end: at(CORPUS_LEN),
            new_end: at(3),
        }],
        "the observer's text changes for an app edit",
    )?;
    ensure_eq(
        heard.refused_inside_notification.get(),
        0,
        "notifications made while the store was locked",
    )
}

fn one_session_is_one_owner_notification(fixture: &mut dyn TextStoreFixture) -> Outcome {
    let store = fresh(fixture, "");
    let before = fixture.owner_notifications();
    edit(&store, |session| {
        for text in ["a", "b", "c"] {
            let _ = session.insert_at_selection(text);
        }
    })?;
    fixture.pump();
    ensure_eq(
        fixture.owner_notifications(),
        before + 1,
        "owner notifications for one session of three edits",
    )
}

// ============================================================================
// Geometry
// ============================================================================

fn scalar_rects(store: &Rc<dyn TextStore>) -> Result<Vec<(Utf16Offset, Bounds<Pixels>)>, String> {
    let boundaries = scalar_boundaries(CORPUS);
    read(store, move |session| {
        boundaries
            .windows(2)
            .map(|pair| {
                let rect = session
                    .rect_for_range(Utf16Range::new(pair[0], pair[1]).expect("BUG: ascending"))
                    .map_err(|error| format!("rect_for_range({pair:?}): {error}"))?;
                Ok((pair[0], rect.bounds))
            })
            .collect::<Result<Vec<_>, String>>()
    })?
}

fn rects_advance_left_to_right(fixture: &mut dyn TextStoreFixture) -> Outcome {
    if !fixture.capabilities().geometry {
        return Ok(());
    }
    let store = fresh(fixture, CORPUS);
    let rects = scalar_rects(&store)?;
    for pair in rects.windows(2) {
        let ((first, a), (second, b)) = (pair[0], pair[1]);
        ensure(b.origin.x.get() + 0.01 >= a.origin.x.get(), || {
            format!("the rect at {second:?} ({b:?}) starts left of the one at {first:?} ({a:?})")
        })?;
    }
    let whole = read(&store, |session| {
        session.rect_for_range(range(0, CORPUS_LEN))
    })?
    .map_err(|error| format!("rect_for_range(whole): {error}"))?;
    ensure(whole.bounds.size.width.get() > 0.0, || {
        format!("the whole text's rect has no width: {whole:?}")
    })
}

fn index_at_point_round_trips_rect_for_range(fixture: &mut dyn TextStoreFixture) -> Outcome {
    if !fixture.capabilities().geometry {
        return Ok(());
    }
    let store = fresh(fixture, CORPUS);
    let rects = scalar_rects(&store)?;
    for &(start, bounds) in &rects {
        // A scalar a layout draws with no width of its own has no interior
        // point to ask about.
        if bounds.size.width.get() < 0.5 {
            continue;
        }
        let point = centre(bounds);
        let found = read(&store, move |session| {
            session.index_at_point(point, PointMode::Exact)
        })?
        .map_err(|error| format!("index_at_point({point:?}) for {start:?}: {error}"))?;
        // The character the store names must be at or before the one asked
        // about (a layout may draw a cluster as one glyph), and its own rect
        // must contain the point.
        let Some(&(_, named)) = rects.iter().find(|(offset, _)| *offset == found) else {
            return Err(format!(
                "index_at_point({point:?}) for {start:?} answered {found:?}, not a scalar start"
            ));
        };
        ensure(found <= start, || {
            format!("index_at_point for {start:?} answered the later {found:?}")
        })?;
        ensure(
            named.origin.x.get() <= point.x.get() + 0.01 && point.x.get() <= right(named) + 0.01,
            || {
                format!(
                    "index_at_point for {start:?} answered {found:?}, whose rect {named:?} misses {point:?}"
                )
            },
        )?;
    }
    Ok(())
}

fn index_at_point_never_splits_a_surrogate_pair(fixture: &mut dyn TextStoreFixture) -> Outcome {
    if !fixture.capabilities().geometry {
        return Ok(());
    }
    let store = fresh(fixture, CORPUS);
    let boundaries = scalar_boundaries(CORPUS);
    let bounds = read(&store, |session| session.document_bounds())?
        .map_err(|error| format!("document_bounds: {error}"))?;
    let y = centre(bounds).y;
    let (left, width) = (bounds.origin.x.get(), bounds.size.width.get());
    for step in 0..=40_u8 {
        let x = left - 5.0 + (width + 10.0) * f32::from(step) / 40.0;
        let point = Point::new(px(x), y);
        for mode in [PointMode::Exact, PointMode::Nearest] {
            let found = read(&store, move |session| session.index_at_point(point, mode))?;
            match found {
                Ok(offset) => ensure(boundaries.contains(&offset), || {
                    format!("index_at_point({point:?}, {mode:?}) = {offset:?}, inside a scalar")
                })?,
                Err(TextStoreError::PointOutside) if mode == PointMode::Exact => {}
                Err(error) => return Err(format!("index_at_point({point:?}, {mode:?}): {error}")),
            }
        }
    }
    Ok(())
}

// ============================================================================
// Protected
// ============================================================================

fn protected_store_refuses_text_reads(fixture: &mut dyn TextStoreFixture) -> Outcome {
    if !fixture.capabilities().protected {
        return Ok(());
    }
    let store = fresh(fixture, CORPUS);
    ensure(store.status().protected, || {
        "a protected fixture's status is not protected".to_owned()
    })?;
    let (text, len) = read(&store, |session| {
        (session.text(range(0, 1)), session.document_len())
    })?;
    ensure_eq(
        text,
        Err(TextStoreError::Protected),
        "a protected text read",
    )?;
    ensure_eq(len, at(CORPUS_LEN), "a protected store's length")?;
    let replaced = edit(&store, |session| {
        session.replace(range(0, 1), "b").map(|_| ())
    })?;
    ensure_eq(replaced, Ok(()), "an edit on a protected store")
}
