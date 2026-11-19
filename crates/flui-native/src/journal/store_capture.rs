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

use flui_foundation::IdMint;
use flui_foundation::geometry::{Bounds, Point};
use flui_foundation::panic::FirstPanic;
use flui_platform::text_store::{
    CommitGate, Composition, CompositionEnd, EditCommand, LockGrant, LockKind, LockOutcome,
    LockTiming, OwnerCalls, PointMode, PressTarget, RangeRect, Selection, SelectionUi, TextChange,
    TextStore, TextStoreChange, TextStoreEdit, TextStoreError, TextStoreHost, TextStoreHostError,
    TextStoreObserver, TextStoreRead, TextStoreStatus, Utf16Offset, Utf16Range,
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
    /// `None` once the window's keys are exhausted, or when the focus row
    /// could not be journaled (the store's status or the sink panicked, which
    /// `failure` keeps), and then `store` is served unjournaled.
    fn journaled(
        &self,
        store: &Rc<dyn TextStore>,
        failure: &mut FirstPanic,
    ) -> Option<Rc<JournaledStore>> {
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
        failure.contain(|| {
            self.journal.record(Captured::StoreFocus(Some(StoreFocus {
                store: key,
                status: store.status(),
            })));
        })?;
        Some(Rc::new(JournaledStore {
            inner: Rc::clone(store),
            key,
            journal: self.journal.clone(),
            document: Rc::new(Cell::new(HeldDocument::Stale)),
            held: Rc::default(),
            protected_noted: Rc::default(),
        }))
    }
}

