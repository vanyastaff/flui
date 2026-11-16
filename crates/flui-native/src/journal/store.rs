//! What a window's input method did to a text store, as a native recording
//! holds it (ADR-0215 §4), and its replay on another store.
//!
//! A pull-model input method (TSF, AppKit, UIKit, the Android and web
//! mirrors, the Linux text inputs) reads and edits the focused field's store
//! inside locks. One [`StoreLock`] is one granted lock: the calls made in it
//! in order, each with the store's answer, and the document the lock found
//! when the recording did not already hold it. Replaying the locks of a
//! recording in order on a store that starts from the same document leaves it
//! with the same text, selection and composition, and every read the input
//! method based an edit on answers as it did when it was recorded; a read that
//! answers otherwise is a [`StoreDivergence`], never a silent difference.
//!
//! Geometry reads (a range's rect, the document's bounds) only place the input
//! method's window and depend on the device's layout: they are not recorded.
//! An offset asked at a point is recorded with its answer, and a replay keeps
//! the answer instead of asking again, since the edit that followed carries
//! the offset itself.

use std::cell::RefCell;
use std::num::NonZeroU64;
use std::rc::Rc;

use flui_foundation::geometry::Point;
use flui_platform::text_store::{
    Composition, LockGrant, LockKind, LockOutcome, LockTiming, PointMode, Selection, TextChange,
    TextInputPurpose, TextStore, TextStoreEdit, TextStoreError, TextStoreRead, TextStoreStatus,
    Utf16Offset, Utf16Range,
};
use flui_protocol::native_recording as wire;

use super::codec::{DtoError, DtoResult, invalid, unread};

pub use wire::{MAX_STORE_STEPS, MAX_STORE_TEXT_BYTES, StoreError};

/// A text store within one recording: issued each time the window is told to
/// serve a store it was not serving, never zero and never issued twice in a
/// recording, so a lock deferred past a focus change still names the store it
/// ran on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordedStore(NonZeroU64);

impl RecordedStore {
    /// The store a recording names by `raw`.
    #[must_use]
    pub const fn from_raw(raw: NonZeroU64) -> Self {
        Self(raw)
    }

    /// Its number within the recording.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// The store a window's input method was told to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreFocus {
    /// The store.
    pub store: RecordedStore,
    /// Its standing properties when it was focused.
    pub status: TextStoreStatus,
}

/// A store's document: its text, selection and composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreDocument {
    /// The text.
    pub text: String,
    /// The selection.
    pub selection: Selection,
    /// The composition, when one is in progress.
    pub composition: Option<Composition>,
}

/// One call an input method made inside a lock, with the store's answer; a
/// refusal is kept by its kind.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StoreStep {
    /// [`TextStoreRead::document_len`].
    DocumentLen(Utf16Offset),
    /// [`TextStoreRead::text`].
    Text {
        /// The range read.
        range: Utf16Range,
        /// The answer.
        answer: Result<String, StoreError>,
    },
    /// [`TextStoreRead::selection`].
    Selection(Selection),
    /// [`TextStoreRead::composition`].
    Composition(Option<Composition>),
    /// [`TextStoreRead::index_at_point`]: kept, never asked again on replay.
    IndexAtPoint {
        /// The point, in window-root logical pixels.
        point: Point<f64>,
        /// How the point was resolved.
        mode: PointMode,
        /// The answer.
        answer: Result<Utf16Offset, StoreError>,
    },
    /// [`TextStoreEdit::replace`].
    Replace {
        /// The range replaced.
        range: Utf16Range,
        /// The text put there.
        text: String,
        /// The answer.
        answer: Result<TextChange, StoreError>,
    },
    /// [`TextStoreEdit::insert_at_selection`].
    InsertAtSelection {
        /// The text put there.
        text: String,
        /// The answer.
        answer: Result<TextChange, StoreError>,
    },
    /// [`TextStoreEdit::set_selection`].
    SetSelection {
        /// The selection asked for.
        selection: Selection,
        /// The answer.
        answer: Result<(), StoreError>,
    },
    /// [`TextStoreEdit::set_composition`].
    SetComposition {
        /// The composition asked for.
        composition: Option<Composition>,
        /// The answer.
        answer: Result<(), StoreError>,
    },
}

