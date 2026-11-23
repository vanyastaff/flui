//! The capture sink that folds registry calls into native recordings
//! (`native-capture`), one per registry, so a run of the backend on a device
//! journals with the same code the in-process captures use.

use std::sync::Arc;

use flui_platform::ImeEvent;
use flui_protocol::native_recording::{Budget, Line};
use parking_lot::Mutex;

use super::capture::{CaptureGuard, CaptureSource, Captured, capture};
use super::codec::{
    Backend, CodecError, CutReason, DisplayRead, Header, NativeRecording, PreferenceRead,
    ReadFailure, RecordedDisplay, RecordedWindowState, Row,
};
use super::order::line_bytes;
use super::store::{StoreDocument, StoreLock};

/// Whether a recorder keeps the text it journals (ADR-0215 §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a recorder redacts text or keeps it, the one opt-in"
)]
pub enum Privacy {
    /// Input method text, the text a text store lock read and wrote, and
    /// window titles are replaced, and each row that
    /// lost text is marked redacted, which its replay refuses. A display's
    /// name is replaced and its persistent key pseudonymized, which keeps its
    /// row replayable.
    #[default]
    Redact,
    /// Keep all text and identifiers: the one explicit opt-in per recording.
    KeepText,
}

/// The text a redacted row carries in place of the original.
const REDACTED: &str = "\u{fffd}";

/// A capture installed on this thread, journalling every registry, loop
/// delivery, window delivery and platform created on it while it lives, in
/// creation order.
#[derive(Debug)]
#[must_use = "the capture ends when the recorder is finished or dropped"]
pub struct Recorder {
    journals: Arc<Mutex<Vec<NativeRecording>>>,
    guard: CaptureGuard,
}

impl Recorder {
    /// Install a capture for `backend` on this thread, redacting text.
    pub fn start(backend: Backend) -> Self {
        Self::with_privacy(backend, Privacy::Redact)
    }

    /// Install a capture for `backend` on this thread that treats text as
    /// `privacy` says.
    pub fn with_privacy(backend: Backend, privacy: Privacy) -> Self {
        let journals: Arc<Mutex<Vec<NativeRecording>>> = Arc::default();
        let sink = Arc::clone(&journals);
        let mut folding = Vec::new();
        let guard = capture(move |source, call| {
            journal(
                &mut sink.lock(),
                &mut folding,
                backend,
                privacy,
                source,
                call,
            );
        });
        Self { journals, guard }
    }

    /// A handle that reads the recordings while the capture goes on.
    pub fn journals(&self) -> Journals {
        Journals(Arc::clone(&self.journals))
    }

    /// End the capture and return its recordings.
    pub fn finish(self) -> Vec<NativeRecording> {
        drop(self.guard);
        std::mem::take(&mut *self.journals.lock())
    }
}

/// The recordings of a live [`Recorder`].
#[derive(Debug, Clone)]
pub struct Journals(Arc<Mutex<Vec<NativeRecording>>>);

impl Journals {
    /// The recordings journalled so far.
    pub fn snapshot(&self) -> Vec<NativeRecording> {
        self.0.lock().clone()
    }
}

/// What a recorder keeps of one source beside its recording.
struct Folding {
    pseudonyms: Pseudonyms,
    /// The bytes and rows the recording holds, from its first row on: the
    /// header is final once setup ends.
    budget: Option<Budget>,
}

/// Fold one captured call into the recording of its source. The recording
/// holds rows while they fit the bounds its reader holds it to; the first
/// that does not is replaced by a `cut` row, and nothing is kept after it, so
/// a recording never holds more than a reader takes.
fn journal(
    recordings: &mut Vec<NativeRecording>,
    folding: &mut Vec<Folding>,
    backend: Backend,
    privacy: Privacy,
    source: CaptureSource,
    call: Captured<'_>,
) {
    let index = source.0 as usize;
    if let Some(header) = opened(backend, call) {
        assert_eq!(
            index,
            recordings.len(),
            "BUG: a capture numbers its sources in order"
        );
        folding.push(Folding {
            pseudonyms: Pseudonyms::default(),
            budget: None,
        });
        recordings.push(NativeRecording {
            header,
            rows: Vec::new(),
        });
        return;
    }
    let recording = &mut recordings[index];
    if matches!(recording.rows.last(), Some(Row::Cut(_))) {
        // A source journals nothing after its cut.
        return;
    }
    let folding = &mut folding[index];
    let setup = recording.rows.is_empty();
    let Some(row) = fold(
        &mut recording.header,
        setup,
        &mut folding.pseudonyms,
        privacy,
        call,
    ) else {
        return;
    };
    let header = &recording.header;
    let budget = folding
        .budget
        .get_or_insert_with(|| Budget::after_header(line_bytes(header.to_wire())));
    // Room is always kept for a `cut` row, so a cut always fits: a row that
    // is one stands in for nothing after it.
    let row = match row {
        Row::Cut(_) => row,
        row => admitted(budget, line_bytes(row.to_wire()), row),
    };
    recording.rows.push(row);
}

