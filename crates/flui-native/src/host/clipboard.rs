//! The clipboard position's vocabulary: what a host's clipboard answers
//! (ADR-0152), and the in-memory clipboard a loop holds where the host has no
//! system clipboard. It lives beside the table, not in the `clipboard` feature,
//! because a loop holds it and the owner thread names no feature.

use std::sync::Arc;
use std::time::Duration;

use flui_platform::{
    ClipboardEntryId, ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten, Concealment,
    ExpiryClearing, capability::Resolver,
};

/// The host's plain-text clipboard, owner-local: what
/// [`Platform::clipboard`](crate::Platform::clipboard) hands out and the
/// application's clipboard provider builds its handles over (ADR-0152).
///
/// A read, a write and a conditional clear answer through their
/// [`Resolver`]: at once where the operating system answers on the owner
/// thread (AppKit, UIKit, Android), from the clipboard's own thread on Win32,
/// whose clipboard another process may hold or render late (ADR-0337), when
/// the browser's promise settles on the web, when the selection is owned or
/// read on X11 and Wayland. It never calls back into its caller.
///
/// Requests are served in the order they are asked: a read or a clear asked
/// after a write is served once that write landed or failed (ADR-0337 §3,
/// ADR-0363).
pub trait NativeClipboard {
    /// Read the clipboard's text and answer `answer`: `Ok(None)` when it
    /// holds no text, a [`ClipboardError`](flui_platform::ClipboardError) when
    /// the host could not read it.
    fn read_text(&self, answer: Resolver<ClipboardRead>);

    /// Replace the clipboard's text with `write`'s, kept out of the
    /// clipboard's history and cloud sync when it is concealed as far as
    /// [`concealment`](Self::concealment) says (ADR-0152 §6), and answer
    /// `answer`: `Ok(())` once the clipboard holds it, the
    /// [`ClipboardError`](flui_platform::ClipboardError) the host gave up with
    /// otherwise (ADR-0337).
    ///
    /// When the write [expires](ClipboardWrite::expires_after) and lands, the
    /// host records its [entry](ClipboardWrite::entry) with the operating
    /// system's token for it, which
    /// [`clear_expired`](Self::clear_expired) compares; any other write that
    /// lands forgets the record (ADR-0363).
    fn write(&self, write: ClipboardWrite, answer: Resolver<ClipboardWritten>);

    /// What this host does with a concealed write.
    fn concealment(&self) -> Concealment;

    /// What this host does with an expiring write: whether it can clear the
    /// entry only while the clipboard still holds it, and whether it hears
    /// the session lock. Constant for a host.
    fn expiry_clearing(&self) -> ExpiryClearing;

    /// Clear the clipboard if it still holds the entry `entry` names, as the
    /// host recorded it when its write landed and as the operating system
    /// shows it now, and answer what was done (ADR-0363). Never clears a
    /// later copy, by this application or another one. Each host states
    /// where the check and the clear are one step of the operating system
    /// and where a copy made between them can still be taken for the entry.
    fn clear_expired(&self, entry: ExpiredEntry, answer: Resolver<ExpiredClear>);

    /// The loop ends: clear whatever expiring entry of this application the
    /// clipboard still holds, as [`clear_expired`](Self::clear_expired) does
    /// for [`ExpiredEntry::Any`], waiting at most `within` for a host that
    /// serves the clipboard off the owner thread. Answers what was done:
    /// [`ExpiredClear::NotHeld`] where the entry ends with the process anyway
    /// (an X11 or Wayland selection), [`ExpiredClear::Failed`] with
    /// [`ClipboardError::Timeout`] when the wait ran out.
    fn clear_expired_before_exit(&self, within: Duration) -> ExpiredClear;
}

/// Which expiring entry a conditional clear may clear (ADR-0363).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExpiredEntry {
    /// The entry this write made, whose time has come.
    Entry(ClipboardEntryId),
    /// Whatever expiring entry of this application the clipboard holds: the
    /// session locked, the machine sleeps or the loop ends.
    Any,
}