impl StoreStep {
    /// The call's name in the schema.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::DocumentLen(_) => "document_len",
            Self::Text { .. } => "text",
            Self::Selection(_) => "selection",
            Self::Composition(_) => "composition",
            Self::IndexAtPoint { .. } => "index_at_point",
            Self::Replace { .. } => "replace",
            Self::InsertAtSelection { .. } => "insert_at_selection",
            Self::SetSelection { .. } => "set_selection",
            Self::SetComposition { .. } => "set_composition",
        }
    }

    /// Whether the call edits, which only a read-write lock may.
    #[must_use]
    pub const fn is_edit(&self) -> bool {
        matches!(
            self,
            Self::Replace { .. }
                | Self::InsertAtSelection { .. }
                | Self::SetSelection { .. }
                | Self::SetComposition { .. }
        )
    }

    /// The bytes of text the call carries, which count against
    /// [`MAX_STORE_TEXT_BYTES`].
    #[must_use]
    pub fn text_bytes(&self) -> usize {
        match self {
            Self::Text {
                answer: Ok(text), ..
            }
            | Self::Replace { text, .. }
            | Self::InsertAtSelection { text, .. } => text.len(),
            _ => 0,
        }
    }

    /// The call with its text replaced by `with`, and whether it carried any.
    #[must_use]
    pub fn redacted(&self, with: &str) -> (Self, bool) {
        // Built anew, so the text it replaces is never copied.
        let step = match self {
            Self::Text {
                range,
                answer: Ok(_),
            } => Self::Text {
                range: *range,
                answer: Ok(with.to_owned()),
            },
            Self::Replace { range, answer, .. } => Self::Replace {
                range: *range,
                text: with.to_owned(),
                answer: *answer,
            },
            Self::InsertAtSelection { answer, .. } => Self::InsertAtSelection {
                text: with.to_owned(),
                answer: *answer,
            },
            other => return (other.clone(), false),
        };
        (step, true)
    }
}

/// One lock an input method was granted on a store, and its calls in order.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreLock {
    /// The store.
    pub store: RecordedStore,
    /// The lock's kind.
    pub kind: LockKind,
    /// The document as the lock found it, when the recording did not hold it
    /// yet.
    pub opened: Option<StoreDocument>,
    /// The calls, at most [`MAX_STORE_STEPS`].
    pub steps: Vec<StoreStep>,
    /// Part of the session is missing: more calls than [`MAX_STORE_STEPS`],
    /// more text than [`MAX_STORE_TEXT_BYTES`], a document the store would not
    /// read out, or a lock body that panicked. A replay refuses it.
    pub incomplete: bool,
    /// The store was protected: one such row stands for every lock until a
    /// lock finds it unprotected, and holds nothing derived from the field's
    /// content, so it has no document and no calls. A replay refuses it.
    pub protected: bool,
}

/// How a store's replay departed from its recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StoreDivergence {
    /// The replaying store refused the lock the recording was granted.
    #[error("the replaying store refused the lock: {0:?}")]
    LockRefused(StoreError),
    /// The replaying store refused the lock for a reason the schema does not
    /// name yet.
    #[error("the replaying store refused the lock for a reason the schema does not name")]
    LockRefusedUnmirrored,
    /// The replaying store queued the lock the recording ran at once.
    #[error("the replaying store deferred the lock")]
    LockDeferred,
    /// The replaying store could not be set to the document the lock found.
    #[error("the replaying store could not take the document the lock found")]
    Document,
    /// Call `step` (0-based) answered otherwise than it did when recorded.
    #[error("call {step} (`{call}`) answered otherwise than recorded")]
    Answer {
        /// The call's index in the lock.
        step: usize,
        /// The call's name in the schema.
        call: &'static str,
    },
}

/// The kind of `error`, as a recording keeps it; `None` for a refusal the
/// schema does not mirror yet.
#[must_use]
pub fn error_kind(error: &TextStoreError) -> Option<StoreError> {
    Some(match error {
        TextStoreError::SyncLockUnavailable => StoreError::SyncLockUnavailable,
        TextStoreError::DeferredQueueFull => StoreError::DeferredQueueFull,
        TextStoreError::Offset(_) => StoreError::Offset,
        TextStoreError::Protected => StoreError::Protected,
        TextStoreError::NoLayout => StoreError::NoLayout,
        TextStoreError::PointOutside => StoreError::PointOutside,
        TextStoreError::Detached => StoreError::Detached,
        _ => return None,
    })
}

