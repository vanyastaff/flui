//! The journal of a window's text store host (ADR-0215 §4), in debug builds
//! with `native-capture`.
//!
//! [`JournaledHost`] stands between a window's store host and the
//! presentation, at the one place a host is handed out
//! (`OwnerPlatform::text_store_host`), so every pull-model host (TSF, AppKit,
//! UIKit, the Android and web mirrors, the Linux text inputs) journals with
//! the same code. It hands the host a [`JournaledStore`] in place of each
//! store it is told to focus, which journals each lock the input method is
//! granted as one `store_lock` row of the window's stream: the calls it made
//! and the store's answers, in order. The row is reported when the lock's body
//! returns, or unwinds (then marked incomplete), with no borrow held.
//!
//! Without a capture installed when the window opened, the host is handed out
//! as it is: nothing is wrapped and nothing is allocated.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::rc::Rc;

use flui_foundation::geometry::{Bounds, Point};
use flui_foundation::sequence::IdMint;
use flui_platform::text_store::{
    CommitGate, Composition, CompositionEnd, LockGrant, LockKind, LockOutcome, LockTiming,
    OwnerCalls, PointMode, RangeRect, Selection, TextChange, TextStore, TextStoreChange,
    TextStoreEdit, TextStoreError, TextStoreHost, TextStoreHostError, TextStoreObserver,
    TextStoreRead, TextStoreStatus, Utf16Offset, Utf16Range,
};

use super::capture::{Captured, Journal};
use super::store::{
    MAX_STORE_STEPS, MAX_STORE_TEXT_BYTES, RecordedStore, StoreDocument, StoreError, StoreFocus,
    StoreLock, StoreStep, error_kind,
};
use crate::owner::panic_boundary::contain_owner_callback;

/// What one window's text store journal holds on its owner thread, shared by
/// every journaled host handed out for the window, so two of them cannot
/// disagree about which store is focused or issue one key twice.
#[derive(Default)]
pub(crate) struct StoreJournal {
    /// The keys the window issues to the stores its input method serves, one
    /// per focus of a store it was not serving, never reissued in its
    /// recording.
    keys: IdMint<NonZeroU64>,
    /// The store the presentation last focused, and the journaled store the
    /// host serves in its place.
    focused: RefCell<Option<Focused>>,
}

/// A window's store host whose stores journal to the window's stream.
pub(crate) struct JournaledHost {
    inner: Rc<dyn TextStoreHost>,
    journal: Journal,
    state: Rc<StoreJournal>,
}

/// A store the presentation focused and the journaled store served for it.
/// The presentation's store is held strongly, so it cannot be freed and
/// another allocated at its address while the pair stands.
struct Focused {
    presented: Rc<dyn TextStore>,
    served: Rc<JournaledStore>,
}

impl JournaledHost {
    /// `inner` journaled to `journal`, sharing the window's `state`; `inner`
    /// itself when `journal` reports nowhere.
    pub(crate) fn wrap(
        inner: Rc<dyn TextStoreHost>,
        journal: &Journal,
        state: Rc<StoreJournal>,
    ) -> Rc<dyn TextStoreHost> {
        if !journal.is_reporting() {
            return inner;
        }
        Rc::new(Self {
            inner,
            journal: journal.clone(),
            state,
        })
    }

    /// The journaled store `store` is served through: the one standing for
    /// it if it is the focused store again, else a new one under a new key.
    /// `None` once the window's keys are exhausted, and then `store` is
    /// served unjournaled.
    fn journaled(&self, store: &Rc<dyn TextStore>) -> Option<Rc<JournaledStore>> {
        if let Some(focused) = &*self.state.focused.borrow()
            && Rc::ptr_eq(&focused.presented, store)
        {
            return Some(Rc::clone(&focused.served));
        }
        let Ok(raw) = self.state.keys.next() else {
            tracing::error!(
                "a window's text store keys are exhausted: its stores are not journaled"
            );
            return None;
        };
        let key = RecordedStore::from_raw(raw);
        self.journal.record(Captured::StoreFocus(Some(StoreFocus {
            store: key,
            status: store.status(),
        })));
        Some(Rc::new(JournaledStore {
            inner: Rc::clone(store),
            key,
            journal: self.journal.clone(),
            document: Rc::new(Cell::new(HeldDocument::Stale)),
        }))
    }
}

