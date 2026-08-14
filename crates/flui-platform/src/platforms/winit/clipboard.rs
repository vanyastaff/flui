//! Clipboard implementation using arboard
//!
//! Provides cross-platform clipboard access using the arboard library.

use parking_lot::Mutex;

use crate::traits::Clipboard;

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
    pub fn new() -> Result<Self, arboard::Error> {
        let clipboard = arboard::Clipboard::new()?;
        Ok(Self {
            clipboard: Mutex::new(Some(clipboard)),
        })
    }

    /// An inert clipboard for sessions where no clipboard backend is
    /// reachable: every read answers `None`, every write is dropped with a
    /// warning. The platform's failed-initialization fallback — it used to
    /// call [`Self::new`] again and `expect` it, which panicked the whole
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

    /// Whether this instance is the backend-less fallback. Test-only: the
    /// `Default`-tracks-backend-availability pin needs an observable that
    /// does not depend on clipboard *contents* (roundtrips through a real
    /// X11 clipboard are timing-sensitive under a virtual display).
    #[cfg(test)]
    fn is_inert(&self) -> bool {
        self.clipboard.lock().is_none()
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
        Self::new().unwrap_or_else(|err| {
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

        match clipboard.get_text() {
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

        match clipboard.set_text(&text) {
            Ok(()) => {
                tracing::debug!(len = text.len(), "Wrote text to clipboard");
            }
            Err(err) => {
                tracing::error!(?err, "Failed to write clipboard text");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_roundtrip() {
        // Note: This test requires clipboard access and may fail in CI
        if let Ok(clipboard) = ArboardClipboard::new() {
            let test_text = "Hello from FLUI!";

            clipboard.write_text(test_text.to_string());

            if let Some(read_text) = clipboard.read_text() {
                assert_eq!(read_text, test_text);
            }
        }
    }

    #[test]
    fn test_clipboard_creation() {
        // Just test that we can create a clipboard instance
        let result = ArboardClipboard::new();

        // This may fail in headless environments, which is expected
        if result.is_err() {
            eprintln!("Note: Clipboard creation failed (expected in headless environments)");
        }
    }

    /// `default()` must track backend availability exactly: functional when
    /// `new()` would succeed (an unconditionally inert `default()` silently
    /// discards every write on a healthy desktop session), inert — and
    /// crucially non-panicking — when no backend is reachable (the old
    /// fallback `expect`ed the same failed init it existed to absorb,
    /// aborting any Wayland-only session at startup).
    #[test]
    fn default_is_functional_iff_a_backend_is_reachable_and_never_panics() {
        // Completing at all pins "never panics" for whichever environment
        // (X11-backed or backend-less) this test happens to run under.
        let clipboard = ArboardClipboard::default();
        assert_eq!(
            clipboard.is_inert(),
            ArboardClipboard::new().is_err(),
            "default() must yield a functional clipboard exactly when the backend \
             is reachable, and the inert fallback exactly when it is not"
        );
    }

    /// The inert fallback's whole contract: reads answer `None`, writes are
    /// dropped, nothing panics.
    #[test]
    fn inert_clipboard_reads_none_and_drops_writes_without_panicking() {
        let clipboard = ArboardClipboard::inert();
        clipboard.write_text("dropped".to_string());
        assert_eq!(clipboard.read_text(), None);
    }
}