/// `purpose` as the schema spells it; `None` for one it does not mirror.
#[must_use]
pub const fn purpose_to_wire(purpose: TextInputPurpose) -> Option<wire::InputPurpose> {
    Some(match purpose {
        TextInputPurpose::Text => wire::InputPurpose::Text,
        TextInputPurpose::Email => wire::InputPurpose::Email,
        TextInputPurpose::Url => wire::InputPurpose::Url,
        TextInputPurpose::Phone => wire::InputPurpose::Phone,
        TextInputPurpose::Number => wire::InputPurpose::Number,
        TextInputPurpose::Search => wire::InputPurpose::Search,
        _ => return None,
    })
}

impl StoreLock {
    /// Replay the lock on `store`: set it to the document the lock found, if
    /// the recording holds it, then make each call under a synchronous lock
    /// of the recorded kind and compare its answer with the recorded one. An
    /// offset asked at a point is not asked again.
    ///
    /// # Errors
    ///
    /// [`StoreDivergence`] for the first answer that differs, or a lock the
    /// store does not grant at once; the calls after it are not made.
    pub fn replay_on(&self, store: &dyn TextStore) -> Result<(), StoreDivergence> {
        if let Some(document) = &self.opened {
            let document = document.clone();
            let restored = Rc::new(RefCell::new(false));
            let done = Rc::clone(&restored);
            let grant = LockGrant::read_write(move |session| {
                *done.borrow_mut() = restore(session, &document).is_ok();
            });
            granted(store.request_lock(grant, LockTiming::Sync))?;
            if !*restored.borrow() {
                return Err(StoreDivergence::Document);
            }
        }
        let steps = self.steps.clone();
        let outcome = Rc::new(RefCell::new(Ok(())));
        let result = Rc::clone(&outcome);
        let grant = match self.kind {
            LockKind::Read => LockGrant::read(move |session| {
                *result.borrow_mut() = replay_steps(&steps, |step| read(session, step));
            }),
            LockKind::ReadWrite => LockGrant::read_write(move |session| {
                *result.borrow_mut() = replay_steps(&steps, |step| edit(session, step));
            }),
        };
        granted(store.request_lock(grant, LockTiming::Sync))?;
        outcome.replace(Ok(()))
    }
}

/// A lock request's answer as a replay needs it: granted at once.
fn granted(answer: Result<LockOutcome, TextStoreError>) -> Result<(), StoreDivergence> {
    match answer {
        Ok(LockOutcome::Granted) => Ok(()),
        Ok(LockOutcome::Deferred) => Err(StoreDivergence::LockDeferred),
        Err(error) => Err(error_kind(&error).map_or(
            StoreDivergence::LockRefusedUnmirrored,
            StoreDivergence::LockRefused,
        )),
    }
}

/// Set `session`'s document to `document`.
fn restore(
    session: &mut dyn TextStoreEdit,
    document: &StoreDocument,
) -> Result<(), TextStoreError> {
    let whole = Utf16Range::try_new(Utf16Offset::ZERO, session.document_len())
        .expect("BUG: a document's length is at or past its start");
    session.replace(whole, &document.text)?;
    session.set_selection(document.selection)?;
    session.set_composition(document.composition)
}

/// Run `replay` over each step until one answers otherwise.
fn replay_steps(
    steps: &[StoreStep],
    mut replay: impl FnMut(&StoreStep) -> bool,
) -> Result<(), StoreDivergence> {
    for (index, step) in steps.iter().enumerate() {
        if !replay(step) {
            return Err(StoreDivergence::Answer {
                step: index,
                call: step.name(),
            });
        }
    }
    Ok(())
}

/// Whether a replayed answer is the recorded one: the same value, or a
/// refusal of the same kind.
fn same<T: PartialEq>(
    replayed: Result<T, TextStoreError>,
    recorded: &Result<T, StoreError>,
) -> bool {
    match (replayed, recorded) {
        (Ok(replayed), Ok(recorded)) => replayed == *recorded,
        (Err(replayed), Err(recorded)) => error_kind(&replayed) == Some(*recorded),
        _ => false,
    }
}

/// Make a read `step` on `session`: whether it answered as recorded. An edit
/// cannot be made under a read lock, so it never does.
fn read(session: &dyn TextStoreRead, step: &StoreStep) -> bool {
    match step {
        StoreStep::DocumentLen(len) => session.document_len() == *len,
        StoreStep::Text { range, answer } => same(session.text(*range), answer),
        StoreStep::Selection(selection) => session.selection() == *selection,
        StoreStep::Composition(composition) => session.composition() == *composition,
        StoreStep::IndexAtPoint { .. } => true,
        StoreStep::Replace { .. }
        | StoreStep::InsertAtSelection { .. }
        | StoreStep::SetSelection { .. }
        | StoreStep::SetComposition { .. } => false,
    }
}