impl ExpiredEntry {
    /// Whether a clear of this applies to the entry `held`.
    #[must_use]
    pub fn names(self, held: ClipboardEntryId) -> bool {
        match self {
            Self::Entry(entry) => entry == held,
            Self::Any => true,
        }
    }
}

/// What a conditional clear did ([`NativeClipboard::clear_expired`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExpiredClear {
    /// The clipboard still held the entry, and it is cleared.
    Cleared,
    /// The clipboard no longer holds the entry: a later write of this
    /// application or another one replaced it, it was cleared already, or
    /// its write never landed. Nothing changed.
    NotHeld,
    /// The host cannot see the clipboard now, so cannot tell whether it still
    /// holds the entry (Android, for an application without input focus):
    /// nothing changed, and a later attempt may tell.
    Unseen,
    /// The host has no conditional clear ([`ExpiryClearing::Unavailable`]):
    /// nothing changed.
    Unsupported,
    /// The host could not reach the clipboard to check or clear it; the entry
    /// may still be there.
    Failed(ClipboardError),
}

impl ExpiredClear {
    /// Whether another attempt later can do what this one could not: the
    /// clipboard was out of sight or out of reach for now.
    #[must_use]
    pub fn is_worth_a_retry(self) -> bool {
        match self {
            Self::Unseen
            | Self::Failed(
                ClipboardError::Unavailable | ClipboardError::Timeout | ClipboardError::Denied,
            ) => true,
            Self::Cleared | Self::NotHeld | Self::Unsupported | Self::Failed(_) => false,
        }
    }
}

/// The entry of the last expiring write a host landed, with the operating
/// system's token for it then (a sequence number, a change count, a marker
/// the entry carries): the pure part of every host's conditional clear
/// (ADR-0363). A host compares the token the operating system shows at the
/// clear with the recorded one; any other write that lands forgets it.
#[derive(Debug)]
pub(crate) struct ExpiringRecord<T> {
    held: Option<(ClipboardEntryId, T)>,
}

impl<T> Default for ExpiringRecord<T> {
    fn default() -> Self {
        Self { held: None }
    }
}

/// What a host does with a conditional clear, given its record and the
/// operating system's token now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClearDecision {
    /// The entry is still there: clear it, which lands as a write of nothing
    /// that does not expire ([`ExpiringRecord::landed`] with `None`).
    Clear,
    /// The entry is not there, or not the one named: answer
    /// [`ExpiredClear::NotHeld`].
    NotHeld,
}

impl<T: PartialEq> ExpiringRecord<T> {
    /// A write landed: record its entry and `token` when it expires
    /// (`entry` is `Some`), forget the earlier one otherwise.
    pub(crate) fn landed(&mut self, entry: Option<ClipboardEntryId>, token: T) {
        self.held = entry.map(|entry| (entry, token));
    }

    /// What a clear of `which` does while the operating system shows `now`.
    /// An entry whose token no longer matches is gone for good, and the
    /// record forgets it.
    pub(crate) fn decide(&mut self, which: ExpiredEntry, now: &T) -> ClearDecision {
        let Some((held, token)) = &self.held else {
            return ClearDecision::NotHeld;
        };
        if token != now {
            self.held = None;
            return ClearDecision::NotHeld;
        }
        if which.names(*held) {
            ClearDecision::Clear
        } else {
            ClearDecision::NotHeld
        }
    }
}

/// The most bytes of text a clipboard or primary-selection read answers, on
/// every host: past it the read answers
/// [`ClipboardError::TooLarge`](flui_platform::ClipboardError::TooLarge).
pub(crate) const MAX_TEXT: usize = 64 * 1024 * 1024;

/// Whether a read of `len` bytes is within [`MAX_TEXT`]: the same predicate
/// [`admit_text`] applies, without its warning, for a reader that must decide
/// while it still holds an OS lock and admit once the lock is released.
pub(crate) const fn text_fits(len: usize) -> bool {
    len <= MAX_TEXT
}

