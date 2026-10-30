//! Input recordings: the [`HostInput`](flui_runtime::HostInput) stream a
//! runtime applied, written and read as JSON Lines (ADR-0215).
//!
//! A runtime emits [`InputRecord`](flui_runtime::InputRecord) rows to its
//! tap; [`record`] installs a tap that feeds a [`RecordingWriter`] on a worker
//! thread (inline on wasm, into any [`Write`](std::io::Write)).
//! [`RecordingReader`] reads the rows back, and [`ReplayableRecording`]
//! checks a whole recording through its `End` row before
//! [`HeadlessHost::replay`](crate::HeadlessHost::replay) applies any of it.
//!
//! Text is redacted by default ([`Privacy::Redact`]): key text, IME strings,
//! assistive action text, window titles and device names never reach the
//! file, and a row that lost text is marked so replay refuses it instead of
//! applying different text. [`Privacy::KeepText`] is the explicit opt-in.
//!
//! The vocabulary carries no serde: private mirror types with pinned
//! spellings stand between it and the wire, and a generated table of its
//! variants keeps them in step (`cargo xtask recording-vocabulary`).
//!
//! ```
//! use flui_testing::recording::{
//!     RecordingClock, RecordingHeader, RecordingReader, RecordingWriter,
//! };
//! use flui_runtime::InputRecord;
//!
//! let header = RecordingHeader::new(RecordingClock::Virtual);
//! let mut writer = RecordingWriter::try_new(Vec::new(), &header)?;
//! writer.write(&InputRecord::End { lost_rows: 0 })?;
//! let bytes = writer.finish()?;
//! let mut reader = RecordingReader::try_new(&bytes[..])?;
//! let end = reader.next().expect("one row")?;
//! assert!(matches!(end.record, InputRecord::End { lost_rows: 0 }));
//! # Ok::<(), flui_testing::recording::RecordingError>(())
//! ```

mod dto;
mod format;
mod replay;
mod tap;

pub use format::{
    FORMAT_VERSION, Privacy, Record, RecordedDevice, RecordedWindow, RecordingClock,
    RecordingError, RecordingHeader, RecordingReader, RecordingWriter,
};
pub use replay::ReplayableRecording;
#[cfg(not(target_arch = "wasm32"))]
pub use tap::tap_factory;
pub use tap::{Recording, record};
