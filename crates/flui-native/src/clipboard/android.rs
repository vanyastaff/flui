//! The Android host's clipboard: an in-process stand-in.

use flui_platform::Clipboard;
use parking_lot::Mutex;

/// Mock clipboard for Android MVP
pub struct MockClipboard {
    content: Mutex<Option<String>>,
}

// Redacted at the source, not just at `AndroidLoop`'s boundary, so a
// future holder of this type cannot reintroduce the leak by deriving its
// own `Debug`. Whether the clipboard is set is safe to show; what it holds
// is the user's data.
impl std::fmt::Debug for MockClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockClipboard")
            .field("has_content", &self.content.lock().is_some())
            .finish()
    }
}

impl MockClipboard {
    pub(crate) fn new() -> Self {
        Self {
            content: Mutex::new(None),
        }
    }
}

impl Clipboard for MockClipboard {
    fn read_text(&self) -> Option<String> {
        self.content.lock().clone()
    }

    fn write_text(&self, text: String) {
        *self.content.lock() = Some(text);
    }
}
