//! Versioned documents that outlive the process.
//!
//! A [`Document`] is application data with a [`StorageName`], a format
//! version and its own byte encoding. [`Persisted`] keeps one document for a
//! widget: it reads it through the realm's [`Storage`] (acquired with
//! [`LifecycleContext::storage`](crate::LifecycleContext::storage)), writes
//! every value [`set`](Persisted::set) on it, and reports where that stands
//! as a [`SaveStatus`].
//!
//! The framework frames the application's bytes with a one-line header,
//! `flui-document <name> <version> <revision>\n`: the name guards against
//! reading another document's file, the version selects the decoder, and the
//! [`Revision`] counts the writes the file has seen. The body belongs to the
//! application, so no serialization crate appears in these signatures; the
//! application encodes with whatever it depends on.
//!
//! ```
//! use flui_platform_api::StorageName;
//! use flui_view::persist::{DecodeError, Document};
//!
//! struct Counter(u64);
//!
//! impl Document for Counter {
//!     const NAME: StorageName = StorageName::from_static("counter");
//!     const VERSION: u32 = 1;
//!
//!     fn initial() -> Self {
//!         Counter(0)
//!     }
//!
//!     fn encode(&self) -> Vec<u8> {
//!         self.0.to_string().into_bytes()
//!     }
//!
//!     fn decode(_version: u32, body: &[u8]) -> Result<Self, DecodeError> {
//!         std::str::from_utf8(body)
//!             .ok()
//!             .and_then(|text| text.parse().ok())
//!             .map(Counter)
//!             .ok_or_else(|| DecodeError::new("the body is not a number"))
//!     }
//! }
//!
//! assert_eq!(Counter::decode(1, b"7").map(|c| c.0), Ok(7));
//! ```
//!
//! [`Storage`]: flui_platform_api::Storage

mod persisted;

pub use persisted::Persisted;

use flui_platform_api::{StorageError, StorageName};

/// Application data that [`Persisted`] keeps across runs.
///
/// The application owns the format: [`encode`](Self::encode) turns a value
/// into body bytes and [`decode`](Self::decode) turns body bytes of any
/// version up to [`VERSION`](Self::VERSION) back into a value, migrating an
/// older format on the way.
pub trait Document: 'static {
    /// The name the document is stored under.
    const NAME: StorageName;

    /// The format version this build writes, counted from 1. A stored file
    /// of a higher version is never decoded or overwritten.
    const VERSION: u32;

    /// The longest stored file accepted, in bytes; a longer one is refused
    /// without being read whole.
    const MAX_BYTES: u64 = 16 * 1024 * 1024;

    /// The value of a document that was never stored.
    fn initial() -> Self;

    /// This value's body bytes. Runs on the owner thread when the value is
    /// set.
    fn encode(&self) -> Vec<u8>;

    /// The value a body written in format `version` holds; `version` is at
    /// most [`VERSION`](Self::VERSION).
    ///
    /// # Errors
    ///
    /// A [`DecodeError`] describing why the body is not a value of this
    /// document.
    fn decode(version: u32, body: &[u8]) -> Result<Self, DecodeError>
    where
        Self: Sized;
}

/// Why [`Document::decode`] could not read a body.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct DecodeError {
    message: String,
}

impl DecodeError {
    /// A decode failure described by `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// How many writes a stored document has seen.
///
/// The revision is kept in the stored file's header, so it keeps counting
/// across runs; [`Persisted::set`] returns the revision of the value it
/// accepted and [`Persisted::committed`] the latest one the storage
/// confirmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Revision(u64);

impl Revision {
    /// The revision as a number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Why a [`Persisted`] document could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PersistError {
    /// The storage refused the request.
    #[error("the storage request failed: {0}")]
    Storage(#[source] StorageError),
    /// The stored bytes are not a document of this name: no header, another
    /// document's header, or a malformed one.
    #[error("the stored document is corrupt: {detail}")]
    Corrupt {
        /// What was wrong with the bytes.
        detail: String,
    },
    /// [`Document::decode`] refused the body.
    #[error("the stored document could not be decoded: {0}")]
    Decode(#[source] DecodeError),
    /// The stored file was written in a newer format than this build reads.
    #[error("the stored document has format version {found}; this build reads up to {supported}")]
    NewerVersion {
        /// The version in the stored file.
        found: u32,
        /// [`Document::VERSION`].
        supported: u32,
    },
    /// The application's codec panicked; the panic was contained.
    #[error("the document's {during} panicked: {message}")]
    Panicked {
        /// Which step panicked, such as `"encode"` or `"decode"`.
        during: &'static str,
        /// The panic's message.
        message: String,
    },
    /// A load was asked for while edits are not yet stored; loading would
    /// replace them.
    #[error("the document has unsaved changes")]
    UnsavedChanges,
    /// The revision counter has no next value; the document takes no more
    /// writes.
    #[error("the document's revision counter is exhausted")]
    Exhausted,
}

/// Where a [`Persisted`] document stands against its storage.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SaveStatus {
    /// No value is loaded, so nothing is written; the error is why the last
    /// load failed, if one did.
    NotLoaded(Option<PersistError>),
    /// The value in memory is the one stored.
    Clean,
    /// A value was handed to the storage, which has not confirmed it yet.
    Saving,
    /// The last write failed; the edits stay in memory and
    /// [`Persisted::retry`] writes the latest value.
    Failed(PersistError),
    /// The stored file is never written: it is from a newer format, or its
    /// directory cannot be locked.
    ReadOnly(PersistError),
    /// The realm has no storage (a platform without files, or none
    /// configured): the document lives in memory only.
    Unavailable,
}