/// Refuse a read of `len` bytes — or, for a UTF-16 source, `len` code units,
/// each at least one UTF-8 byte — past [`MAX_TEXT`], before its copy is made.
///
/// Every check of a finished read's size goes through here, so the limit, the
/// refusal and its warning are the same on every host. A reader that stops a
/// stream at a cap while it still reads (the Wayland pipe, an X11 property
/// read) takes [`MAX_TEXT`] as that cap instead: it has no length to admit
/// until it has stopped.
///
/// The refusal emits a `tracing` event, which is user code: call this with none
/// of the host's own state locked or borrowed.
pub(crate) fn admit_text(len: usize) -> Result<(), flui_platform::ClipboardError> {
    if !text_fits(len) {
        tracing::warn!(len, "a clipboard read past {MAX_TEXT} bytes was refused");
        return Err(flui_platform::ClipboardError::TooLarge);
    }
    Ok(())
}

/// What a read answers with `text` the application wrote and the host still
/// holds: shared, not copied, and refused past [`MAX_TEXT`] bytes as text
/// from any other client is, so a read's answer never depends on who owns
/// the clipboard.
pub(crate) fn owned_text(text: &Arc<str>) -> ClipboardRead {
    admit_text(text.len())?;
    Ok(Some(Arc::clone(text)))
}

/// What a read answers with `text` the host handed over whole: refused past
/// [`MAX_TEXT`] bytes before the shared copy is made, so text a host already
/// holds is never copied a second time at any size the read refuses.
#[cfg(any(
    windows,
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_arch = "wasm32",
    wayland_host,
    x11_host,
))]
pub(crate) fn host_text(text: String) -> ClipboardRead {
    admit_text(text.len())?;
    Ok(Some(Arc::from(text)))
}

/// A clipboard the operating system reads and writes synchronously on the
/// owner thread, with no other process able to keep it waiting: its read
/// and its write answer at once.
pub(crate) trait SyncClipboard {
    /// The clipboard's text now, or why it could not be read.
    fn read_now(&self) -> ClipboardRead;

    /// Replace the clipboard's text with `write`'s, or say why it could not.
    fn write_now(&self, write: ClipboardWrite) -> ClipboardWritten;

    /// What this host does with a concealed write.
    fn concealment(&self) -> Concealment;

    /// What this host does with an expiring write.
    fn expiry_clearing(&self) -> ExpiryClearing;

    /// Clear the clipboard if it still holds the entry `entry` names, or say
    /// what stopped it.
    fn clear_expired_now(&self, entry: ExpiredEntry) -> ExpiredClear;
}

impl<C: SyncClipboard> NativeClipboard for C {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        // A retired owner has nobody left to answer.
        let _ = answer.resolve(self.read_now());
    }

    fn write(&self, write: ClipboardWrite, answer: Resolver<ClipboardWritten>) {
        // A retired owner has nobody left to answer.
        let _ = answer.resolve(self.write_now(write));
    }

    fn concealment(&self) -> Concealment {
        SyncClipboard::concealment(self)
    }

    fn expiry_clearing(&self) -> ExpiryClearing {
        SyncClipboard::expiry_clearing(self)
    }

    fn clear_expired(&self, entry: ExpiredEntry, answer: Resolver<ExpiredClear>) {
        // A retired owner has nobody left to answer: the clear is done.
        let _ = answer.resolve(self.clear_expired_now(entry));
    }

    /// Answered at once: the operating system clears on the owner thread.
    fn clear_expired_before_exit(&self, _within: Duration) -> ExpiredClear {
        self.clear_expired_now(ExpiredEntry::Any)
    }
}

/// A clipboard a loop holds for its whole life, which a `Send + Sync`
/// platform keeps in its state; each [`Platform::clipboard`](crate::Platform::clipboard)
/// hands out an owner-local `Rc` over it.
pub(crate) trait SharedClipboard: NativeClipboard + Send + Sync {}

impl<C: NativeClipboard + Send + Sync> SharedClipboard for C {}

