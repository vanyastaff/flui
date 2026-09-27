//! The conformance kit against the reference store, and against stores
//! with one fault each: the kit must pass the first and fail every other,
//! naming the case that caught the fault. A kit that cannot fail proves
//! nothing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_platform_api::text_store::{
    CommitGate, Composition, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, PointMode,
    RangeRect, Selection, TextChange, TextStore, TextStoreEdit, TextStoreError, TextStoreObserver,
    TextStoreRead, TextStoreStatus, Utf16Offset, Utf16Range, utf16,
};
use flui_testing::text_store_kit::{
    self, FixtureCapabilities, InMemoryFixture, KIT_VERSION, TextStoreFixture,
};
use flui_types::geometry::{Bounds, Pixels, Point};

#[test]
fn in_memory_store_conforms_to_kit_v1() {
    text_store_kit::assert_conforms(&mut InMemoryFixture::new(), KIT_VERSION);
}

#[test]
fn a_protected_in_memory_store_conforms_to_kit_v1() {
    text_store_kit::assert_conforms(&mut InMemoryFixture::protected(), KIT_VERSION);
}

#[test]
fn version_one_runs_every_case() {
    let cases = text_store_kit::cases();
    assert_eq!(cases.len(), 25);
    assert!(cases.iter().all(|case| case.since == 1));
}

/// One deliberate defect a store might have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    /// Reports its length in UTF-8 bytes.
    CountsUtf8Bytes,
    /// Ignores the commit gate it is handed and grants at once.
    GrantsInsideTransaction,
    /// Tells the observer about the platform's own edits.
    EchoesPlatformEdits,
    /// Moves a platform selection inside a grapheme to the grapheme's end.
    SnapsPlatformSelectionToGraphemes,
    /// Answers a point in the right half of a supplementary scalar with the
    /// offset between its surrogates.
    SplitsSurrogatesOnHitTest,
    /// Notifies its owner once per edit rather than once per session.
    NotifiesPerEdit,
    /// Tells the observer of an app edit at once, inside a frame
    /// transaction too.
    NotifiesInsideTransaction,
}

/// `InMemoryTextStore` with `fault` spliced into its sessions.
struct Faulty {
    inner: Rc<InMemoryTextStore>,
    fault: Fault,
    observer: Rc<RefCell<Option<Rc<dyn TextStoreObserver>>>>,
    edits: Rc<Cell<usize>>,
}

impl TextStore for Faulty {
    fn status(&self) -> TextStoreStatus {
        self.inner.status()
    }

    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        let fault = self.fault;
        let wrapped = match grant {
            LockGrant::Read(body) => LockGrant::read(move |session| {
                body(&ReadFault {
                    inner: session,
                    fault,
                });
            }),
            LockGrant::ReadWrite(body) => {
                let (observer, edits) = (Rc::clone(&self.observer), Rc::clone(&self.edits));
                LockGrant::read_write(move |session| {
                    body(&mut EditFault {
                        inner: session,
                        fault,
                        observer,
                        edits,
                    });
                })
            }
        };
        self.inner.request_lock(wrapped, timing)
    }

    fn run_deferred_grants(&self) -> usize {
        self.inner.run_deferred_grants()
    }

    fn set_commit_gate(&self, gate: CommitGate) {
        if self.fault != Fault::GrantsInsideTransaction {
            self.inner.set_commit_gate(gate);
        }
    }

    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.observer.borrow_mut().clone_from(&observer);
        self.inner.set_observer(observer);
    }
}

fn whole_text(session: &dyn TextStoreRead) -> String {
    let whole = Utf16Range::new(Utf16Offset::ZERO, session.document_len())
        .expect("zero precedes every length");
    session.text(whole).unwrap_or_default()
}

fn faulty_len(session: &dyn TextStoreRead, fault: Fault) -> Utf16Offset {
    if fault == Fault::CountsUtf8Bytes {
        Utf16Offset::new(whole_text(session).len())
    } else {
        session.document_len()
    }
}

