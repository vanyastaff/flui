//! [`InMemoryTextStore`]: a complete text store over a `String`, with no
//! widget and no layout engine behind it.
//!
//! It is ADR-0090 §4's "test-only minimal field": the reference the
//! conformance kit (`flui_testing::text_store_kit`) certifies itself
//! against, and the store a platform backend's tests drive without mounting
//! a widget tree. It stands to [`TextStore`] as
//! [`InMemoryClipboard`](crate::InMemoryClipboard) stands to
//! [`Clipboard`](crate::Clipboard).
//!
//! Its geometry is a fixed grid: every Unicode scalar is one cell
//! [`InMemoryTextStore::ADVANCE`] wide and [`InMemoryTextStore::LINE_HEIGHT`]
//! tall, starting at the origin, so a test can compute every rect and point
//! by hand.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_types::geometry::{Bounds, Pixels, Point, Size, px};

use super::lock::{CommitGate, LockArbiter, LockGrant, LockOutcome, LockTiming, TextStoreError};
use super::session::{
    Composition, PointMode, RangeRect, Selection, TextChange, TextStoreEdit, TextStoreRead,
    TextStoreStatus,
};
use super::store::{TextStore, TextStoreObserver};
use super::utf16::{self, Utf16Offset, Utf16Range};

/// A text store over a `String`, for tests and as the kit's reference.
///
/// Owner-thread only (`Rc`, not `Send`). Platform edits go through
/// [`TextStore::request_lock`]; [`Self::app_replace`] is an edit the
/// application makes, reported to the observer afterwards.
pub struct InMemoryTextStore {
    doc: RefCell<Document>,
    arbiter: LockArbiter,
    observer: RefCell<Option<Rc<dyn TextStoreObserver>>>,
    /// App-side changes the observer has not heard of yet, because a lock
    /// was held or the gate was shut when they happened.
    pending: RefCell<Vec<Notice>>,
    protected: Cell<bool>,
    owner_notifications: Cell<usize>,
}

/// One notification an [`InMemoryTextStore`] owes its observer.
#[derive(Debug)]
enum Notice {
    Text(TextChange),
    Selection,
    Status,
}

impl std::fmt::Debug for InMemoryTextStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryTextStore")
            .field("doc", &self.doc)
            .field("arbiter", &self.arbiter)
            .field("pending", &self.pending)
            .field("protected", &self.protected.get())
            .finish_non_exhaustive()
    }
}

impl InMemoryTextStore {
    /// The width of one scalar's cell, in logical pixels.
    pub const ADVANCE: f32 = 10.0;
    /// The height of the one line, in logical pixels.
    pub const LINE_HEIGHT: f32 = 20.0;

