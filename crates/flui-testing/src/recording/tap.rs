//! Feeding a runtime's records to a [`RecordingWriter`] (ADR-0215 §1).
//!
//! The tap runs on the runtime's owner thread and only converts each record
//! into its owned, already redacted row. Serialisation and IO run on a worker
//! thread over those owned rows, with no lock shared with the runtime; on
//! wasm, where there are no threads, the tap writes inline.

use std::io::Write;

use flui_foundation::IdExhausted;
use flui_runtime::{InputRecord, InputTap, InputTapHandle, InputTaps};

use super::format::{PreparedRow, Privacy, RecordingError, RecordingWriter};

/// A recording in progress: the runtime's tap registration and the writer it
/// feeds. [`finish`](Self::finish) ends it with its `End` row.
#[must_use = "dropping a recording unregisters its tap and leaves it without an end row"]
pub struct Recording<W: Write + 'static> {
    handle: InputTapHandle,
    sink: Sink<W>,
}

impl<W: Write + 'static> std::fmt::Debug for Recording<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recording")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Sink<W> = std::thread::JoinHandle<Result<RecordingWriter<W>, RecordingError>>;

#[cfg(target_arch = "wasm32")]
type Sink<W> = std::rc::Rc<std::cell::RefCell<Inline<W>>>;

/// A writer fed inline on the owner thread, keeping its first error.
#[cfg(target_arch = "wasm32")]
pub(crate) struct Inline<W: Write> {
    writer: Option<RecordingWriter<W>>,
    failure: Option<RecordingError>,
}

/// The tap: converts each record to its owned row and hands it on.
struct RowTap {
    privacy: Privacy,
    #[cfg(not(target_arch = "wasm32"))]
    rows: std::sync::mpsc::Sender<PreparedRow>,
    #[cfg(target_arch = "wasm32")]
    rows: Box<dyn Fn(PreparedRow)>,
}

impl InputTap for RowTap {
    fn record(&self, record: &InputRecord) {
        let row = super::format::prepare(record, self.privacy);
        #[cfg(not(target_arch = "wasm32"))]
        {
            // A worker that stopped on a write error keeps that error for
            // `finish`; later rows have nowhere to go.
            let _ = self.rows.send(row);
        }
        #[cfg(target_arch = "wasm32")]
        (self.rows)(row);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn<W: Write + Send + 'static>(writer: RecordingWriter<W>) -> (RowTap, Sink<W>) {
    let privacy = writer.privacy();
    let (rows, received) = std::sync::mpsc::channel::<PreparedRow>();
    let worker = std::thread::Builder::new()
        .name("flui-input-recording".to_owned())
        .spawn(move || {
            let mut writer = writer;
            for row in received {
                writer.write_row(row)?;
            }
            Ok(writer)
        })
        .expect("BUG: the recording worker thread could not start");
    (RowTap { privacy, rows }, worker)
}

#[cfg(target_arch = "wasm32")]
fn spawn<W: Write + 'static>(writer: RecordingWriter<W>) -> (RowTap, Sink<W>) {
    let privacy = writer.privacy();
    let sink = std::rc::Rc::new(std::cell::RefCell::new(Inline {
        writer: Some(writer),
        failure: None,
    }));
    let target = std::rc::Rc::clone(&sink);
    let rows = Box::new(move |row| {
        let mut inline = target.borrow_mut();
        if inline.failure.is_some() {
            return;
        }
        if let Some(writer) = inline.writer.as_mut()
            && let Err(error) = writer.write_row(row)
        {
            inline.failure = Some(error);
        }
    });
    (RowTap { privacy, rows }, sink)
}

/// Record everything the runtime behind `taps` applies into `writer`, until
/// [`Recording::finish`].
///
/// # Errors
///
/// [`IdExhausted`] when `taps` issued every registration generation.
#[cfg(not(target_arch = "wasm32"))]
pub fn record<W: Write + Send + 'static>(
    taps: &InputTaps,
    writer: RecordingWriter<W>,
) -> Result<Recording<W>, IdExhausted> {
    let (tap, sink) = spawn(writer);
    Ok(Recording {
        handle: taps.install(tap)?,
        sink,
    })
}

/// Record everything the runtime behind `taps` applies into `writer`, until
/// [`Recording::finish`].
///
/// # Errors
///
/// [`IdExhausted`] when `taps` issued every registration generation.
#[cfg(target_arch = "wasm32")]
pub fn record<W: Write + 'static>(
    taps: &InputTaps,
    writer: RecordingWriter<W>,
) -> Result<Recording<W>, IdExhausted> {
    let (tap, sink) = spawn(writer);
    Ok(Recording {
        handle: taps.install(tap)?,
        sink,
    })
}

impl<W: Write + 'static> Recording<W> {
    /// Deliver the remaining rows and the `End` row, then return the sink.
    ///
    /// # Errors
    ///
    /// [`RecordingError::Incomplete`] when the final `Gap` or the `End` row
    /// could not be delivered, and the writer's first [`RecordingError::Io`],
    /// [`RecordingError::Json`] or [`RecordingError::Unmirrored`].
    pub fn finish(self) -> Result<W, RecordingError> {
        let delivered = self.handle.finish();
        let writer = self.sink.join_writer()?;
        delivered.map_err(|incomplete| RecordingError::Incomplete {
            lost_rows: incomplete.lost_rows,
        })?;
        writer.finish()
    }
}

trait JoinWriter<W: Write> {
    fn join_writer(self) -> Result<RecordingWriter<W>, RecordingError>;
}

#[cfg(not(target_arch = "wasm32"))]
impl<W: Write> JoinWriter<W> for Sink<W> {
    fn join_writer(self) -> Result<RecordingWriter<W>, RecordingError> {
        // The tap and its sender left with the registration.
        self.join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    }
}

#[cfg(target_arch = "wasm32")]
impl<W: Write> JoinWriter<W> for Sink<W> {
    fn join_writer(self) -> Result<RecordingWriter<W>, RecordingError> {
        let mut inline = self.borrow_mut();
        if let Some(failure) = inline.failure.take() {
            return Err(failure);
        }
        Ok(inline
            .writer
            .take()
            .expect("BUG: a recording's inline writer is taken once, at finish"))
    }
}

/// A factory for [`AppConfig::with_input_tap`](https://docs.rs/flui-app):
/// each runtime records into the writer `make` returns, for the runtime's
/// life. `make` gets the header that runtime's recording starts from, on
/// the monotonic clock: its windows with their scales and gesture values and
/// its preference revision, as [`HeadlessHost::recording_header`](crate::HeadlessHost::recording_header)
/// writes them. A worker thread writes each runtime's rows; it ends,
/// flushing its writer, once the runtime and its tap are gone.
#[cfg(not(target_arch = "wasm32"))]
pub fn tap_factory<W, F>(make: F) -> flui_runtime::InputTapFactory
where
    W: Write + Send + 'static,
    F: Fn(super::format::RecordingHeader) -> RecordingWriter<W> + Send + Sync + 'static,
{
    flui_runtime::InputTapFactory::new(move |start| {
        let header = super::format::RecordingHeader::starting(
            super::format::RecordingClock::Monotonic,
            start,
        );
        let (tap, worker) = spawn(make(header));
        // Detached: the worker owns the writer and exits when the tap's
        // sender drops with the runtime.
        drop(worker);
        Box::new(tap)
    })
}