/// `row`, of `line` bytes, if `budget` admits it; otherwise the row that
/// stands in its place and is counted instead: a `cut` where the recording is
/// full, and a `gap` of one row for a row no recording holds (longer than a
/// line may be), so what the recorder keeps is bounded and always written.
fn admitted(budget: &mut Budget, line: usize, row: Row) -> Row {
    let Err(overrun) = budget.admit_row(line, 0, true) else {
        return row;
    };
    if let Some(reason) = overrun.cut_reason() {
        return Row::Cut(reason);
    }
    let gap = Row::Gap { rows: 1 };
    let gap_line = line_bytes(Ok::<_, CodecError>(Line::Gap { rows: 1 }));
    match budget.admit_row(gap_line, 0, true) {
        Ok(()) => gap,
        // The gap passes a bound of its own: the recording is full (a gap is
        // never longer than a line, and the recorder's header declares no
        // device, so no other bound refuses it).
        Err(overrun) => Row::Cut(overrun.cut_reason().unwrap_or(CutReason::Bytes)),
    }
}

/// The header a call that opens its source starts it with.
pub(super) fn opened(backend: Backend, call: Captured<'_>) -> Option<Header> {
    match call {
        Captured::Started {
            epoch,
            uptime,
            counter,
        } => Some(Header::new(backend, epoch, uptime, counter)),
        Captured::Opened(stream) => Some(Header::for_stream(backend, stream)),
        _ => None,
    }
}

/// Fold a call of an opened source into its `header`, while `setup` (no row yet), or into the
/// row it adds, its text treated as `privacy` says.
pub(super) fn fold(
    header: &mut Header,
    setup: bool,
    pseudonyms: &mut Pseudonyms,
    privacy: Privacy,
    call: Captured<'_>,
) -> Option<Row> {
    let row = match call {
        Captured::ReservedContact(contact) => {
            header.reserved_contact = Some(contact);
            return None;
        }
        Captured::FirstPointer(first) => {
            header.first_pointer = Some(first);
            return None;
        }
        Captured::FirstDevice(first) => {
            header.first_device = Some(first);
            return None;
        }
        Captured::ClickSettings(settings) if setup => {
            header.click_settings = settings;
            return None;
        }
        Captured::ClickSettings(settings) => Row::ClickSettings(settings),
        Captured::Reading(reading) => Row::Reading(*reading),
        Captured::Batch {
            coalesced,
            predicted,
            current,
        } => Row::Batch {
            coalesced: coalesced.to_vec(),
            predicted: predicted.to_vec(),
            current: *current,
        },
        Captured::AddDevice { device, kind, time } => Row::AddDevice { device, kind, time },
        Captured::RemoveDevice { device, time } => Row::RemoveDevice { device, time },
        Captured::CancelSurface {
            surface,
            time,
            reason,
        } => Row::CancelSurface {
            surface,
            time,
            reason,
        },
        Captured::CancelContact {
            surface,
            contact,
            time,
            reason,
        } => Row::CancelContact {
            surface,
            contact,
            time,
            reason,
        },
        Captured::Viewport { reading, frame } => Row::Viewport { reading, frame },
        Captured::ViewportAbandoned { frame } => Row::ViewportAbandoned { frame },
        Captured::Wheel(reading) => Row::Wheel(*reading),
        Captured::Scroll(reading) => Row::Scroll(*reading),
        Captured::Notice(notice) => Row::Notice(notice),
        Captured::SessionEnd(phase) => Row::SessionEnd(phase),
        Captured::Reopen => Row::Reopen,
        Captured::Ime(event) => match privacy {
            Privacy::KeepText => Row::Ime {
                event: event.clone(),
                redacted: false,
            },
            Privacy::Redact => {
                let (event, redacted) = redacted(event);
                Row::Ime { event, redacted }
            }
        },
        Captured::Preferences(read) => Row::Preferences(match read {
            Ok(values) => PreferenceRead::Observed(values.clone()),
            Err(error) => PreferenceRead::Failed(ReadFailure::of(error)),
        }),
        Captured::Displays(read) => {
            let read = match read {
                Ok(snapshot) => DisplayRead::Observed {
                    revision: snapshot.revision().get(),
                    displays: snapshot
                        .displays()
                        .iter()
                        .cloned()
                        .map(RecordedDisplay::of)
                        .collect(),
                },
                Err(error) => DisplayRead::Failed(ReadFailure::of(error)),
            };
            match (privacy, read) {
                (Privacy::Redact, DisplayRead::Observed { revision, displays }) => Row::Displays {
                    read: DisplayRead::Observed {
                        revision,
                        displays: displays
                            .into_iter()
                            .map(|display| pseudonyms.anonymized(display))
                            .collect(),
                    },
                    redacted: true,
                },
                (_, read) => Row::Displays {
                    read,
                    redacted: false,
                },
            }
        }
        Captured::WindowState(state) => {
            let state = RecordedWindowState::of(state.clone());
            match privacy {
                Privacy::KeepText => Row::State {
                    state,
                    redacted: false,
                },
                Privacy::Redact => {
                    let redacted = state.title().is_some();
                    Row::State {
                        state: state.retitled(|_| REDACTED.to_owned()),
                        redacted,
                    }
                }
            }
        }
        Captured::StoreFocus(focus) => Row::StoreFocus(focus),
        Captured::StoreLock(lock) => match privacy {
            Privacy::KeepText => Row::StoreLock {
                lock: lock.clone(),
                redacted: false,
            },
            Privacy::Redact => {
                let (lock, redacted) = redacted_lock(lock);
                Row::StoreLock { lock, redacted }
            }
        },
        Captured::CloseRequest { answer, queued } => Row::CloseRequest { answer, queued },
        Captured::Closed(cause) => Row::Closed(cause),
        Captured::Cut(reason) => Row::Cut(reason),
        Captured::Started { .. } | Captured::Opened(_) => {
            unreachable!("BUG: a source opens once, before its other calls")
        }
    };
    Some(row)
}