fn faulty_index(
    session: &dyn TextStoreRead,
    fault: Fault,
    point: Point<Pixels>,
    mode: PointMode,
) -> Result<Utf16Offset, TextStoreError> {
    let found = session.index_at_point(point, mode)?;
    if fault != Fault::SplitsSurrogatesOnHitTest {
        return Ok(found);
    }
    let pair = Utf16Range::new(found, Utf16Offset::new(found.get() + 2)).expect("ascending");
    let Ok(text) = session.text(pair) else {
        return Ok(found);
    };
    let supplementary = text.chars().count() == 1;
    let rect = session.rect_for_range(pair)?.bounds;
    let middle = rect.origin.x.get() + rect.size.width.get() / 2.0;
    if supplementary && point.x.get() > middle {
        Ok(Utf16Offset::new(found.get() + 1))
    } else {
        Ok(found)
    }
}

struct ReadFault<'a> {
    inner: &'a dyn TextStoreRead,
    fault: Fault,
}

impl TextStoreRead for ReadFault<'_> {
    fn document_len(&self) -> Utf16Offset {
        faulty_len(self.inner, self.fault)
    }
    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        self.inner.text(range)
    }
    fn selection(&self) -> Selection {
        self.inner.selection()
    }
    fn composition(&self) -> Option<Composition> {
        self.inner.composition()
    }
    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        self.inner.rect_for_range(range)
    }
    fn document_bounds(&self) -> Result<Bounds<Pixels>, TextStoreError> {
        self.inner.document_bounds()
    }
    fn index_at_point(
        &self,
        point: Point<Pixels>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        faulty_index(self.inner, self.fault, point, mode)
    }
}

struct EditFault<'a> {
    inner: &'a mut dyn TextStoreEdit,
    fault: Fault,
    observer: Rc<RefCell<Option<Rc<dyn TextStoreObserver>>>>,
    edits: Rc<Cell<usize>>,
}

impl EditFault<'_> {
    fn edited(&self, change: Option<TextChange>) {
        self.edits.set(self.edits.get() + 1);
        if self.fault == Fault::EchoesPlatformEdits {
            let observer = self.observer.borrow().clone();
            if let (Some(observer), Some(change)) = (observer, change) {
                observer.text_changed(change);
            }
        }
    }

    /// `offset` moved past a combining mark that follows it.
    fn snapped(&self, offset: Utf16Offset) -> Utf16Offset {
        let text = whole_text(&*self.inner);
        let Ok(byte) = utf16::byte_offset(&text, offset) else {
            return offset;
        };
        match text[byte..].chars().next() {
            Some('\u{300}'..='\u{36f}') => Utf16Offset::new(offset.get() + 1),
            _ => offset,
        }
    }
}

impl TextStoreRead for EditFault<'_> {
    fn document_len(&self) -> Utf16Offset {
        faulty_len(&*self.inner, self.fault)
    }
    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        self.inner.text(range)
    }
    fn selection(&self) -> Selection {
        self.inner.selection()
    }
    fn composition(&self) -> Option<Composition> {
        self.inner.composition()
    }
    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        self.inner.rect_for_range(range)
    }
    fn document_bounds(&self) -> Result<Bounds<Pixels>, TextStoreError> {
        self.inner.document_bounds()
    }
    fn index_at_point(
        &self,
        point: Point<Pixels>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        faulty_index(&*self.inner, self.fault, point, mode)
    }
}

impl TextStoreEdit for EditFault<'_> {
    fn replace(&mut self, range: Utf16Range, text: &str) -> Result<TextChange, TextStoreError> {
        let change = self.inner.replace(range, text)?;
        self.edited(Some(change));
        Ok(change)
    }

    fn insert_at_selection(&mut self, text: &str) -> Result<TextChange, TextStoreError> {
        let change = self.inner.insert_at_selection(text)?;
        self.edited(Some(change));
        Ok(change)
    }

    fn set_selection(&mut self, selection: Selection) -> Result<(), TextStoreError> {
        let selection = if self.fault == Fault::SnapsPlatformSelectionToGraphemes {
            Selection {
                anchor: self.snapped(selection.anchor),
                active: self.snapped(selection.active),
            }
        } else {
            selection
        };
        self.inner.set_selection(selection)?;
        self.edited(None);
        Ok(())
    }

    fn set_composition(&mut self, composition: Option<Composition>) -> Result<(), TextStoreError> {
        self.inner.set_composition(composition)?;
        self.edited(None);
        Ok(())
    }
}

struct FaultyFixture {
    store: Rc<Faulty>,
}

