//! The clipboard's vocabulary (ADR-0152 §5 and §6): what a read of the
//! clipboard or of the primary selection answers, the text shared rather
//! than copied per reader, or why there is none; and what a write asks of
//! the host.

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

/// One write to the clipboard: its text, and whether the clipboard keeps it
/// out of the user's clipboard history and cloud sync (ADR-0152 §6).
///
/// A password manager writes a secret [`concealed`](Self::concealed): the
/// paste still works, but the entry is not kept or shown by the system's
/// clipboard history, synced to the user's other devices, or recorded by a
/// clipboard manager that honours the host's marker. What a host does with
/// it is its [`Concealment`].
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ClipboardWrite {
    text: String,
    concealed: bool,
}

impl ClipboardWrite {
    /// A write of `text`, kept by the clipboard's history like any other.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            concealed: false,
        }
    }

    /// The same write, kept out of the clipboard's history and cloud sync
    /// as far as the host can ([`Concealment`]).
    #[must_use]
    pub fn concealed(mut self) -> Self {
        self.concealed = true;
        self
    }

    /// The text written.
    #[must_use]
    pub fn as_text(&self) -> &str {
        &self.text
    }

    /// Whether the write asks to be kept out of history and sync.
    #[must_use]
    pub fn is_concealed(&self) -> bool {
        self.concealed
    }

    /// The text written, given up by the write.
    #[must_use]
    pub fn into_text(self) -> String {
        self.text
    }
}

impl std::fmt::Debug for ClipboardWrite {
    // Hand-written: a concealed write is a secret, and any write may be
    // private to the user. Its length and marker are safe to show.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardWrite")
            .field("len", &self.text.len())
            .field("concealed", &self.concealed)
            .finish()
    }
}

/// What a host does with a [`concealed`](ClipboardWrite::concealed) write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Concealment {
    /// The system keeps the entry out of its clipboard history and cloud
    /// sync (Windows' history formats, macOS's concealed pasteboard type).
    Honored,
    /// The host marks the entry, and only what honours the mark keeps it
    /// out: a keyboard's preview on Android, a KDE or GNOME clipboard
    /// manager on Linux, the universal clipboard on iOS.
    HintOnly,
    /// The host has no way to mark it: the write is an ordinary one.
    Unavailable,
}
