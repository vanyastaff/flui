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
    /// warning. The platform's construction fallback — it used to call
    /// [`Self::new`] again and `expect` it, which panicked the whole app at
    /// startup with the exact failure the fallback existed to absorb
    /// (observed on a Wayland-only session, where arboard has no X11 socket
    /// to reach).
    pub fn inert() -> Self {
        Self {
            clipboard: Mutex::new(None),
        }
    }
}

impl Default for ArboardClipboard {
    fn default() -> Self {
        Self::inert()
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
}