/// Make `step` on `session`: whether it answered as recorded.
fn edit(session: &mut dyn TextStoreEdit, step: &StoreStep) -> bool {
    match step {
        StoreStep::Replace {
            range,
            text,
            answer,
        } => same(session.replace(*range, text), answer),
        StoreStep::InsertAtSelection { text, answer } => {
            same(session.insert_at_selection(text), answer)
        }
        StoreStep::SetSelection { selection, answer } => {
            same(session.set_selection(*selection), answer)
        }
        StoreStep::SetComposition {
            composition,
            answer,
        } => same(session.set_composition(*composition), answer),
        StoreStep::DocumentLen(_)
        | StoreStep::Text { .. }
        | StoreStep::Selection(_)
        | StoreStep::Composition(_)
        | StoreStep::IndexAtPoint { .. } => read(&*session, step),
    }
}

/// An offset as the schema writes it.
fn units(offset: Utf16Offset) -> u64 {
    u64::try_from(offset.get()).expect("BUG: an offset into a document fits in 64 bits")
}

fn offset_from_wire(units: u64) -> DtoResult<Utf16Offset> {
    usize::try_from(units)
        .map(Utf16Offset::new)
        .map_err(|_| invalid(format!("offset {units} does not fit this host")))
}

fn range_to_wire(range: Utf16Range) -> [u64; 2] {
    [units(range.start()), units(range.end())]
}

fn range_from_wire([start, end]: [u64; 2]) -> DtoResult<Utf16Range> {
    Utf16Range::try_new(offset_from_wire(start)?, offset_from_wire(end)?)
        .ok_or_else(|| invalid(format!("range {start}..{end} is inverted")))
}

fn selection_to_wire(selection: Selection) -> [u64; 2] {
    [units(selection.anchor), units(selection.active)]
}

fn selection_from_wire([anchor, active]: [u64; 2]) -> DtoResult<Selection> {
    Ok(Selection {
        anchor: offset_from_wire(anchor)?,
        active: offset_from_wire(active)?,
    })
}

fn composition_to_wire(composition: Composition) -> wire::StoreComposition {
    wire::StoreComposition {
        range: range_to_wire(composition.range),
        hides_caret: composition.hides_caret,
    }
}

fn composition_from_wire(composition: wire::StoreComposition) -> DtoResult<Composition> {
    Ok(Composition {
        range: range_from_wire(composition.range)?,
        hides_caret: composition.hides_caret,
    })
}

fn change_to_wire(change: TextChange) -> [u64; 3] {
    [
        units(change.start),
        units(change.old_end),
        units(change.new_end),
    ]
}

fn change_from_wire([start, old_end, new_end]: [u64; 3]) -> DtoResult<TextChange> {
    if start > old_end || start > new_end {
        return Err(invalid(format!(
            "edit {start}..{old_end} → {start}..{new_end} ends before it starts"
        )));
    }
    Ok(TextChange {
        start: offset_from_wire(start)?,
        old_end: offset_from_wire(old_end)?,
        new_end: offset_from_wire(new_end)?,
    })
}

/// An answer the schema writes as a value or an error, exactly one of them.
fn answer_from_wire<W, T>(
    value: Option<W>,
    error: Option<StoreError>,
    what: &str,
    convert: impl FnOnce(W) -> DtoResult<T>,
) -> DtoResult<Result<T, StoreError>> {
    match (value, error) {
        (Some(value), None) => convert(value).map(Ok),
        (None, Some(error)) => Ok(Err(error)),
        _ => Err(invalid(format!(
            "a `{what}` call holds an answer or an error, exactly one"
        ))),
    }
}

/// An answer the schema writes as a value or an error.
fn answer_to_wire<T, W>(
    answer: Result<T, StoreError>,
    convert: impl FnOnce(T) -> W,
) -> (Option<W>, Option<StoreError>) {
    match answer {
        Ok(value) => (Some(convert(value)), None),
        Err(error) => (None, Some(error)),
    }
}

