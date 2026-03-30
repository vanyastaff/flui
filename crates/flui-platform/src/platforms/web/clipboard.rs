//! Web clipboard implementation (in-memory MVP)

use parking_lot::Mutex;

use crate::traits::Clipboard;

pub struct WebClipboard {
    content: Mutex<Option<String>>,
}

unsafe impl Send for WebClipboard {}
unsafe impl Sync for WebClipboard {}

impl WebClipboard {
    pub fn new() -> Self {
        Self {
            content: Mutex::new(None),
        }
    }
}

impl Clipboard for WebClipboard {
    fn read_text(&self) -> Option<String> {
        self.content.lock().clone()
    }

    fn write_text(&self, text: String) {
        *self.content.lock() = Some(text);
    }
}
