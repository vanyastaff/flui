//! Clipboard implementation using arboard
//!
//! Provides cross-platform clipboard access using the arboard library.

use std::sync::Arc;

use parking_lot::Mutex;

use flui_platform::Clipboard;

/// Arboard-based clipboard implementation
///
/// Thread-safe wrapper around arboard::Clipboard. The inner slot is `None`
/// for the inert fallback: on a pure-Wayland session (no X11 socket at
/// all, e.g. weston headless — and no compositor support for the
/// wlr-data-control protocol arboard's optional Wayland path needs),
/// clipboard init has no backend to reach, and a platform must come up
/// with a non-functional clipboard rather than not come up at all.
pub struct ArboardClipboard {
    clipboard: Mutex<Option<arboard::Clipboard>>,
}

impl std::fmt::Debug for ArboardClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `arboard::Clipboard` does not implement `Debug`; there is nothing
        // else worth printing about this wrapper.
        f.debug_struct("ArboardClipboard").finish_non_exhaustive()
    }
}

impl ArboardClipboard {
    /// Create a new clipboard instance
    pub fn try_new() -> Result<Self, arboard::Error> {
        let clipboard = arboard::Clipboard::new()?;
        Ok(Self {
            clipboard: Mutex::new(Some(clipboard)),
        })
    }

    /// An inert clipboard for sessions where no clipboard backend is
    /// reachable: every read answers `None`, every write is dropped with a
    /// warning. The platform's failed-initialization fallback — it used to
    /// call [`Self::try_new`] again and `expect` it, which panicked the whole
    /// app at startup with the exact failure the fallback existed to absorb
    /// (observed on a Wayland-only session, where arboard has no X11 socket
    /// to reach). Deliberately a separate constructor rather than the
    /// [`Default`] impl: `default()` still initializes the real system
    /// clipboard when it can.
    pub fn inert() -> Self {
        Self {
            clipboard: Mutex::new(None),
        }
    }

    /// The winit host's clipboard: the system's, or the inert one when its
    /// initialization fails (a headless environment).
    pub(crate) fn or_inert() -> Arc<Self> {
        // `inert()` explicitly, not `default()`: init just failed, and
        // `default()` would retry the same doomed backend init before
        // falling back — the fallback for a KNOWN-failed init is the inert
        // clipboard directly.
        Self::try_new().map_or_else(
            |err| {
                tracing::warn!(?err, "Failed to initialize clipboard, using inert fallback");
                Arc::new(Self::inert())
            },
            Arc::new,
        )
    }
}

/// Runs one `arboard` call as a clipboard session. On Windows `arboard`
/// opens the Win32 clipboard with a `NULL` owner, which does not exclude
/// other threads of this process, so it must share the process-wide session
/// lock with the Win32 backend (`clipboard::lock`); a per-instance
/// `Mutex` cannot, since several instances of either backend may coexist.
/// Elsewhere a pass-through.
fn clipboard_session<R>(call: impl FnOnce() -> R) -> R {
    #[cfg(windows)]
    {
        super::lock::with_clipboard_session(call)
    }
    #[cfg(not(windows))]
    {
        call()
    }
}

impl Default for ArboardClipboard {
    /// The system clipboard when a backend is reachable, the inert fallback
    /// otherwise — never a panic. `default()` on a healthy desktop session
    /// must yield a *functional* clipboard (an unconditionally inert
    /// `default()` would silently discard every copy/paste for callers that
    /// construct through `Default`); only a failed backend init degrades to
    /// [`Self::inert`].
    fn default() -> Self {
        Self::try_new().unwrap_or_else(|err| {
            tracing::warn!(
                ?err,
                "clipboard backend unreachable; using the inert clipboard"
            );
            Self::inert()
        })
    }
}

impl Clipboard for ArboardClipboard {
    fn read_text(&self) -> Option<String> {
        let mut clipboard = self.clipboard.lock();
        let Some(clipboard) = clipboard.as_mut() else {
            tracing::debug!("clipboard read on an inert (backend-less) clipboard");
            return None;
        };

        match clipboard_session(|| clipboard.get_text()) {
            Ok(text) => {
                tracing::debug!(len = text.len(), "Read text from clipboard");
                Some(text)
            }
            Err(err) => {
                tracing::warn!(?err, "Failed to read clipboard text");
                None
            }
        }
    }

    fn write_text(&self, text: String) {
        let mut clipboard = self.clipboard.lock();
        let Some(clipboard) = clipboard.as_mut() else {
            tracing::warn!(
                len = text.len(),
                "clipboard write dropped: no clipboard backend was reachable at platform init"
            );
            return;
        };

        match clipboard_session(|| clipboard.set_text(&text)) {
            Ok(()) => {
                tracing::debug!(len = text.len(), "Wrote text to clipboard");
            }
            Err(err) => {
                tracing::error!(?err, "Failed to write clipboard text");
            }
        }
    }
}
