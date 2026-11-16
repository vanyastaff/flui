//! The clipboard's vocabulary (ADR-0152 §5 and §6, ADR-0337): what a read of
//! the clipboard or of the primary selection answers, the text shared rather
//! than copied per reader, or why there is none; what a write asks of the
//! host, and whether the host took it.

use std::sync::Arc;

/// The answer to one clipboard or primary-selection read: `Ok(None)` when it
/// holds no text, the text when it does, or why the host could not read it.
///
/// The text is shared: every reader a read answers holds the same
/// allocation, however large.
pub type ClipboardRead = Result<Option<Arc<str>>, ClipboardError>;

/// The answer to one clipboard write (ADR-0337), the same on every host:
///
/// - `Ok(())`: the system clipboard took the text.
/// - `Err(`[`ClipboardError::ApplicationOnly`]`)`: there is no system
///   clipboard for the text to go to, and the host keeps it: this
///   application's later reads answer it until a newer write or a newer
///   system selection, and other applications never see it. A host answers
///   it only where it can hold that promise.
/// - any other error: nothing was kept. The clipboard is as it was, and no
///   later read in this process answers the text, with one exception: a host
///   that must clear the clipboard before it fills it (Win32's
///   `EmptyClipboard`, AppKit's `clearContents`) and is then refused by the
///   system itself leaves the clipboard without text.
///
/// A Wayland compositor confirms no selection request, so a request it drops
/// silently still answers `Ok`. Worth a retry: [`ClipboardError::Unavailable`]
/// and [`ClipboardError::Timeout`] (a clipboard held, busy or slow now), and
/// [`ClipboardError::Denied`] once the user acts. Never worth one:
/// [`ClipboardError::Gone`], the one answer on every host for a clipboard
/// no retry can reach. `Gone` is about this clipboard object (its thread
/// ended, its loop released it, the calling thread is never served);
/// `ApplicationOnly` is about where the text can go (no system clipboard
/// for it), while the clipboard object still works; the two never meet.
pub type ClipboardWritten = Result<(), ClipboardError>;

/// Why a clipboard or primary-selection read has no answer, or a clipboard
/// write did not reach the clipboard.
///
/// A clipboard that holds no text, or only data in another format, is no
/// error: its read answers `Ok(None)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum ClipboardError {
    /// The host could not reach the clipboard now, and a retry can: another
    /// application held it open past the host's patience, the host's queue
    /// of requests or of queued text is full, the selection's owner left
    /// during the transfer, or a Wayland window wrote without holding the
    /// keyboard (a compositor takes a selection only under the serial of an
    /// input the window holds; nothing is kept).
    #[error("the clipboard could not be reached")]
    Unavailable,
    /// The user or the platform refused this application the clipboard: a
    /// browser's clipboard permission, or a mobile system reading for an
    /// application without input focus.
    #[error("the clipboard was denied to this application")]
    Denied,
    /// The clipboard did not answer in time: a selection owner, a Win32
    /// owner that renders its data only when it is asked, or a Win32 request
    /// that waited past its queue's bound for its turn behind such owners.
    #[error("the clipboard's owner did not answer in time")]
    Timeout,
    /// The clipboard held text that is not valid in its declared encoding.
    #[error("the clipboard's text is malformed")]
    Malformed,
    /// The clipboard held more text than the host reads in one answer: the
    /// read stops rather than grow without bound.
    #[error("the clipboard's text is larger than the host reads")]
    TooLarge,
    /// Only a write answers it: there is no system clipboard for the text
    /// to go to (a web page without the Clipboard API; a Wayland seat with
    /// no data device, or a compositor with no data device manager), so the
    /// host keeps it. This application's later reads answer it until a newer
    /// write or a newer system selection; no other application ever sees
    /// it, and the host never offers it to a system clipboard later. A host
    /// answers it only where it can hold that, and so nothing is lost: a cut
    /// may delete on it, as a paste in the application returns the text.
    #[error("only this application can read the text back")]
    ApplicationOnly,
    /// This clipboard object is gone, and no retry can reach it: it could
    /// not be started or has ended (Win32's clipboard thread, X11's
    /// selection thread), the loop that held it released it (Wayland), the
    /// platform gave it no service (an Android activity without a clipboard
    /// manager), or the caller is on a thread the platform never serves
    /// (UIKit off the main thread).
    #[error("the clipboard is gone for this run")]
    Gone,
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
            text: Into::<String>::into(text),
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
