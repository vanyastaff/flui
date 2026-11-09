//! The clipboard position's vocabulary: what a host's clipboard answers
//! (ADR-0152), and the in-memory clipboard a loop holds where the host has no
//! system clipboard. It lives beside the table, not in the `clipboard` feature,
//! because a loop holds it and the owner thread names no feature.

use std::sync::Arc;

use flui_platform::{ClipboardRead, ClipboardWrite, Concealment, capability::Resolver};

/// The host's plain-text clipboard, owner-local: what
/// [`Platform::clipboard`](crate::Platform::clipboard) hands out and the
/// application's clipboard provider builds its handles over (ADR-0152).
///
/// A read answers through its [`Resolver`]: at once where the operating
/// system reads synchronously (Win32, AppKit, UIKit, `arboard`), when the
/// browser's promise settles on the web. It never calls back into its caller.
pub trait NativeClipboard {
    /// Read the clipboard's text and answer `answer`: `Ok(None)` when it
    /// holds no text, a [`ClipboardError`](flui_platform::ClipboardError) when
    /// the host could not read it.
    fn read_text(&self, answer: Resolver<ClipboardRead>);

    /// Replace the clipboard's text with `write`'s, kept out of the
    /// clipboard's history and cloud sync when it is concealed as far as
    /// [`concealment`](Self::concealment) says (ADR-0152 §6).
    fn write(&self, write: ClipboardWrite);

    /// What this host does with a concealed write.
    fn concealment(&self) -> Concealment;

    /// Replace the clipboard's text: `write(ClipboardWrite::text(text))`.
    fn write_text(&self, text: String) {
        self.write(ClipboardWrite::text(text));
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

/// A clipboard the operating system reads synchronously: its read answers
/// at once.
pub(crate) trait SyncClipboard {
    /// The clipboard's text now, or why it could not be read.
    fn read_now(&self) -> ClipboardRead;

    /// Replace the clipboard's text with `write`'s.
    fn write_now(&self, write: ClipboardWrite);

    /// What this host does with a concealed write.
    fn concealment(&self) -> Concealment;
}

impl<C: SyncClipboard> NativeClipboard for C {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        // A retired owner has nobody left to answer.
        let _ = answer.resolve(self.read_now());
    }

    fn write(&self, write: ClipboardWrite) {
        self.write_now(write);
    }

    fn concealment(&self) -> Concealment {
        SyncClipboard::concealment(self)
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

    fn write(&self, write: ClipboardWrite) {
        (**self).write(write);
    }

    fn concealment(&self) -> Concealment {
        (**self).concealment()
    }
}

/// The in-memory clipboard of the headless host and of a host without a
/// system clipboard yet: it reads back what the application wrote.
#[derive(Default)]
pub struct MemoryClipboard {
    text: parking_lot::Mutex<Option<Arc<str>>>,
}

impl MemoryClipboard {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for MemoryClipboard {
    // Hand-written: the text may be private to the user. Whether it holds
    // any is safe to show.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryClipboard")
            .field("has_text", &self.text.lock().is_some())
            .finish()
    }
}

impl SyncClipboard for MemoryClipboard {
    fn read_now(&self) -> ClipboardRead {
        // Clone out and release the guard first: the admission may emit a
        // `tracing` event, and a subscriber is user code that may re-enter
        // this clipboard.
        let text: Option<Arc<str>> = self.text.lock().clone();
        text.as_ref().map_or(Ok(None), owned_text)
    }

    fn write_now(&self, write: ClipboardWrite) {
        *self.text.lock() = Some(Arc::<str>::from(write.into_text()));
    }

    /// Nothing keeps a history of the in-memory clipboard.
    fn concealment(&self) -> Concealment {
        Concealment::Honored
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
    use flui_platform::{ClipboardError, ClipboardWrite};

    use super::{MAX_TEXT, MemoryClipboard, SyncClipboard as _};

    /// Text the application wrote is read back up to the limit and refused
    /// past it, as text from any other client is.
    #[test]
    fn owned_text_is_refused_past_the_limit() {
        let clipboard = MemoryClipboard::new();
        clipboard.write_now(ClipboardWrite::text("a".repeat(MAX_TEXT)));
        assert_eq!(
            clipboard.read_now().map(|text| text.map(|text| text.len())),
            Ok(Some(MAX_TEXT))
        );
        clipboard.write_now(ClipboardWrite::text("a".repeat(MAX_TEXT + 1)));
        assert_eq!(clipboard.read_now(), Err(ClipboardError::TooLarge));
    }
}