    /// A store holding `text`, caret at the end, no composition, behind an
    /// open gate of its own until [`TextStore::set_commit_gate`] installs
    /// another.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Rc<Self> {
        let text = text.into();
        let end = utf16::utf16_len(&text);
        Rc::new(Self {
            doc: RefCell::new(Document {
                text,
                selection: Selection::collapsed(end),
                composition: None,
            }),
            arbiter: LockArbiter::new(),
            observer: RefCell::new(None),
            pending: RefCell::new(Vec::new()),
            protected: Cell::new(false),
            owner_notifications: Cell::new(0),
        })
    }

    /// The whole text.
    #[must_use]
    pub fn text(&self) -> String {
        self.doc.borrow().text.clone()
    }

    /// The current selection.
    #[must_use]
    pub fn selection(&self) -> Selection {
        self.doc.borrow().selection
    }

    /// The current composition.
    #[must_use]
    pub fn composition(&self) -> Option<Composition> {
        self.doc.borrow().composition
    }

    /// Mark the store protected (a password field) and tell the observer.
    pub fn set_protected(&self, protected: bool) {
        if self.protected.replace(protected) != protected {
            self.report(Notice::Status);
        }
    }

    /// An edit the application makes: `range` becomes `text` under the same
    /// rules as [`TextStoreEdit::replace`], and the observer hears of it
    /// afterwards — at once, or, inside a frame transaction, once the gate
    /// opens.
    ///
    /// # Panics
    ///
    /// When `range` does not name a range of the text, or when called from
    /// inside a grant: both are the calling test's bug.
    pub fn app_replace(&self, range: Utf16Range, text: &str) {
        let change = self
            .doc
            .borrow_mut()
            .replace(range, text)
            .expect("BUG: app_replace was given a range outside the text");
        self.pending.borrow_mut().push(Notice::Text(change));
        self.report(Notice::Selection);
    }

    /// How many read-write sessions changed the document: the notifications
    /// a field would send its own listeners, one per session.
    #[must_use]
    pub fn owner_notifications(&self) -> usize {
        self.owner_notifications.get()
    }

    /// Queue `notice` and send everything queued if the observer may hear
    /// it now.
    fn report(&self, notice: Notice) {
        self.pending.borrow_mut().push(notice);
        self.flush_notifications();
    }

    /// Send the queued notices, unless a lock is held or the gate is shut:
    /// an observer that answers with a synchronous lock request (a TSF sink)
    /// must be granted it.
    fn flush_notifications(&self) {
        if self.arbiter.is_locked() || !self.arbiter.may_commit() {
            return;
        }
        let notices = std::mem::take(&mut *self.pending.borrow_mut());
        let observer = self.observer.borrow().clone();
        let Some(observer) = observer else {
            return;
        };
        for notice in notices {
            match notice {
                Notice::Text(change) => observer.text_changed(change),
                Notice::Selection => observer.selection_changed(),
                Notice::Status => observer.status_changed(),
            }
        }
    }

    fn open(&self, grant: LockGrant) {
        let protected = self.protected.get();
        match grant {
            LockGrant::Read(body) => {
                let doc = self.doc.borrow();
                body(&ReadSession {
                    doc: &doc,
                    protected,
                });
            }
            LockGrant::ReadWrite(body) => {
                let edited = {
                    let mut doc = self.doc.borrow_mut();
                    let mut session = EditSession {
                        doc: &mut doc,
                        protected,
                        edited: false,
                    };
                    body(&mut session);
                    session.edited
                };
                if edited {
                    self.owner_notifications
                        .set(self.owner_notifications.get() + 1);
                }
            }
        }
    }
}

impl TextStore for InMemoryTextStore {
    fn status(&self) -> TextStoreStatus {
        TextStoreStatus::EDITABLE_SINGLE_LINE.with_protected(self.protected.get())
    }

    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        // An app edit still owed is reported before the platform's session
        // can see it, and one made from inside the grant once it ends.
        self.flush_notifications();
        let outcome = self
            .arbiter
            .request(grant, timing, &mut |grant| self.open(grant));
        self.flush_notifications();
        outcome
    }

    /// Also where notifications held back by the frame transaction are
    /// sent, before and after the queued grants run.
    fn run_deferred_grants(&self) -> usize {
        self.flush_notifications();
        let ran = self.arbiter.run_deferred(&mut |grant| self.open(grant));
        self.flush_notifications();
        ran
    }

    fn set_commit_gate(&self, gate: CommitGate) {
        self.arbiter.set_gate(gate);
    }

    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        *self.observer.borrow_mut() = observer;
    }
}

/// The document an [`InMemoryTextStore`] holds.
#[derive(Debug)]
struct Document {
    text: String,
    selection: Selection,
    composition: Option<Composition>,
}

impl Document {
    fn check(&self, offset: Utf16Offset) -> Result<(), TextStoreError> {
        utf16::byte_offset(&self.text, offset)?;
        Ok(())
    }

    fn replace(&mut self, range: Utf16Range, text: &str) -> Result<TextChange, TextStoreError> {
        let bytes = utf16::byte_range(&self.text, range)?;
        self.text.replace_range(bytes, text);
        let inserted = utf16::utf16_len(text).get();
        let new_end = Utf16Offset::new(range.start().get() + inserted);
        self.selection = Selection::collapsed(new_end);
        self.composition = self
            .composition
            .and_then(|composition| shift_composition(composition, range, inserted));
        Ok(TextChange {
            start: range.start(),
            old_end: range.end(),
            new_end,
        })
    }

