//! The capture sink that folds registry calls into native recordings
//! (`native-capture`), one per registry, so a run of the backend on a device
//! journals with the same code the in-process captures use.

use std::sync::Arc;

use flui_platform::ImeEvent;
use parking_lot::Mutex;

use super::capture::{CaptureGuard, CaptureSource, Captured, capture};
use super::codec::{
    Backend, DisplayRead, Header, NativeRecording, PreferenceRead, ReadFailure, RecordedDisplay,
    Row,
};

/// Whether a recorder keeps the text it journals (ADR-0215 §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Privacy {
    /// Input method text is replaced, and each row that lost text is marked
    /// redacted, which its replay refuses.
    #[default]
    Redact,
    /// Keep all text: the one explicit opt-in per recording.
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
        let guard = capture(move |source, call| {
            journal(&mut sink.lock(), backend, privacy, source, call);
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

/// Fold one captured call into the recording of its source.
fn journal(
    recordings: &mut Vec<NativeRecording>,
    backend: Backend,
    privacy: Privacy,
    source: CaptureSource,
    call: Captured<'_>,
) {
    let index = source.0 as usize;
    let header = match call {
        Captured::Started {
            epoch,
            uptime,
            counter,
        } => Some(Header::new(backend, epoch, uptime, counter)),
        Captured::Opened(stream) => Some(Header::for_stream(backend, stream)),
        _ => None,
    };
    if let Some(header) = header {
        assert_eq!(
            index,
            recordings.len(),
            "BUG: a capture numbers its sources in order"
        );
        recordings.push(NativeRecording {
            header,
            rows: Vec::new(),
        });
        return;
    }
    let recording = &mut recordings[index];
    let setup = recording.rows.is_empty();
    let header = &mut recording.header;
    let row = match call {
        Captured::ReservedContact(contact) => {
            header.reserved_contact = Some(contact);
            return;
        }
        Captured::FirstPointer(first) => {
            header.first_pointer = Some(first);
            return;
        }
        Captured::FirstDevice(first) => {
            header.first_device = Some(first);
            return;
        }
        Captured::ClickSettings(settings) if setup => {
            header.click_settings = settings;
            return;
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
        Captured::Displays(read) => Row::Displays(match read {
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
        }),
        Captured::Started { .. } | Captured::Opened(_) => {
            unreachable!("BUG: a source opens once, handled above")
        }
    };
    recording.rows.push(row);
}

/// `event` with its text replaced, and whether it had any to replace.
fn redacted(event: &ImeEvent) -> (ImeEvent, bool) {
    match event {
        ImeEvent::Preedit { cursor, .. } => (
            ImeEvent::Preedit {
                text: REDACTED.to_owned(),
                cursor: cursor.map(|_| (0, REDACTED.len())),
            },
            true,
        ),
        ImeEvent::Commit(_) => (ImeEvent::Commit(REDACTED.to_owned()), true),
        other => (other.clone(), false),
    }
}
