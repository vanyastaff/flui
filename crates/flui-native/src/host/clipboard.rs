//! The clipboard position's vocabulary: what a host's clipboard answers
//! (ADR-0152), and the in-memory clipboard a loop holds where the host has no
//! system clipboard. It lives beside the table, not in the `clipboard` feature,
//! because a loop holds it and the owner thread names no feature.

use std::sync::Arc;

use flui_platform::{ClipboardRead, capability::Resolver};

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

    /// Replace the clipboard's text.
    fn write_text(&self, text: String);
}

/// The most bytes of text a clipboard or primary-selection read answers, on
/// every host: past it the read answers
/// [`ClipboardError::TooLarge`](flui_platform::ClipboardError::TooLarge).
#[cfg(any(
    windows,
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_arch = "wasm32",
    wayland_host,
    x11_host,
    feature = "winit-backend"
))]
pub(crate) const MAX_TEXT: usize = 64 * 1024 * 1024;

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
    feature = "winit-backend"
))]
pub(crate) fn host_text(text: String) -> ClipboardRead {
    if text.len() > MAX_TEXT {
        tracing::warn!(
            len = text.len(),
            "a clipboard read past {MAX_TEXT} bytes was refused"
        );
        return Err(flui_platform::ClipboardError::TooLarge);
    }
    Ok(Some(Arc::from(text)))
}

/// A clipboard the operating system reads synchronously: its read answers
/// at once.
pub(crate) trait SyncClipboard {
    /// The clipboard's text now, or why it could not be read.
    fn read_now(&self) -> ClipboardRead;

    /// Replace the clipboard's text.
    fn write_now(&self, text: String);
}

impl<C: SyncClipboard> NativeClipboard for C {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        // A retired owner has nobody left to answer.
        let _ = answer.resolve(self.read_now());
    }

    fn write_text(&self, text: String) {
        self.write_now(text);
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

    fn write_text(&self, text: String) {
        (**self).write_text(text);
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
        Ok(self.text.lock().clone())
    }

    fn write_now(&self, text: String) {
        *self.text.lock() = Some(Arc::from(text));
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
        feature = "winit-backend"
    )
))]
mod tests {
    use flui_platform::ClipboardError;

    use super::{MAX_TEXT, host_text};

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
}
