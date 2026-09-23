//! Web clipboard implementation
//!
//! Uses the Navigator Clipboard API (`navigator.clipboard.writeText()`) for
//! writing, with an in-memory fallback for immediate reads. The browser
//! Clipboard API's `readText()` is async and requires a user gesture, so we
//! cannot use it from a synchronous `read_text()` call. Instead, we cache the
//! last written value and return it from `read_text()`.
//!
//! **Limitation:** `read_text()` only returns text that was written by this
//! application within the current session. External clipboard content (e.g.,
//! copied from another tab) is not accessible synchronously.

use parking_lot::Mutex;

use crate::traits::Clipboard;

pub struct WebClipboard {
    /// In-memory fallback: caches the last written text for synchronous reads.
    /// Also serves as the clipboard when the Navigator Clipboard API is
    /// unavailable (e.g., insecure context, older browsers).
    fallback: Mutex<Option<String>>,
}

// SAFETY: WASM is single-threaded — no data races possible
unsafe impl Send for WebClipboard {}
unsafe impl Sync for WebClipboard {}

impl WebClipboard {
    pub fn new() -> Self {
        Self {
            fallback: Mutex::new(None),
        }
    }
}

impl Clipboard for WebClipboard {
    fn read_text(&self) -> Option<String> {
        // navigator.clipboard.readText() is async and requires a user gesture,
        // so we return the cached last-written value instead.
        self.fallback.lock().clone()
    }

    fn write_text(&self, text: String) {
        // Try the Navigator Clipboard API first (fire-and-forget).
        // navigator.clipboard() returns a web_sys::Clipboard directly;
        // writeText() returns a Promise which we discard since the trait is
        // synchronous.
        if let Some(window) = web_sys::window() {
            let clipboard = window.navigator().clipboard();
            let _ = clipboard.write_text(&text);
        }
        // Always update the in-memory fallback for immediate read_text() access
        *self.fallback.lock() = Some(text);
    }

    fn has_text(&self) -> bool {
        self.fallback.lock().is_some()
    }
}
