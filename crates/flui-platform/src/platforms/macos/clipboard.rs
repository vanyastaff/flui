//! macOS clipboard implementation using NSPasteboard
//!
//! Provides clipboard access using the macOS Cocoa NSPasteboard API.
//! Thread-safe wrapper with proper Objective-C object lifetime management.

use crate::traits::Clipboard;
use cocoa::appkit::NSPasteboard;
use cocoa::base::{id, nil};
use cocoa::foundation::{NSArray, NSString};
use objc::runtime::Object;
use parking_lot::Mutex;

/// macOS NSPasteboard-based clipboard implementation
///
/// Thread-safe wrapper around NSPasteboard.
/// Uses the general pasteboard ([NSPasteboard generalPasteboard]).
pub struct MacOSClipboard {
    /// Cached reference to general pasteboard (singleton)
    pasteboard: Mutex<id>,
}

impl MacOSClipboard {
    /// Create a new clipboard instance
    ///
    /// Gets the general system pasteboard ([NSPasteboard generalPasteboard]).
    pub fn new() -> Self {
        unsafe {
            let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];

            if pasteboard == nil {
                tracing::error!("Failed to get NSPasteboard.generalPasteboard");
            } else {
                tracing::debug!("Created macOS clipboard (NSPasteboard)");
            }

            Self {
                pasteboard: Mutex::new(pasteboard),
            }
        }
    }

    /// Get the current change count
    ///
    /// Change count increments each time the pasteboard contents change.
    /// Use this to detect if clipboard has changed without reading contents.
    #[allow(dead_code)]
    fn change_count(&self) -> i64 {
        unsafe {
            let pasteboard = *self.pasteboard.lock();
            if pasteboard == nil {
                return 0;
            }
            msg_send![pasteboard, changeCount]
        }
    }
}

impl Default for MacOSClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for MacOSClipboard {
    fn read_text(&self) -> Option<String> {
        unsafe {
            let pasteboard = *self.pasteboard.lock();
            if pasteboard == nil {
                tracing::warn!("Pasteboard is nil, cannot read text");
                return None;
            }

            // NSPasteboardTypeString is the UTI for plain text (kUTTypeUTF8PlainText)
            let string_type = NSString::alloc(nil);
            let string_type = NSString::init_str(string_type, "public.utf8-plain-text");

            // Get available types
            let types: id = msg_send![pasteboard, types];
            if types == nil {
                tracing::debug!("No types available on pasteboard");
                return None;
            }

            // Check if text is available
            let has_string: bool = msg_send![types, containsObject: string_type];

            if !has_string {
                tracing::debug!("Pasteboard does not contain text");
                return None;
            }

            // Read string
            let ns_string: id = msg_send![pasteboard, stringForType: string_type];
            if ns_string == nil {
                tracing::warn!("Failed to read string from pasteboard");
                return None;
            }

            // Convert NSString to Rust String
            let c_str: *const i8 = msg_send![ns_string, UTF8String];
            if c_str.is_null() {
                tracing::warn!("Failed to get UTF8String from NSString");
                return None;
            }

            let rust_string = std::ffi::CStr::from_ptr(c_str)
                .to_string_lossy()
                .into_owned();

            tracing::debug!(len = rust_string.len(), "Read text from clipboard");
            Some(rust_string)
        }
    }

    fn write_text(&self, text: String) {
        unsafe {
            let pasteboard = *self.pasteboard.lock();
            if pasteboard == nil {
                tracing::error!("Pasteboard is nil, cannot write text");
                return;
            }

            // Clear existing contents
            let _: i64 = msg_send![pasteboard, clearContents];

            // Create NSString from Rust String
            let ns_string = NSString::alloc(nil);
            let ns_string = NSString::init_str(ns_string, &text);

            // NSPasteboardTypeString UTI
            let string_type = NSString::alloc(nil);
            let string_type = NSString::init_str(string_type, "public.utf8-plain-text");

            // Write string to pasteboard
            let success: bool = msg_send![pasteboard, setString:ns_string forType:string_type];

            if success {
                tracing::debug!(len = text.len(), "Wrote text to clipboard");
            } else {
                tracing::error!(len = text.len(), "Failed to write text to clipboard");
            }
        }
    }

    fn has_text(&self) -> bool {
        unsafe {
            let pasteboard = *self.pasteboard.lock();
            if pasteboard == nil {
                return false;
            }

            let string_type = NSString::alloc(nil);
            let string_type = NSString::init_str(string_type, "public.utf8-plain-text");

            let types: id = msg_send![pasteboard, types];
            if types == nil {
                return false;
            }

            msg_send![types, containsObject: string_type]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_creation() {
        let clipboard = MacOSClipboard::new();
        let pasteboard = *clipboard.pasteboard.lock();
        assert!(pasteboard != nil, "Pasteboard should not be nil");
    }

    #[test]
    fn test_clipboard_roundtrip() {
        // Note: This test requires clipboard access and may fail in CI
        let clipboard = MacOSClipboard::new();

        let test_text = "Hello from FLUI macOS!";
        clipboard.write_text(test_text.to_string());

        if let Some(read_text) = clipboard.read_text() {
            assert_eq!(read_text, test_text);
        } else {
            eprintln!("Note: Failed to read clipboard (may be expected in CI)");
        }
    }

    #[test]
    fn test_has_text() {
        let clipboard = MacOSClipboard::new();

        // Write text
        clipboard.write_text("Test".to_string());

        // Check if text is available
        assert!(clipboard.has_text(), "Clipboard should have text after write");
    }

    #[test]
    fn test_change_count() {
        let clipboard = MacOSClipboard::new();

        let count1 = clipboard.change_count();
        clipboard.write_text("Test 1".to_string());
        let count2 = clipboard.change_count();

        assert!(count2 > count1, "Change count should increment after write");
    }
}