    /// The cell index of the scalar starting at `offset`.
    fn cell_of(&self, offset: Utf16Offset) -> Result<usize, TextStoreError> {
        let byte = utf16::byte_offset(&self.text, offset)?;
        Ok(self.text[..byte].chars().count())
    }

    /// The UTF-16 offset where cell `cell` starts.
    fn offset_of_cell(&self, cell: usize) -> Utf16Offset {
        Utf16Offset::new(self.text.chars().take(cell).map(char::len_utf16).sum())
    }

    fn cells(&self) -> usize {
        self.text.chars().count()
    }
}

/// Where a composition goes when `edit` is replaced by `inserted` units:
/// shifted when the edit lies wholly before it, kept when wholly after,
/// dropped when the two overlap or the edit inserts strictly inside it.
fn shift_composition(
    composition: Composition,
    edit: Utf16Range,
    inserted: usize,
) -> Option<Composition> {
    let (start, end) = (edit.start().get(), edit.end().get());
    let (composed_start, composed_end) = (
        composition.range.start().get(),
        composition.range.end().get(),
    );
    if end <= composed_start {
        let shift = |offset: usize| Utf16Offset::new(offset + inserted - (end - start));
        let range = Utf16Range::new(shift(composed_start), shift(composed_end))
            .expect("BUG: shifting both ends by one amount keeps them ordered");
        Some(Composition {
            range,
            ..composition
        })
    } else if start >= composed_end {
        Some(composition)
    } else {
        None
    }
}

struct ReadSession<'a> {
    doc: &'a Document,
    protected: bool,
}

struct EditSession<'a> {
    doc: &'a mut Document,
    protected: bool,
    edited: bool,
}

fn read_text(doc: &Document, protected: bool, range: Utf16Range) -> Result<String, TextStoreError> {
    let bytes = utf16::byte_range(&doc.text, range)?;
    if protected {
        return Err(TextStoreError::Protected);
    }
    Ok(doc.text[bytes].to_owned())
}

fn rect_for_range(doc: &Document, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
    let first = doc.cell_of(range.start())?;
    let last = doc.cell_of(range.end())?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a test store's cell count is far below f32's exact-integer range"
    )]
    let (left, width) = (
        first as f32 * InMemoryTextStore::ADVANCE,
        (last - first) as f32 * InMemoryTextStore::ADVANCE,
    );
    Ok(RangeRect {
        bounds: Bounds::new(
            Point::new(px(left), px(0.0)),
            Size::new(px(width), px(InMemoryTextStore::LINE_HEIGHT)),
        ),
        clipped: false,
    })
}

fn document_bounds(doc: &Document) -> Bounds<Pixels> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a test store's cell count is far below f32's exact-integer range"
    )]
    let width = doc.cells() as f32 * InMemoryTextStore::ADVANCE;
    Bounds::new(
        Point::new(px(0.0), px(0.0)),
        Size::new(px(width), px(InMemoryTextStore::LINE_HEIGHT)),
    )
}

fn index_at_point(
    doc: &Document,
    point: Point<Pixels>,
    mode: PointMode,
) -> Result<Utf16Offset, TextStoreError> {
    let cells = doc.cells();
    let x = point.x.get() / InMemoryTextStore::ADVANCE;
    let y = point.y.get();
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the cell index is clamped to 0..=cells before the cast"
    )]
    let cell = match mode {
        PointMode::Exact => {
            let inside = (0.0..InMemoryTextStore::LINE_HEIGHT).contains(&y)
                && (0.0..cells as f32).contains(&x);
            if !inside {
                return Err(TextStoreError::PointOutside);
            }
            x.floor() as usize
        }
        PointMode::Nearest => x.round().clamp(0.0, cells as f32) as usize,
    };
    Ok(doc.offset_of_cell(cell))
}