impl FaultyFixture {
    fn new(fault: Fault) -> Self {
        Self {
            store: Rc::new(Faulty {
                inner: InMemoryTextStore::new(""),
                fault,
                observer: Rc::new(RefCell::new(None)),
                edits: Rc::new(Cell::new(0)),
            }),
        }
    }
}

impl TextStoreFixture for FaultyFixture {
    fn store(&mut self) -> Rc<dyn TextStore> {
        let store: Rc<dyn TextStore> = self.store.clone(); // the kit drives the faulty wrapper through the erased contract.
        store
    }

    fn reset(&mut self, text: &str) {
        self.store = Rc::new(Faulty {
            inner: InMemoryTextStore::new(text),
            fault: self.store.fault,
            observer: Rc::new(RefCell::new(None)),
            edits: Rc::new(Cell::new(0)),
        });
    }

    fn app_replace_all(&mut self, text: &str) {
        let inner = &self.store.inner;
        let whole = Utf16Range::new(Utf16Offset::ZERO, utf16::utf16_len(&inner.text()))
            .expect("zero precedes every length");
        if self.store.fault != Fault::NotifiesInsideTransaction {
            inner.app_replace(whole, text);
            return;
        }
        // Edit with the inner store's observer unset, then tell the
        // platform's observer at once, whatever the gate says.
        let observer = self.store.observer.borrow().clone();
        inner.set_observer(None);
        inner.app_replace(whole, text);
        inner.set_observer(observer.clone());
        if let Some(observer) = observer {
            observer.text_changed(TextChange {
                start: Utf16Offset::ZERO,
                old_end: whole.end(),
                new_end: utf16::utf16_len(text),
            });
            observer.selection_changed();
        }
    }

    fn pump(&mut self) {
        let _ = self.store.run_deferred_grants();
    }

    fn owner_notifications(&self) -> usize {
        if self.store.fault == Fault::NotifiesPerEdit {
            self.store.edits.get()
        } else {
            self.store.inner.owner_notifications()
        }
    }

    fn capabilities(&self) -> FixtureCapabilities {
        FixtureCapabilities::new().with_geometry(true)
    }
}

#[track_caller]
fn assert_kit_catches(fault: Fault, case: &str) {
    let failures = text_store_kit::run(&mut FaultyFixture::new(fault), KIT_VERSION);
    assert!(
        failures.iter().any(|failure| failure.case == case),
        "{fault:?} should fail `{case}`; the kit reported {failures:#?}"
    );
}

#[test]
fn kit_fails_a_store_that_counts_utf8_bytes() {
    assert_kit_catches(Fault::CountsUtf8Bytes, "length_counts_utf16_units");
}

#[test]
fn kit_fails_a_store_that_grants_inside_a_transaction() {
    assert_kit_catches(
        Fault::GrantsInsideTransaction,
        "sync_request_inside_a_transaction_is_refused",
    );
    assert_kit_catches(
        Fault::GrantsInsideTransaction,
        "async_request_inside_a_transaction_waits_for_the_next_anchor",
    );
}

#[test]
fn kit_fails_a_store_that_echoes_platform_edits() {
    assert_kit_catches(
        Fault::EchoesPlatformEdits,
        "platform_edits_are_not_echoed_to_the_observer",
    );
}

#[test]
fn kit_fails_a_store_that_snaps_platform_selection_to_graphemes() {
    assert_kit_catches(
        Fault::SnapsPlatformSelectionToGraphemes,
        "selection_inside_a_grapheme_is_kept_exactly",
    );
}

#[test]
fn kit_fails_a_store_that_splits_surrogates_on_hit_test() {
    assert_kit_catches(
        Fault::SplitsSurrogatesOnHitTest,
        "index_at_point_never_splits_a_surrogate_pair",
    );
}

#[test]
fn kit_fails_a_store_that_notifies_per_edit() {
    assert_kit_catches(
        Fault::NotifiesPerEdit,
        "one_session_is_one_owner_notification",
    );
}

#[test]
fn kit_fails_a_store_that_notifies_inside_a_transaction() {
    assert_kit_catches(
        Fault::NotifiesInsideTransaction,
        "app_edits_inside_a_transaction_reach_the_observer_after_it",
    );
}