impl<C: NativeClipboard + ?Sized> NativeClipboard for Arc<C> {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        (**self).read_text(answer);
    }

    fn write(&self, write: ClipboardWrite, answer: Resolver<ClipboardWritten>) {
        (**self).write(write, answer);
    }

    fn concealment(&self) -> Concealment {
        (**self).concealment()
    }

    fn expiry_clearing(&self) -> ExpiryClearing {
        (**self).expiry_clearing()
    }

    fn clear_expired(&self, entry: ExpiredEntry, answer: Resolver<ExpiredClear>) {
        (**self).clear_expired(entry, answer);
    }

    fn clear_expired_before_exit(&self, within: Duration) -> ExpiredClear {
        (**self).clear_expired_before_exit(within)
    }
}

/// The in-memory clipboard of the headless host and of a host without a
/// system clipboard yet: it reads back what the application wrote.
///
/// An expiring write is cleared by its entry while no write replaced it; the
/// check and the clear are one step under the clipboard's lock, so nothing
/// written meanwhile is taken for the entry.
#[derive(Default)]
pub struct MemoryClipboard {
    state: parking_lot::Mutex<MemoryState>,
}

/// What the in-memory clipboard holds.
#[derive(Default)]
struct MemoryState {
    text: Option<Arc<str>>,
    /// How many writes changed the text, the token an expiring entry is
    /// recorded with.
    revision: u64,
    expiring: ExpiringRecord<u64>,
}

impl MemoryState {
    /// Hold `text` from now on, a write that makes `entry` when it expires.
    fn hold(&mut self, text: Option<Arc<str>>, entry: Option<ClipboardEntryId>) {
        self.text = text;
        self.revision = self
            .revision
            .checked_add(1)
            .expect("BUG: an in-memory clipboard takes fewer than 2^64 writes");
        self.expiring.landed(entry, self.revision);
    }
}

impl MemoryClipboard {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Another application copies `text`: the clipboard holds it, and no
    /// entry of this application's is there any more.
    #[cfg(test)]
    pub(crate) fn copy_from_elsewhere(&self, text: impl Into<String>) {
        self.state
            .lock()
            .hold(Some(Arc::<str>::from(Into::<String>::into(text))), None);
    }
}

impl std::fmt::Debug for MemoryClipboard {
    // Hand-written: the text may be private to the user. Whether it holds
    // any is safe to show.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryClipboard")
            .field("has_text", &self.state.lock().text.is_some())
            .finish()
    }
}

impl SyncClipboard for MemoryClipboard {
    fn read_now(&self) -> ClipboardRead {
        // Clone out and release the guard first: the admission may emit a
        // `tracing` event, and a subscriber is user code that may re-enter
        // this clipboard.
        let text: Option<Arc<str>> = self.state.lock().text.clone();
        text.as_ref().map_or(Ok(None), owned_text)
    }

    fn write_now(&self, write: ClipboardWrite) -> ClipboardWritten {
        let entry = write.entry();
        let text = Arc::<str>::from(write.into_text());
        self.state.lock().hold(Some(text), entry);
        Ok(())
    }

    /// Nothing keeps a history of the in-memory clipboard.
    fn concealment(&self) -> Concealment {
        Concealment::Honored
    }

    /// The headless host delivers the session lock its test reports.
    fn expiry_clearing(&self) -> ExpiryClearing {
        ExpiryClearing::AtTimeAndLock
    }

    fn clear_expired_now(&self, entry: ExpiredEntry) -> ExpiredClear {
        // The text cleared drops after the guard: it holds no user code, but
        // a secret is let go of with nothing held either way.
        let cleared = {
            let mut state = self.state.lock();
            let now = state.revision;
            match state.expiring.decide(entry, &now) {
                ClearDecision::NotHeld => return ExpiredClear::NotHeld,
                ClearDecision::Clear => {
                    let text = state.text.take();
                    state.hold(None, None);
                    text
                }
            }
        };
        drop(cleared);
        ExpiredClear::Cleared
    }
}

/// The host's primary selection, owner-local: what
/// [`Platform::primary_selection`](crate::Platform::primary_selection) hands
/// out where the platform has one (X11 and Wayland), and the application's
/// primary selection provider builds its handles over (ADR-0152).
///
/// The application offers its selection lazily: the host asks `text` for
/// the selected text only when another client reads it, on the owner thread
/// at a turn of the loop, never inside the platform's own dispatch, so
/// `text` may read the widget state it shares and call back into this
/// selection.
pub trait NativePrimarySelection {
    /// Make the application's selection the primary selection, answered by
    /// `text` whenever it is read, in place of what it offered before.
    fn offer(&self, text: std::rc::Rc<dyn Fn() -> Option<String>>);