impl TextStoreHost for JournaledHost {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        let (pair, served): (_, Option<Rc<dyn TextStore>>) = match store {
            None => {
                if self.state.focused.borrow().is_some() {
                    self.journal.record(Captured::StoreFocus(None));
                }
                (None, None)
            }
            Some(store) => match self.journaled(&store) {
                Some(journaled) => {
                    let served: Rc<dyn TextStore> = journaled.clone();
                    let focused = Focused {
                        presented: store,
                        served: journaled,
                    };
                    (Some(focused), Some(served))
                }
                None => (None, Some(store)),
            },
        };
        // The previous pair is let go outside the borrow, and the host is
        // told with none held: it may reach application code.
        let replaced = self.state.focused.replace(pair);
        drop(replaced);
        self.inner.focus_store(served);
    }

    fn complete_composition(
        &self,
        store: &Rc<dyn TextStore>,
    ) -> Result<CompositionEnd, TextStoreHostError> {
        let served: Rc<dyn TextStore> = match &*self.state.focused.borrow() {
            Some(focused) if Rc::ptr_eq(&focused.presented, store) => {
                Rc::clone(&focused.served) as Rc<dyn TextStore>
            }
            _ => Rc::clone(store),
        };
        self.inner.complete_composition(&served)
    }
}

/// What the recording holds of a journaled store's document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeldDocument {
    /// Nothing yet, or the application changed it since: the next lock that
    /// makes a recorded call records the document it found first.
    Stale,
    /// What the recorded locks left it as.
    Current,
    /// No observer reports the application's changes to it: every lock that
    /// makes a recorded call records the document it found first.
    Unobserved,
}

/// A store the presentation focused, served to the window's host in its
/// place: each lock granted on it journals one `store_lock` row.
pub(crate) struct JournaledStore {
    inner: Rc<dyn TextStore>,
    key: RecordedStore,
    journal: Journal,
    /// Shared with the observer it installs and the locks it grants.
    document: Rc<Cell<HeldDocument>>,
}

impl TextStore for JournaledStore {
    fn status(&self) -> TextStoreStatus {
        self.inner.status()
    }

    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        let tap = LockTap {
            key: self.key,
            journal: self.journal.clone(),
            document: Rc::clone(&self.document),
            store: Rc::downgrade(&self.inner),
        };
        let grant = match grant {
            LockGrant::Read(body) => LockGrant::read(move |session| {
                let Some(log) = tap.open(LockKind::Read) else {
                    return body(session);
                };
                body(&ReadTap { session, log: &log });
                log.finish();
            }),
            LockGrant::ReadWrite(body) => LockGrant::read_write(move |session| {
                let Some(log) = tap.open(LockKind::ReadWrite) else {
                    return body(session);
                };
                body(&mut EditTap { session, log: &log });
                log.finish();
            }),
        };
        self.inner.request_lock(grant, timing)
    }

    fn run_deferred_grants(&self) -> usize {
        self.inner.run_deferred_grants()
    }

    fn set_commit_gate(&self, gate: CommitGate) {
        self.inner.set_commit_gate(gate);
    }

    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        let observer = observer.map(|inner| {
            if self.document.get() == HeldDocument::Unobserved {
                self.document.set(HeldDocument::Stale);
            }
            Rc::new(ChangeWatch {
                inner,
                document: Rc::clone(&self.document),
            }) as Rc<dyn TextStoreObserver>
        });
        if observer.is_none() {
            self.document.set(HeldDocument::Unobserved);
        }
        self.inner.set_observer(observer);
    }

    fn screen_moved(&self) {
        self.inner.screen_moved();
    }
}