impl TextStoreRead for ReadSession<'_> {
    fn document_len(&self) -> Utf16Offset {
        utf16::utf16_len(&self.doc.text)
    }

    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        read_text(self.doc, self.protected, range)
    }

    fn selection(&self) -> Selection {
        self.doc.selection
    }

    fn composition(&self) -> Option<Composition> {
        self.doc.composition
    }

    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        rect_for_range(self.doc, range)
    }

    fn document_bounds(&self) -> Result<Bounds<Pixels>, TextStoreError> {
        Ok(document_bounds(self.doc))
    }

    fn index_at_point(
        &self,
        point: Point<Pixels>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        index_at_point(self.doc, point, mode)
    }
}

impl TextStoreRead for EditSession<'_> {
    fn document_len(&self) -> Utf16Offset {
        utf16::utf16_len(&self.doc.text)
    }

    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        read_text(self.doc, self.protected, range)
    }

    fn selection(&self) -> Selection {
        self.doc.selection
    }

    fn composition(&self) -> Option<Composition> {
        self.doc.composition
    }

    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        rect_for_range(self.doc, range)
    }

    fn document_bounds(&self) -> Result<Bounds<Pixels>, TextStoreError> {
        Ok(document_bounds(self.doc))
    }

    fn index_at_point(
        &self,
        point: Point<Pixels>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        index_at_point(self.doc, point, mode)
    }
}

impl TextStoreEdit for EditSession<'_> {
    fn replace(&mut self, range: Utf16Range, text: &str) -> Result<TextChange, TextStoreError> {
        let change = self.doc.replace(range, text)?;
        self.edited = true;
        Ok(change)
    }

    fn insert_at_selection(&mut self, text: &str) -> Result<TextChange, TextStoreError> {
        let range = self.doc.selection.range();
        self.replace(range, text)
    }

    fn set_selection(&mut self, selection: Selection) -> Result<(), TextStoreError> {
        self.doc.check(selection.anchor)?;
        self.doc.check(selection.active)?;
        if self.doc.selection != selection {
            self.doc.selection = selection;
            self.edited = true;
        }
        Ok(())
    }

    fn set_composition(&mut self, composition: Option<Composition>) -> Result<(), TextStoreError> {
        if let Some(composition) = composition {
            self.doc.check(composition.range.start())?;
            self.doc.check(composition.range.end())?;
        }
        if self.doc.composition != composition {
            self.doc.composition = composition;
            self.edited = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static_assertions::assert_not_impl_any!(InMemoryTextStore: Send, Sync);

    fn at(units: usize) -> Utf16Offset {
        Utf16Offset::new(units)
    }

    fn range(start: usize, end: usize) -> Utf16Range {
        Utf16Range::new(at(start), at(end)).expect("ordered")
    }

    #[test]
    fn an_edit_before_the_composition_shifts_it_and_one_inside_clears_it() {
        let store = InMemoryTextStore::new("abcdef");
        let _ = store.request_lock(
            LockGrant::read_write(|session| {
                session
                    .set_composition(Some(Composition {
                        range: range(2, 4),
                        hides_caret: false,
                    }))
                    .expect("in range");
                session.replace(range(0, 1), "xyz").expect("in range");
            }),
            LockTiming::Sync,
        );
        assert_eq!(store.composition().map(|c| c.range), Some(range(4, 6)));
        let _ = store.request_lock(
            LockGrant::read_write(|session| {
                session.replace(range(5, 5), "!").expect("in range");
            }),
            LockTiming::Sync,
        );
        assert_eq!(store.composition(), None);
        assert_eq!(store.text(), "xyzbc!def");
    }

    #[test]
    fn cells_are_one_scalar_each() {
        let store = InMemoryTextStore::new("a😀b");
        let rect = Rc::new(Cell::new(None));
        let seen = Rc::clone(&rect);
        let _ = store.request_lock(
            LockGrant::read(move |session| seen.set(session.rect_for_range(range(1, 3)).ok())),
            LockTiming::Sync,
        );
        let bounds = rect.get().expect("laid out").bounds;
        assert_eq!(bounds.origin.x, px(InMemoryTextStore::ADVANCE));
        assert_eq!(bounds.size.width, px(InMemoryTextStore::ADVANCE));
    }
}