    /// Stop offering the application's selection, if it still holds the
    /// primary selection.
    fn withdraw(&self);

    /// Whether the application's selection is the primary selection: it
    /// offered one, withdrew none since, and no other client selected.
    fn owns(&self) -> bool;

    /// Read the primary selection's text and answer `answer`: `Ok(None)`
    /// when it holds none, a [`ClipboardError`](flui_platform::ClipboardError)
    /// when the host could not read it.
    fn read_text(&self, answer: Resolver<ClipboardRead>);
}

impl<C: NativePrimarySelection + ?Sized> NativePrimarySelection for Arc<C> {
    fn offer(&self, text: std::rc::Rc<dyn Fn() -> Option<String>>) {
        (**self).offer(text);
    }

    fn withdraw(&self) {
        (**self).withdraw();
    }

    fn owns(&self) -> bool {
        (**self).owns()
    }

    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        (**self).read_text(answer);
    }
}

#[cfg(all(
    test,
    any(
        windows,
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_arch = "wasm32",
        wayland_host,
        x11_host,
    )
))]
mod tests {
    use flui_platform::ClipboardError;

    use super::{MAX_TEXT, admit_text, host_text};

    /// Text a host handed over is answered up to the limit and refused past
    /// it, the refusal made before any copy of the text.
    #[test]
    fn host_text_is_refused_past_the_limit() {
        let at_limit = "a".repeat(MAX_TEXT);
        assert_eq!(
            host_text(at_limit).map(|text| text.map(|text| text.len())),
            Ok(Some(MAX_TEXT))
        );
        let past = "a".repeat(MAX_TEXT + 1);
        assert_eq!(host_text(past), Err(ClipboardError::TooLarge));
    }

    /// Text a host holds as UTF-16 is refused on its length before a copy
    /// past the limit is made; text within it is measured exactly as UTF-8,
    /// so two-byte characters that fit as units but not as bytes are refused
    /// once copied.
    #[test]
    fn utf16_text_past_the_limit_is_refused_before_its_copy() {
        assert_eq!(admit_text(MAX_TEXT + 1), Err(ClipboardError::TooLarge));
        assert_eq!(admit_text(MAX_TEXT), Ok(()));

        let units = MAX_TEXT / 2 + 1;
        assert_eq!(admit_text(units), Ok(()));
        assert_eq!(
            host_text("é".repeat(units)),
            Err(ClipboardError::TooLarge),
            "admitted on units, refused on its {} bytes",
            2 * units
        );
    }
}

#[cfg(test)]
mod owned_tests {
    use std::time::Duration;

    use flui_platform::{ClipboardError, ClipboardExpiry, ClipboardWrite};

    use super::{
        ClearDecision, ExpiredClear, ExpiredEntry, ExpiringRecord, MAX_TEXT, MemoryClipboard,
        SyncClipboard as _,
    };

    fn expiring(text: &str) -> ClipboardWrite {
        ClipboardWrite::text(text).expires_after(
            ClipboardExpiry::try_new(Duration::from_secs(30)).expect("thirty seconds expire"),
        )
    }

    /// What happens between an expiring write and its clear.
    enum Between {
        Nothing,
        AnotherApplicationCopies,
        AnOrdinaryWriteOfOurs,
        ASecondExpiringWrite,
        TheSameClearBefore,
    }

