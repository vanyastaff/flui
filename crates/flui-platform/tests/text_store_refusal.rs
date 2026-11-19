//! An edit a store refuses inside a grant: a grant that ran before the
//! request returned hands the refusal to the caller; one the store queued
//! hands it to the late reporter the caller supplied.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::geometry::{Bounds, Point};
use flui_platform::ImeEvent;
use flui_platform::text_store::{
    CommitGate, Composition, LockGrant, LockOutcome, LockTiming, PointMode, RangeRect, Selection,
    TextChange, TextStore, TextStoreEdit, TextStoreError, TextStoreObserver, TextStoreRead,
    TextStoreStatus, Utf16Offset, Utf16Range, commit_composition_in_place, project_ime_event,
};

/// A field that takes no edits: it holds a composition over its whole text and
/// refuses every edit, as a read-only field may. While `queues` is set it
/// queues every grant for [`TextStore::run_deferred_grants`].
struct ReadOnlyField {
    queues: Cell<bool>,
    queued: RefCell<Vec<LockGrant>>,
}

impl ReadOnlyField {
    fn new() -> Self {
        Self {
            queues: Cell::new(false),
            queued: RefCell::new(Vec::new()),
        }
    }
}

impl TextStore for ReadOnlyField {
    fn status(&self) -> TextStoreStatus {
        TextStoreStatus::EDITABLE_SINGLE_LINE
    }

    fn request_lock(&self, grant: LockGrant, _: LockTiming) -> Result<LockOutcome, TextStoreError> {
        if self.queues.get() {
            self.queued.borrow_mut().push(grant);
            return Ok(LockOutcome::Deferred);
        }
        run(grant);
        Ok(LockOutcome::Granted)
    }

    fn run_deferred_grants(&self) -> usize {
        let queued = std::mem::take(&mut *self.queued.borrow_mut());
        let ran = queued.len();
        queued.into_iter().for_each(run);
        ran
    }

    fn set_commit_gate(&self, _: CommitGate) {}

    fn set_observer(&self, _: Option<Rc<dyn TextStoreObserver>>) {}

    fn press_target_at(
        &self,
        _: Point<f64>,
    ) -> Result<flui_platform::text_store::PressTarget, TextStoreError> {
        Err(TextStoreError::NoLayout)
    }
}

fn run(grant: LockGrant) {
    match grant {
        LockGrant::Read(body) => body(&ReadOnlySession),
        LockGrant::ReadWrite(body) => body(&mut ReadOnlySession),
    }
}

/// The session of [`ReadOnlyField`]: the text "ab", composing.
struct ReadOnlySession;

const LEN: Utf16Offset = Utf16Offset::new(2);

impl TextStoreRead for ReadOnlySession {
    fn document_len(&self) -> Utf16Offset {
        LEN
    }
    fn text(&self, _: Utf16Range) -> Result<String, TextStoreError> {
        Ok("ab".to_owned())
    }
    fn selection(&self) -> Selection {
        Selection::collapsed(LEN)
    }
    fn composition(&self) -> Option<Composition> {
        Some(Composition {
            range: Utf16Range::try_new(Utf16Offset::new(0), LEN).expect("ordered"),
            hides_caret: false,
        })
    }
    fn rect_for_range(&self, _: Utf16Range) -> Result<RangeRect, TextStoreError> {
        Err(TextStoreError::NoLayout)
    }
    fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
        Err(TextStoreError::NoLayout)
    }
    fn index_at_point(&self, _: Point<f64>, _: PointMode) -> Result<Utf16Offset, TextStoreError> {
        Err(TextStoreError::NoLayout)
    }
    fn visible_bounds(&self) -> Result<Option<Bounds<f64>>, TextStoreError> {
        Err(TextStoreError::NoLayout)
    }
}

impl TextStoreEdit for ReadOnlySession {
    fn replace(&mut self, _: Utf16Range, _: &str) -> Result<TextChange, TextStoreError> {
        Err(TextStoreError::Protected)
    }
    fn insert_at_selection(&mut self, _: &str) -> Result<TextChange, TextStoreError> {
        Err(TextStoreError::Protected)
    }
    fn set_selection(&mut self, _: Selection) -> Result<(), TextStoreError> {
        Err(TextStoreError::Protected)
    }
    fn set_composition(&mut self, _: Option<Composition>) -> Result<(), TextStoreError> {
        Err(TextStoreError::Protected)
    }
}

/// The late refusals a test hears, in order.
type Heard = Rc<RefCell<Vec<(&'static str, TextStoreError)>>>;

fn hear(heard: &Heard, what: &'static str) -> impl FnOnce(TextStoreError) + 'static {
    let heard = Rc::clone(heard);
    move |error| heard.borrow_mut().push((what, error))
}

/// The commit in place and the projection answer a refusal in a grant that
/// ran at once with the store's error, not `Granted`, and call no late
/// reporter; a refusal in a grant the store queued reaches the reporter its
/// caller supplied, once, when the queue drains.
#[test]
fn a_refused_edit_reaches_the_caller_or_its_late_reporter() {
    let field = ReadOnlyField::new();
    let heard = Heard::default();
    assert_eq!(
        commit_composition_in_place(&field, hear(&heard, "commit")),
        Err(TextStoreError::Protected),
        "the commit's refusal is the caller's"
    );
    assert_eq!(
        project_ime_event(
            &field,
            &ImeEvent::Commit("c".to_owned()),
            hear(&heard, "projection")
        ),
        Err(TextStoreError::Protected),
        "the projection's refusal is the caller's"
    );
    assert!(heard.borrow().is_empty(), "a refusal at once is not late");

    field.queues.set(true);
    assert_eq!(
        commit_composition_in_place(&field, hear(&heard, "commit")),
        Ok(LockOutcome::Deferred)
    );
    assert_eq!(
        project_ime_event(&field, &ImeEvent::Disabled, hear(&heard, "projection")),
        Ok(LockOutcome::Deferred)
    );
    assert!(
        heard.borrow().is_empty(),
        "nothing refused before the anchor"
    );
    assert_eq!(field.run_deferred_grants(), 2, "both queued grants ran");
    assert_eq!(
        *heard.borrow(),
        [
            ("commit", TextStoreError::Protected),
            ("projection", TextStoreError::Protected)
        ],
        "each queued refusal reaches its reporter once"
    );
}