/// `event` with its text replaced, and whether it had any to replace.
fn redacted(event: &ImeEvent) -> (ImeEvent, bool) {
    match event {
        // The cursor is the input method's target clause; of a redacted
        // preedit nothing is left to select, so a caret after it stays.
        ImeEvent::Preedit { cursor, .. } => (
            ImeEvent::Preedit {
                text: REDACTED.to_owned(),
                cursor: cursor.map(|_| (REDACTED.len(), REDACTED.len())),
            },
            true,
        ),
        ImeEvent::Commit(_) => (ImeEvent::Commit(REDACTED.to_owned()), true),
        other => (other.clone(), false),
    }
}

/// `lock` with every text it carries (the document it found, the text read
/// and written) replaced, and whether it carried any.
fn redacted_lock(lock: &StoreLock) -> (StoreLock, bool) {
    let mut redacted = lock.opened.is_some();
    let steps = lock
        .steps
        .iter()
        .map(|step| {
            let (step, carried) = step.redacted(REDACTED);
            redacted |= carried;
            step
        })
        .collect();
    // Built anew, so the text it replaces is never copied.
    let opened = lock.opened.as_ref().map(|document| StoreDocument {
        text: REDACTED.to_owned(),
        selection: document.selection,
        composition: document.composition,
    });
    (
        StoreLock {
            opened,
            steps,
            store: lock.store,
            kind: lock.kind,
            incomplete: lock.incomplete,
            protected: lock.protected,
        },
        redacted,
    )
}

/// The pseudonyms one recording gives the displays' persistent keys: a
/// counter in order of first sight, never derived from the key, so a redacted
/// recording keeps which reads saw the same display without naming it.
#[derive(Debug, Default)]
pub(super) struct Pseudonyms(
    #[expect(
        clippy::disallowed_types,
        reason = "keyed by the display keys the platform reports, which the recording only reads"
    )]
    std::collections::HashMap<String, String>,
);

impl Pseudonyms {
    /// `display` with its name replaced and its persistent key pseudonymized.
    fn anonymized(&mut self, mut display: RecordedDisplay) -> RecordedDisplay {
        REDACTED.clone_into(&mut display.parts.name);
        display.parts.persistent_key = display.parts.persistent_key.map(|key| {
            let next = self.0.len() + 1;
            self.0
                .entry(key)
                .or_insert_with(|| format!("display-{next}"))
                .clone()
        });
        display
    }
}