    /// The in-memory clipboard clears an expiring entry only while it still
    /// holds it: never a later copy of another application's or of this
    /// one, never twice, and a second expiring write makes the first's
    /// clear do nothing.
    #[test]
    fn an_expiring_entry_is_cleared_only_while_it_is_still_there() {
        use Between::*;
        for (name, between, by_entry, outcome, left) in [
            ("its time", Nothing, true, ExpiredClear::Cleared, None),
            ("a lock", Nothing, false, ExpiredClear::Cleared, None),
            (
                "another application's copy",
                AnotherApplicationCopies,
                true,
                ExpiredClear::NotHeld,
                Some("theirs"),
            ),
            (
                "another application's copy at a lock",
                AnotherApplicationCopies,
                false,
                ExpiredClear::NotHeld,
                Some("theirs"),
            ),
            (
                "an ordinary write of ours",
                AnOrdinaryWriteOfOurs,
                false,
                ExpiredClear::NotHeld,
                Some("mine"),
            ),
            (
                "a second expiring write",
                ASecondExpiringWrite,
                true,
                ExpiredClear::NotHeld,
                Some("second"),
            ),
            (
                "a clear that already ran",
                TheSameClearBefore,
                true,
                ExpiredClear::NotHeld,
                None,
            ),
        ] {
            let clipboard = MemoryClipboard::new();
            let first = expiring("hunter2");
            let entry = first.entry().expect("an expiring write names its entry");
            assert_eq!(clipboard.write_now(first), Ok(()), "{name}");
            match between {
                Nothing => {}
                AnotherApplicationCopies => clipboard.copy_from_elsewhere("theirs"),
                AnOrdinaryWriteOfOurs => {
                    assert_eq!(clipboard.write_now(ClipboardWrite::text("mine")), Ok(()));
                }
                ASecondExpiringWrite => {
                    assert_eq!(clipboard.write_now(expiring("second")), Ok(()));
                }
                TheSameClearBefore => assert_eq!(
                    clipboard.clear_expired_now(ExpiredEntry::Entry(entry)),
                    ExpiredClear::Cleared
                ),
            }
            let which = if by_entry {
                ExpiredEntry::Entry(entry)
            } else {
                ExpiredEntry::Any
            };
            assert_eq!(clipboard.clear_expired_now(which), outcome, "{name}");
            assert_eq!(
                clipboard
                    .read_now()
                    .expect("the in-memory clipboard reads")
                    .as_deref(),
                left,
                "{name}"
            );
        }
    }

    /// A token that no longer matches is forgotten: the entry is gone for
    /// good, even if a later token compares equal again, and a clear of one
    /// entry never takes another's.
    #[test]
    fn a_record_forgets_an_entry_its_token_no_longer_matches() {
        let first = expiring("a").entry().expect("expiring");
        let second = expiring("b").entry().expect("expiring");
        let mut record = ExpiringRecord::<u32>::default();
        assert_eq!(
            record.decide(ExpiredEntry::Any, &1),
            ClearDecision::NotHeld,
            "nothing recorded"
        );
        record.landed(Some(first), 7);
        assert_eq!(
            record.decide(ExpiredEntry::Entry(second), &7),
            ClearDecision::NotHeld,
            "another entry's clear"
        );
        assert_eq!(
            record.decide(ExpiredEntry::Entry(first), &7),
            ClearDecision::Clear
        );
        assert_eq!(
            record.decide(ExpiredEntry::Entry(first), &8),
            ClearDecision::NotHeld
        );
        assert_eq!(
            record.decide(ExpiredEntry::Entry(first), &7),
            ClearDecision::NotHeld,
            "forgotten once the token moved"
        );
        record.landed(Some(first), 9);
        record.landed(None, 10);
        assert_eq!(
            record.decide(ExpiredEntry::Any, &10),
            ClearDecision::NotHeld,
            "a write that does not expire forgets the entry"
        );
    }

    /// Text the application wrote is read back up to the limit and refused
    /// past it, as text from any other client is.
    #[test]
    fn owned_text_is_refused_past_the_limit() {
        let clipboard = MemoryClipboard::new();
        assert_eq!(
            clipboard.write_now(ClipboardWrite::text("a".repeat(MAX_TEXT))),
            Ok(())
        );
        assert_eq!(
            clipboard.read_now().map(|text| text.map(|text| text.len())),
            Ok(Some(MAX_TEXT))
        );
        assert_eq!(
            clipboard.write_now(ClipboardWrite::text("a".repeat(MAX_TEXT + 1))),
            Ok(())
        );
        assert_eq!(clipboard.read_now(), Err(ClipboardError::TooLarge));
    }
}