impl TextStoreHost for JournaledHost {
    /// The host is told whatever the journal's calls do: what they run of
    /// code the journal does not own (the store's status, the sink, the drop
    /// of the store let go) is contained, and the first failure is raised
    /// once the host was told.
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        let mut failure = FirstPanic::new();
        let (pair, served): (_, Option<Rc<dyn TextStore>>) = match store {
            None => {
                if self.state.focused.borrow().is_some() {
                    failure.contain(|| self.journal.record(Captured::StoreFocus(None)));
                }
                (None, None)
            }
            Some(store) => match self.journaled(&store, &mut failure) {
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
        failure.contain(|| drop(replaced));
        self.inner.focus_store(served);
        failure.resume();
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

    fn show_keyboard(&self, store: &Rc<dyn TextStore>) {
        let served: Rc<dyn TextStore> = match &*self.state.focused.borrow() {
            Some(focused) if Rc::ptr_eq(&focused.presented, store) => {
                Rc::clone(&focused.served) as Rc<dyn TextStore>
            }
            _ => Rc::clone(store),
        };
        self.inner.show_keyboard(&served);
    }

    fn selection_ui(&self) -> SelectionUi {
        self.inner.selection_ui()
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
    /// The document the recording holds the store at, when known: what the
    /// last lock found and left unchanged. A lock that finds it again does
    /// not write it again.
    held: Rc<RefCell<Option<StoreDocument>>>,
    /// A lock on the store while protected was recorded; until a lock finds it
    /// unprotected, later ones are not.
    protected_noted: Rc<Cell<bool>>,
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
            held: Rc::clone(&self.held),
            protected_noted: Rc::clone(&self.protected_noted),
            store: Rc::downgrade(&self.inner),
        };
        // Every lock that runs unrecorded, for whatever reason, leaves the
        // recording without the document it leaves behind.
        let forget = Unrecorded {
            document: Rc::clone(&self.document),
            held: Rc::clone(&self.held),
        };
        // The body the input method was granted runs whatever the journal
        // does: what the tap runs of code it does not own (the store's status)
        // is contained, the first failure is kept, and it is raised once the
        // body ran.
        let grant = match grant {
            LockGrant::Read(body) => LockGrant::read(move |session| {
                let mut failure = FirstPanic::new();
                match failure.contain(|| tap.open(LockKind::Read)).flatten() {
                    None => {
                        forget.forget();
                        failure.contain(|| body(session));
                    }
                    Some(log) => {
                        if failure
                            .contain(|| body(&ReadTap { session, log: &log }))
                            .is_some()
                        {
                            log.finish();
                        }
                    }
                }
                failure.resume();
            }),
            LockGrant::ReadWrite(body) => LockGrant::read_write(move |session| {
                let mut failure = FirstPanic::new();
                match failure.contain(|| tap.open(LockKind::ReadWrite)).flatten() {
                    None => {
                        forget.forget();
                        failure.contain(|| body(session));
                    }
                    Some(log) => {
                        if failure
                            .contain(|| body(&mut EditTap { session, log: &log }))
                            .is_some()
                        {
                            log.finish();
                        }
                    }
                }
                failure.resume();
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

    /// Unrecorded: an edit command's effect on the document reaches the
    /// recording as the application edit it is.
    fn edit_command_enabled(&self, command: EditCommand) -> bool {
        self.inner.edit_command_enabled(command)
    }

    fn perform_edit_command(&self, command: EditCommand) -> bool {
        self.inner.perform_edit_command(command)
    }
}

/// What a granted lock on a journaled store needs to journal itself.
struct LockTap {
    key: RecordedStore,
    journal: Journal,
    document: Rc<Cell<HeldDocument>>,
    held: Rc<RefCell<Option<StoreDocument>>>,
    protected_noted: Rc<Cell<bool>>,
    store: std::rc::Weak<dyn TextStore>,
}

impl LockTap {
    /// The journal of a lock of `kind` granted now, or `None` when the lock
    /// runs untouched: no sink wants store rows now (the recording's copy of
    /// the document goes stale), or the store holds a secret
    /// ([`TextStoreStatus::holds_secret`]: protected, or a password or a
    /// one-time code shown in clear). Such a
    /// store's locks are recorded once, as a lock on a protected store and
    /// nothing derived from the field's content (no text, range, offset or
    /// length), until a lock finds it unprotected again: how many locks it was
    /// granted would count the keys typed into it.
    ///
    /// Reads the store's status, which is the application's code: the caller
    /// contains it.
    fn open(self, kind: LockKind) -> Option<LockLog> {
        if !self.journal.is_wanted() {
            self.forget_document();
            return None;
        }
        let secret = self
            .store
            .upgrade()
            .is_some_and(|store| store.status().holds_secret());
        if secret {
            self.forget_document();
            if !self.protected_noted.replace(true) {
                let lock = StoreLock {
                    store: self.key,
                    kind: LockKind::ReadWrite,
                    opened: None,
                    steps: Vec::new(),
                    incomplete: false,
                    protected: true,
                };
                contain_owner_callback(|| self.journal.record(Captured::StoreLock(&lock)));
            }
            return None;
        }
        self.protected_noted.set(false);
        Some(LockLog::open(
            self.key,
            kind,
            self.journal,
            self.document,
            self.held,
        ))
    }

    /// The recording no longer holds what this lock leaves the document as.
    fn forget_document(&self) {
        forget_document(&self.document, &self.held);
    }
}

/// What a lock that runs unrecorded (no sink wants it, the store is
/// protected, or the journal's own calls failed) has to forget.
struct Unrecorded {
    document: Rc<Cell<HeldDocument>>,
    held: Rc<RefCell<Option<StoreDocument>>>,
}

impl Unrecorded {
    fn forget(&self) {
        forget_document(&self.document, &self.held);
    }
}

/// The one place a store's journal forgets the document it held: the next
/// lock that makes a recorded call records the document it finds.
fn forget_document(document: &Cell<HeldDocument>, held: &RefCell<Option<StoreDocument>>) {
    if document.get() == HeldDocument::Current {
        document.set(HeldDocument::Stale);
    }
    held.replace(None);
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

    fn field_moved(&self) {
        self.inner.field_moved();
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
    held: Rc<RefCell<Option<StoreDocument>>>,
    entries: RefCell<Entries>,
    finished: Cell<bool>,
}

impl LockLog {
    fn open(
        key: RecordedStore,
        kind: LockKind,
        journal: Journal,
        document: Rc<Cell<HeldDocument>>,
        held: Rc<RefCell<Option<StoreDocument>>>,
    ) -> Self {
        let document_due = document.get() != HeldDocument::Current;
        Self {
            key,
            kind,
            journal,
            document,
            held,
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
                let found = StoreDocument {
                    text,
                    selection,
                    composition,
                };
                // The recording holds the store at this document already: a
                // store nobody observes would otherwise write it on every lock.
                if self.held.borrow().as_ref() != Some(&found) {
                    entries.text_bytes = found.text.len();
                    entries.opened = Some(found);
                }
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
        // What the recording holds the store at after this lock: what it found,
        // if it changed nothing; unknown once it edited or lost a call.
        if incomplete || entries.steps.iter().any(StoreStep::is_edit) {
            self.held.replace(None);
        } else if let Some(found) = &entries.opened {
            self.held.replace(Some(found.clone()));
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

    fn visible_bounds(&self) -> Result<Option<Bounds<f64>>, TextStoreError> {
        self.session.visible_bounds()
    }

    fn press_target_at(&self, point: Point<f64>) -> Result<PressTarget, TextStoreError> {
        self.session.press_target_at(point)
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

    fn visible_bounds(&self) -> Result<Option<Bounds<f64>>, TextStoreError> {
        self.session.visible_bounds()
    }

    fn press_target_at(&self, point: Point<f64>) -> Result<PressTarget, TextStoreError> {
        self.session.press_target_at(point)
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
