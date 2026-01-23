//! Clipboard implementation using arboard
//!
//! Provides cross-platform clipboard access using the arboard library.

use crate::traits::Clipboard;
use parking_lot::Mutex;

/// Arboard-based clipboard implementation
///
/// Thread-safe wrapper around arboard::Clipboard.
pub struct ArboardClipboard {
    clipboard: Mutex<arboard::Clipboard>,
}

impl ArboardClipboard {
    /// Create a new clipboard instance
    pub fn new() -> Result<Self, arboard::Error> {
        let clipboard = arboard::Clipboard::new()?;
        Ok(Self {
            clipboard: Mutex::new(clipboard),
        })
    }
}

impl Default for ArboardClipboard {
    fn default() -> Self {
        Self::new().expect("Failed to initialize clipboard")
    }
}

impl Clipboard for ArboardClipboard {
    fn read_text(&self) -> Option<String> {
        let mut clipboard = self.clipboard.lock();

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

        match clipboard.set_text(&text) {
            Ok(_) => {
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