fn step_to_wire(step: StoreStep) -> wire::StoreStep {
    match step {
        StoreStep::DocumentLen(len) => wire::StoreStep::DocumentLen { len: units(len) },
        StoreStep::Text { range, answer } => {
            let (text, error) = answer_to_wire(answer, std::convert::identity);
            wire::StoreStep::Text {
                range: range_to_wire(range),
                text,
                error,
            }
        }
        StoreStep::Selection(selection) => wire::StoreStep::Selection {
            selection: selection_to_wire(selection),
        },
        StoreStep::Composition(composition) => wire::StoreStep::Composition {
            composition: composition.map(composition_to_wire),
        },
        StoreStep::IndexAtPoint {
            point,
            mode,
            answer,
        } => {
            let (index, error) = answer_to_wire(answer, units);
            wire::StoreStep::IndexAtPoint {
                point: [point.x, point.y],
                round_to_nearest_edge: mode.round_to_nearest_edge,
                nearest_when_outside: mode.nearest_when_outside,
                index,
                error,
            }
        }
        StoreStep::Replace {
            range,
            text,
            answer,
        } => {
            let (change, error) = answer_to_wire(answer, change_to_wire);
            wire::StoreStep::Replace {
                range: range_to_wire(range),
                text,
                change,
                error,
            }
        }
        StoreStep::InsertAtSelection { text, answer } => {
            let (change, error) = answer_to_wire(answer, change_to_wire);
            wire::StoreStep::InsertAtSelection {
                text,
                change,
                error,
            }
        }
        StoreStep::SetSelection { selection, answer } => wire::StoreStep::SetSelection {
            selection: selection_to_wire(selection),
            error: answer.err(),
        },
        StoreStep::SetComposition {
            composition,
            answer,
        } => wire::StoreStep::SetComposition {
            composition: composition.map(composition_to_wire),
            error: answer.err(),
        },
    }
}

fn step_from_wire(step: wire::StoreStep) -> DtoResult<StoreStep> {
    Ok(match step {
        wire::StoreStep::DocumentLen { len } => StoreStep::DocumentLen(offset_from_wire(len)?),
        wire::StoreStep::Text { range, text, error } => StoreStep::Text {
            range: range_from_wire(range)?,
            answer: answer_from_wire(text, error, "text", Ok)?,
        },
        wire::StoreStep::Selection { selection } => {
            StoreStep::Selection(selection_from_wire(selection)?)
        }
        wire::StoreStep::Composition { composition } => {
            StoreStep::Composition(composition.map(composition_from_wire).transpose()?)
        }
        wire::StoreStep::IndexAtPoint {
            point: [x, y],
            round_to_nearest_edge,
            nearest_when_outside,
            index,
            error,
        } => {
            if !(x.is_finite() && y.is_finite()) {
                return Err(invalid(format!("point ({x}, {y}) is not finite")));
            }
            StoreStep::IndexAtPoint {
                point: Point::new(x, y),
                mode: PointMode {
                    round_to_nearest_edge,
                    nearest_when_outside,
                },
                answer: answer_from_wire(index, error, "index_at_point", offset_from_wire)?,
            }
        }
        wire::StoreStep::Replace {
            range,
            text,
            change,
            error,
        } => StoreStep::Replace {
            range: range_from_wire(range)?,
            text,
            answer: answer_from_wire(change, error, "replace", change_from_wire)?,
        },
        wire::StoreStep::InsertAtSelection {
            text,
            change,
            error,
        } => StoreStep::InsertAtSelection {
            text,
            answer: answer_from_wire(change, error, "insert_at_selection", change_from_wire)?,
        },
        wire::StoreStep::SetSelection { selection, error } => StoreStep::SetSelection {
            selection: selection_from_wire(selection)?,
            answer: error.map_or(Ok(()), Err),
        },
        wire::StoreStep::SetComposition { composition, error } => StoreStep::SetComposition {
            composition: composition.map(composition_from_wire).transpose()?,
            answer: error.map_or(Ok(()), Err),
        },
        _ => return Err(unread("StoreStep")),
    })
}

/// The bytes of text a wire step carries, read before the step is built.
fn wire_text_bytes(step: &wire::StoreStep) -> usize {
    match step {
        wire::StoreStep::Text {
            text: Some(text), ..
        }
        | wire::StoreStep::Replace { text, .. }
        | wire::StoreStep::InsertAtSelection { text, .. } => text.len(),
        _ => 0,
    }
}

/// Whether a wire step edits.
const fn wire_is_edit(step: &wire::StoreStep) -> bool {
    matches!(
        step,
        wire::StoreStep::Replace { .. }
            | wire::StoreStep::InsertAtSelection { .. }
            | wire::StoreStep::SetSelection { .. }
            | wire::StoreStep::SetComposition { .. }
    )
}

