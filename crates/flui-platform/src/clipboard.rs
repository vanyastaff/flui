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
/// out of the user's clipboard history and cloud sync (ADR-0152 §6), and
/// clears it after a time (ADR-0363).
///
/// A password manager writes a secret [`concealed`](Self::concealed): the
/// paste still works, but the entry is not kept or shown by the system's
/// clipboard history, synced to the user's other devices, or recorded by a
/// clipboard manager that honours the host's marker. What a host does with
/// it is its [`Concealment`]. A secret written
/// [`expires_after`](Self::expires_after) is concealed too, and is cleared
/// at its time and when the session locks if the clipboard still holds it;
/// what a host does with that is its [`ExpiryClearing`].
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ClipboardWrite {
    text: String,
    kept: Kept,
}

/// How the clipboard keeps a write's entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kept {
    /// Like any other entry.
    Ordinary,
    /// Out of history and sync.
    Concealed,
    /// Out of history and sync, and cleared after `after` if still there.
    Expiring {
        after: ClipboardExpiry,
        entry: ClipboardEntryId,
    },
}

impl ClipboardWrite {
    /// A write of `text`, kept by the clipboard's history like any other.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Into::<String>::into(text),
            kept: Kept::Ordinary,
        }
    }

    /// The same write, kept out of the clipboard's history and cloud sync
    /// as far as the host can ([`Concealment`]). An expiring write stays
    /// expiring.
    #[must_use]
    pub fn concealed(mut self) -> Self {
        if self.kept == Kept::Ordinary {
            self.kept = Kept::Concealed;
        }
        self
    }

    /// The same write, concealed, and cleared `after` it is asked for, and
    /// when the user's session locks, if the clipboard still holds this
    /// entry then: a later copy, by this application or another one, is
    /// never cleared (ADR-0363). What the host can do is its
    /// [`ExpiryClearing`].
    ///
    /// The write is named by a new [`ClipboardEntryId`]; a clone of it names
    /// the same entry, so writing a clone twice makes one entry, cleared at
    /// the earlier time.
    ///
    /// # Panics
    ///
    /// When the process has made 2^64 expiring writes: the entry names are
    /// never reissued.
    #[must_use]
    pub fn expires_after(mut self, after: ClipboardExpiry) -> Self {
        self.kept = Kept::Expiring {
            after,
            entry: ClipboardEntryId::mint(),
        };
        self
    }

    /// The text written.
    #[must_use]
    pub fn as_text(&self) -> &str {
        &self.text
    }

    /// Whether the write asks to be kept out of history and sync: a
    /// concealed write, or an expiring one.
    #[must_use]
    pub fn is_concealed(&self) -> bool {
        self.kept != Kept::Ordinary
    }

    /// How long the entry lives, for an expiring write.
    #[must_use]
    pub fn expiry(&self) -> Option<ClipboardExpiry> {
        match self.kept {
            Kept::Expiring { after, .. } => Some(after),
            Kept::Ordinary | Kept::Concealed => None,
        }
    }

    /// The entry an expiring write makes, which the host's conditional clear
    /// names; `None` for a write that does not expire.
    #[must_use]
    pub fn entry(&self) -> Option<ClipboardEntryId> {
        match self.kept {
            Kept::Expiring { entry, .. } => Some(entry),
            Kept::Ordinary | Kept::Concealed => None,
        }
    }

    /// The text written, given up by the write.
    #[must_use]
    pub fn into_text(self) -> String {
        self.text
    }
}

impl std::fmt::Debug for ClipboardWrite {
    // Hand-written: a concealed write is a secret, and any write may be
    // private to the user. Its length, marker and expiry are safe to show.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardWrite")
            .field("len", &self.text.len())
            .field("kept", &self.kept)
            .finish()
    }
}

/// How long a concealed clipboard entry lives before it clears itself, if
/// the clipboard still holds it ([`ClipboardWrite::expires_after`]): from
/// [`MIN`](Self::MIN) to [`MAX`](Self::MAX).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClipboardExpiry(std::time::Duration);

impl ClipboardExpiry {
    /// The shortest expiry: below a second nobody can paste the entry before
    /// it goes.
    pub const MIN: std::time::Duration = std::time::Duration::from_secs(1);

    /// The longest expiry: past a day the entry is a long-lived one, not a
    /// secret on the clipboard for a moment, and every clock the hosts arm
    /// it on adds a day without overflow.
    pub const MAX: std::time::Duration = std::time::Duration::from_hours(24);

    /// An expiry of `after`.
    ///
    /// # Errors
    ///
    /// [`ClipboardExpiryError::TooShort`] below [`MIN`](Self::MIN), zero
    /// included; [`ClipboardExpiryError::TooLong`] past [`MAX`](Self::MAX).
    pub const fn try_new(after: std::time::Duration) -> Result<Self, ClipboardExpiryError> {
        if after.as_nanos() < Self::MIN.as_nanos() {
            return Err(ClipboardExpiryError::TooShort);
        }
        if after.as_nanos() > Self::MAX.as_nanos() {
            return Err(ClipboardExpiryError::TooLong);
        }
        Ok(Self(after))
    }

    /// The time the entry lives.
    #[must_use]
    pub const fn get(self) -> std::time::Duration {
        self.0
    }
}

/// Why a duration is no [`ClipboardExpiry`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum ClipboardExpiryError {
    /// Shorter than [`ClipboardExpiry::MIN`].
    #[error("a clipboard expiry is at least a second")]
    TooShort,
    /// Longer than [`ClipboardExpiry::MAX`].
    #[error("a clipboard expiry is at most a day")]
    TooLong,
}

/// The entry one expiring clipboard write makes, unique in the process and
/// never reissued: the host records it with the operating system's token
/// for the entry when the write lands, and clears the clipboard for it only
/// while both still match (ADR-0363).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClipboardEntryId(std::num::NonZeroU64);

/// The process's expiring clipboard entries.
static CLIPBOARD_ENTRIES: flui_foundation::AtomicIdMint<std::num::NonZeroU64> =
    flui_foundation::AtomicIdMint::new();

impl ClipboardEntryId {
    /// A name no entry had before.
    fn mint() -> Self {
        Self(
            CLIPBOARD_ENTRIES
                .next()
                .expect("BUG: a process makes fewer than 2^64 expiring clipboard writes"),
        )
    }

    /// The raw value, for a host that carries it in the entry itself (an
    /// Android clip's extras) and for diagnostics; there is no conversion
    /// back.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// What a host does with an expiring write ([`ClipboardWrite::expires_after`]):
/// whether it clears the entry at its time and when the session locks, if the
/// clipboard still holds it then.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExpiryClearing {
    /// Cleared at its time and when the user's session locks (or the machine
    /// sleeps), if the clipboard still holds it.
    AtTimeAndLock,
    /// Cleared at its time if the clipboard still holds it; the host hears no
    /// session lock (iOS).
    AtTime,
    /// Cleared at its time if the application can see the clipboard then,
    /// and otherwise when it is next in front, if the clipboard still holds
    /// it; the host hears no session lock (Android, where only the
    /// application with input focus sees the clipboard).
    AtTimeOrInFront,
    /// The host cannot clear an entry only while it is still there (the
    /// web's clipboard has no such form): the write is a concealed one that
    /// stays until something replaces it.
    Unavailable,
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