/// What a granted lock on a journaled store needs to journal itself.
struct LockTap {
    key: RecordedStore,
    journal: Journal,
    document: Rc<Cell<HeldDocument>>,
    store: std::rc::Weak<dyn TextStore>,
}

impl LockTap {
    /// The journal of a lock of `kind` granted now, or `None` when the lock
    /// runs untouched: no sink wants store rows now (the recording's copy of
    /// the document goes stale), or the store is protected, when the lock is
    /// recorded as having happened and nothing derived from the field's
    /// content is (no text, range, offset or length).
    fn open(self, kind: LockKind) -> Option<LockLog> {
        if !self.journal.is_wanted() {
            self.forget_document();
            return None;
        }
        if self
            .store
            .upgrade()
            .is_some_and(|store| store.status().protected)
        {
            self.forget_document();
            let lock = StoreLock {
                store: self.key,
                kind,
                opened: None,
                steps: Vec::new(),
                incomplete: false,
                protected: true,
            };
            contain_owner_callback(|| self.journal.record(Captured::StoreLock(&lock)));
            return None;
        }
        Some(LockLog::open(self.key, kind, self.journal, self.document))
    }

    /// The recording no longer holds what this lock leaves the document as.
    fn forget_document(&self) {
        if self.document.get() == HeldDocument::Current {
            self.document.set(HeldDocument::Stale);
        }
    }
}

/// The platform's observer of a journaled store: an application change it
/// hears makes the recording's copy of the document stale.
struct ChangeWatch {
    inner: Rc<dyn TextStoreObserver>,
    document: Rc<Cell<HeldDocument>>,
}

impl ChangeWatch {
    fn changed(&self) {
        if self.document.get() == HeldDocument::Current {
            self.document.set(HeldDocument::Stale);
        }
    }
}

impl TextStoreObserver for ChangeWatch {
    fn publish_change(
        &self,
        change: TextStoreChange,
        publish: &mut dyn FnMut(&mut OwnerCalls),
        calls: &mut OwnerCalls,
    ) {
        self.changed();
        self.inner.publish_change(change, publish, calls);
    }

    fn text_changed(&self, change: TextChange) {
        self.changed();
        self.inner.text_changed(change);
    }

    fn selection_changed(&self) {
        self.changed();
        self.inner.selection_changed();
    }

    fn composition_changed(&self) {
        self.changed();
        self.inner.composition_changed();
    }

    fn layout_changed(&self) {
        self.inner.layout_changed();
    }

    fn status_changed(&self) {
        self.changed();
        self.inner.status_changed();
    }
}

/// The row one granted lock is building.
#[derive(Default)]
struct Entries {
    opened: Option<StoreDocument>,
    steps: Vec<StoreStep>,
    text_bytes: usize,
    incomplete: bool,
    /// The document has to be recorded before the next recorded call.
    document_due: bool,
}

/// One granted lock's journal: reported as a `store_lock` row when it is
/// dropped, at the end of the lock's body or while it unwinds.
struct LockLog {
    key: RecordedStore,
    kind: LockKind,
    journal: Journal,
    document: Rc<Cell<HeldDocument>>,
    entries: RefCell<Entries>,
    finished: Cell<bool>,
}

impl LockLog {
    fn open(
        key: RecordedStore,
        kind: LockKind,
        journal: Journal,
        document: Rc<Cell<HeldDocument>>,
    ) -> Self {
        let document_due = document.get() != HeldDocument::Current;
        Self {
            key,
            kind,
            journal,
            document,
            entries: RefCell::new(Entries {
                document_due,
                ..Entries::default()
            }),
            finished: Cell::new(false),
        }
    }

