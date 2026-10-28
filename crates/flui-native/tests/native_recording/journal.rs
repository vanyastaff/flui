//! The capture sink that folds registry calls into native recordings
//! (`native-capture`).
//!
//! Self-contained beside [`codec`](super::codec) so a run of the
//! backend on a device journals with
//! the same code the in-process captures use.

use std::sync::Arc;

use flui_native::input::assembly::capture::{CaptureGuard, CaptureSource, Captured, capture};
use parking_lot::Mutex;

use super::codec::{Backend, Header, NativeRecording, Row};

/// A capture installed on this thread, journalling every registry created
/// on it while it lives, in creation order.
#[must_use = "the capture ends when the recorder is finished or dropped"]
pub struct Recorder {
    journals: Arc<Mutex<Vec<NativeRecording>>>,
    guard: CaptureGuard,
}

impl Recorder {
    /// Install a capture for `backend` on this thread.
    pub fn start(backend: Backend) -> Self {
        let journals: Arc<Mutex<Vec<NativeRecording>>> = Arc::default();
        let sink = Arc::clone(&journals);
        let guard = capture(move |source, call| journal(&mut sink.lock(), backend, source, call));
        Self { journals, guard }
    }

    /// A handle that reads the recordings while the capture goes on.
    #[cfg(test)]
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
#[cfg(test)]
#[derive(Clone)]
pub struct Journals(Arc<Mutex<Vec<NativeRecording>>>);

#[cfg(test)]
impl Journals {
    /// The recordings journalled so far.
    pub fn snapshot(&self) -> Vec<NativeRecording> {
        self.0.lock().clone()
    }
}

/// Run `session` with a capture installed on this thread, and return its
/// result with the recording of every registry created during it, in
/// creation order.
pub fn record<T>(backend: Backend, session: impl FnOnce() -> T) -> (T, Vec<NativeRecording>) {
    let recorder = Recorder::start(backend);
    let result = session();
    (result, recorder.finish())
}

/// Fold one captured call into the recording of its registry.
fn journal(
    recordings: &mut Vec<NativeRecording>,
    backend: Backend,
    source: CaptureSource,
    call: Captured<'_>,
) {
    let index = source.0 as usize;
    if let Captured::Started {
        epoch,
        uptime,
        counter,
    } = call
    {
        assert_eq!(index, recordings.len(), "sources are numbered in order");
        recordings.push(NativeRecording {
            header: Header::new(backend, epoch, uptime, counter),
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
        other => panic!("the capture grew a call the native codec does not mirror: {other:?}"),
    };
    recording.rows.push(row);
}