/// `lock` as the schema holds it, its text moved rather than copied.
pub(super) fn lock_to_wire(lock: StoreLock) -> wire::StoreLock {
    wire::StoreLock {
        store: lock.store.get(),
        kind: match lock.kind {
            LockKind::Read => wire::StoreLockKind::Read,
            LockKind::ReadWrite => wire::StoreLockKind::ReadWrite,
        },
        opened: lock.opened.map(|document| wire::StoreDocument {
            text: document.text,
            selection: selection_to_wire(document.selection),
            composition: document.composition.map(composition_to_wire),
        }),
        steps: lock.steps.into_iter().map(step_to_wire).collect(),
        incomplete: lock.incomplete,
        protected: lock.protected,
    }
}

/// A lock as the schema holds it, its bounds checked before any call of it
/// is built: at most [`MAX_STORE_STEPS`] calls and [`MAX_STORE_TEXT_BYTES`]
/// of text, a nonzero store, no edit under a read lock, and nothing of a
/// protected store's content.
pub(super) fn lock_from_wire(lock: wire::StoreLock) -> DtoResult<StoreLock> {
    let store = NonZeroU64::new(lock.store)
        .map(RecordedStore::from_raw)
        .ok_or_else(|| invalid("store 0 names no store"))?;
    if lock.steps.len() > MAX_STORE_STEPS {
        return Err(invalid(format!(
            "a store lock holds {} calls, past the {MAX_STORE_STEPS} a row holds",
            lock.steps.len()
        )));
    }
    let text_bytes = lock
        .steps
        .iter()
        .map(wire_text_bytes)
        .chain(lock.opened.iter().map(|document| document.text.len()))
        .try_fold(0_usize, usize::checked_add)
        .filter(|total| *total <= MAX_STORE_TEXT_BYTES);
    if text_bytes.is_none() {
        return Err(invalid(format!(
            "a store lock carries more than the {MAX_STORE_TEXT_BYTES} bytes of text a row holds"
        )));
    }
    let kind = match lock.kind {
        wire::StoreLockKind::Read => LockKind::Read,
        wire::StoreLockKind::ReadWrite => LockKind::ReadWrite,
        other => return Err(unread(other.name())),
    };
    if lock.protected && (lock.opened.is_some() || !lock.steps.is_empty()) {
        return Err(invalid("a protected store's lock holds content"));
    }
    if kind == LockKind::Read && lock.steps.iter().any(wire_is_edit) {
        return Err(invalid("a read lock holds an edit"));
    }
    let opened = lock
        .opened
        .map(|document| {
            Ok::<_, DtoError>(StoreDocument {
                text: document.text,
                selection: selection_from_wire(document.selection)?,
                composition: document
                    .composition
                    .map(composition_from_wire)
                    .transpose()?,
            })
        })
        .transpose()?;
    Ok(StoreLock {
        store,
        kind,
        opened,
        steps: lock
            .steps
            .into_iter()
            .map(step_from_wire)
            .collect::<DtoResult<Vec<StoreStep>>>()?,
        incomplete: lock.incomplete,
        protected: lock.protected,
    })
}

pub(super) fn focus_to_wire(focus: &StoreFocus) -> DtoResult<wire::StoreFocus> {
    Ok(wire::StoreFocus {
        store: focus.store.get(),
        status: wire::StoreStatus {
            protected: focus.status.protected,
            single_line: focus.status.single_line,
            purpose: purpose_to_wire(focus.status.purpose)
                .ok_or(DtoError::Unmirrored("TextInputPurpose"))?,
        },
    })
}

pub(super) fn focus_from_wire(focus: wire::StoreFocus) -> DtoResult<StoreFocus> {
    let store = NonZeroU64::new(focus.store)
        .map(RecordedStore::from_raw)
        .ok_or_else(|| invalid("store 0 names no store"))?;
    let purpose = match focus.status.purpose {
        wire::InputPurpose::Text => TextInputPurpose::Text,
        wire::InputPurpose::Email => TextInputPurpose::Email,
        wire::InputPurpose::Url => TextInputPurpose::Url,
        wire::InputPurpose::Phone => TextInputPurpose::Phone,
        wire::InputPurpose::Number => TextInputPurpose::Number,
        wire::InputPurpose::Search => TextInputPurpose::Search,
        other => return Err(unread(other.name())),
    };
    let mut status = TextStoreStatus::EDITABLE_SINGLE_LINE
        .with_protected(focus.status.protected)
        .with_purpose(purpose);
    status.single_line = focus.status.single_line;
    Ok(StoreFocus { store, status })
}