    /// The lock's body returned.
    fn finish(self) {
        self.finished.set(true);
    }

    /// Before a recorded call: the document as the lock found it, if the
    /// recording does not hold it and this is the lock's first such call.
    fn before_call(&self, session: &dyn TextStoreRead) {
        if !std::mem::take(&mut self.entries.borrow_mut().document_due) {
            return;
        }
        let len = session.document_len();
        // Every UTF-16 unit is at least one byte of UTF-8: a document of more
        // units than the row holds bytes is never read, let alone copied.
        if len.get() > MAX_STORE_TEXT_BYTES {
            self.entries.borrow_mut().incomplete = true;
            return;
        }
        let whole = Utf16Range::try_new(Utf16Offset::ZERO, len)
            .expect("BUG: a document's length is at or past its start");
        let text = session.text(whole);
        let (selection, composition) = (session.selection(), session.composition());
        let mut entries = self.entries.borrow_mut();
        match text {
            Ok(text) if text.len() <= MAX_STORE_TEXT_BYTES => {
                entries.text_bytes = text.len();
                entries.opened = Some(StoreDocument {
                    text,
                    selection,
                    composition,
                });
            }
            // Too long to hold, or not to be read out (a protected field).
            Ok(_) | Err(_) => entries.incomplete = true,
        }
    }

    /// Record a call carrying `text_bytes` of text, which `step` builds, or
    /// mark the lock incomplete when the row cannot hold it or the schema
    /// does not mirror its answer (`step` gives `None`).
    fn record(&self, text_bytes: usize, step: impl FnOnce() -> Option<StoreStep>) {
        let mut entries = self.entries.borrow_mut();
        let total = entries.text_bytes.checked_add(text_bytes).filter(|total| {
            *total <= MAX_STORE_TEXT_BYTES && entries.steps.len() < MAX_STORE_STEPS
        });
        match total.and_then(|total| Some((total, step()?))) {
            Some((total, step)) => {
                entries.text_bytes = total;
                entries.steps.push(step);
            }
            None => entries.incomplete = true,
        }
    }

    fn document_len(&self, session: &dyn TextStoreRead) -> Utf16Offset {
        self.before_call(session);
        let len = session.document_len();
        self.record(0, || Some(StoreStep::DocumentLen(len)));
        len
    }

    fn text(
        &self,
        session: &dyn TextStoreRead,
        range: Utf16Range,
    ) -> Result<String, TextStoreError> {
        self.before_call(session);
        let answer = session.text(range);
        let bytes = answer.as_ref().map_or(0, String::len);
        self.record(bytes, || {
            Some(StoreStep::Text {
                range,
                answer: kept(&answer)?,
            })
        });
        answer
    }

    fn selection(&self, session: &dyn TextStoreRead) -> Selection {
        self.before_call(session);
        let selection = session.selection();
        self.record(0, || Some(StoreStep::Selection(selection)));
        selection
    }

    fn composition(&self, session: &dyn TextStoreRead) -> Option<Composition> {
        self.before_call(session);
        let composition = session.composition();
        self.record(0, || Some(StoreStep::Composition(composition)));
        composition
    }

    fn index_at_point(
        &self,
        session: &dyn TextStoreRead,
        point: Point<f64>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        self.before_call(session);
        let answer = session.index_at_point(point, mode);
        self.record(0, || {
            Some(StoreStep::IndexAtPoint {
                point,
                mode,
                answer: kept(&answer)?,
            })
        });
        answer
    }
}

