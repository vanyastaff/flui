//! What a read of the clipboard or of the primary selection answers
//! (ADR-0152 §5): the text, shared rather than copied per reader, or why
//! there is none.

use std::sync::Arc;

/// The answer to one clipboard or primary-selection read: `Ok(None)` when it
/// holds no text, the text when it does, or why the host could not read it.
///
/// The text is shared: every reader a read answers holds the same
/// allocation, however large.
pub type ClipboardRead = Result<Option<Arc<str>>, ClipboardError>;

/// Why a clipboard or primary-selection read has no answer.
///
/// A clipboard that holds no text, or only data in another format, is no
/// error: its read answers `Ok(None)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum ClipboardError {
    /// The host could not reach the clipboard: another application holds
    /// it open, the display connection is gone, or the selection's owner
    /// left during the transfer.
    #[error("the clipboard could not be reached")]
    Unavailable,
    /// The user or the platform refused this application the read: a
    /// browser's clipboard permission, or a mobile system reading for an
    /// application without input focus.
    #[error("reading the clipboard was denied")]
    Denied,
    /// The clipboard's owner did not answer in time.
    #[error("the clipboard's owner did not answer in time")]
    Timeout,
    /// The clipboard held text that is not valid in its declared encoding.
    #[error("the clipboard's text is malformed")]
    Malformed,
    /// The clipboard held more text than the host reads in one answer: the
    /// read stops rather than grow without bound.
    #[error("the clipboard's text is larger than the host reads")]
    TooLarge,
}