impl Drop for LockLog {
    fn drop(&mut self) {
        let entries = self.entries.take();
        if entries.steps.is_empty() && entries.opened.is_none() && !entries.incomplete {
            // Geometry reads only, or none: nothing in the store depends on
            // them.
            return;
        }
        let incomplete = entries.incomplete || !self.finished.get();
        if self.document.get() != HeldDocument::Unobserved {
            self.document.set(if incomplete {
                HeldDocument::Stale
            } else {
                HeldDocument::Current
            });
        }
        let lock = StoreLock {
            store: self.key,
            kind: self.kind,
            opened: entries.opened,
            steps: entries.steps,
            incomplete,
            protected: false,
        };
        // A sink that panics ends its capture; neither the lock's body nor an
        // unwind already under way is disturbed by it.
        contain_owner_callback(|| self.journal.record(Captured::StoreLock(&lock)));
    }
}

/// `answer` as a recording keeps it; `None` for a refusal the schema does
/// not mirror.
fn kept<T: Clone>(answer: &Result<T, TextStoreError>) -> Option<Result<T, StoreError>> {
    match answer {
        Ok(value) => Some(Ok(value.clone())),
        Err(error) => error_kind(error).map(Err),
    }
}

/// A read lock's session as the input method sees it, journaling each call.
struct ReadTap<'a> {
    session: &'a dyn TextStoreRead,
    log: &'a LockLog,
}

impl TextStoreRead for ReadTap<'_> {
    fn document_len(&self) -> Utf16Offset {
        self.log.document_len(self.session)
    }

    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        self.log.text(self.session, range)
    }

    fn selection(&self) -> Selection {
        self.log.selection(self.session)
    }

    fn composition(&self) -> Option<Composition> {
        self.log.composition(self.session)
    }

    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        self.session.rect_for_range(range)
    }

    fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
        self.session.document_bounds()
    }

    fn index_at_point(
        &self,
        point: Point<f64>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        self.log.index_at_point(self.session, point, mode)
    }
}

/// A read-write lock's session as the input method sees it, journaling each
/// call.
struct EditTap<'a> {
    session: &'a mut dyn TextStoreEdit,
    log: &'a LockLog,
}

impl TextStoreRead for EditTap<'_> {
    fn document_len(&self) -> Utf16Offset {
        self.log.document_len(&*self.session)
    }

    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        self.log.text(&*self.session, range)
    }

    fn selection(&self) -> Selection {
        self.log.selection(&*self.session)
    }

    fn composition(&self) -> Option<Composition> {
        self.log.composition(&*self.session)
    }

    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        self.session.rect_for_range(range)
    }

    fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
        self.session.document_bounds()
    }

    fn index_at_point(
        &self,
        point: Point<f64>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        self.log.index_at_point(&*self.session, point, mode)
    }
}

impl TextStoreEdit for EditTap<'_> {
    fn replace(&mut self, range: Utf16Range, text: &str) -> Result<TextChange, TextStoreError> {
        self.log.before_call(&*self.session);
        let answer = self.session.replace(range, text);
        self.log.record(text.len(), || {
            Some(StoreStep::Replace {
                range,
                text: text.to_owned(),
                answer: kept(&answer)?,
            })
        });
        answer
    }

    fn insert_at_selection(&mut self, text: &str) -> Result<TextChange, TextStoreError> {
        self.log.before_call(&*self.session);
        let answer = self.session.insert_at_selection(text);
        self.log.record(text.len(), || {
            Some(StoreStep::InsertAtSelection {
                text: text.to_owned(),
                answer: kept(&answer)?,
            })
        });
        answer
    }

    fn set_selection(&mut self, selection: Selection) -> Result<(), TextStoreError> {
        self.log.before_call(&*self.session);
        let answer = self.session.set_selection(selection);
        self.log.record(0, || {
            Some(StoreStep::SetSelection {
                selection,
                answer: kept(&answer)?,
            })
        });
        answer
    }

    fn set_composition(&mut self, composition: Option<Composition>) -> Result<(), TextStoreError> {
        self.log.before_call(&*self.session);
        let answer = self.session.set_composition(composition);
        self.log.record(0, || {
            Some(StoreStep::SetComposition {
                composition,
                answer: kept(&answer)?,
            })
        });
        answer
    }
}
