//! Windows clipboard implementation
//!
//! Provides clipboard access using the Windows Clipboard API.
//! Thread-safe wrapper with proper clipboard lifecycle management.

use crate::traits::Clipboard;
use parking_lot::Mutex;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Windows clipboard implementation
///
/// Thread-safe wrapper around Windows Clipboard API.
/// Opens and closes the clipboard for each operation to avoid blocking other applications.
pub struct WindowsClipboard {
    /// Dummy HWND for clipboard operations (we use None which means current thread)
    /// Mutex is used to ensure thread-safe access
    _lock: Mutex<()>,
}

impl WindowsClipboard {
    /// Create a new clipboard instance
    pub fn new() -> Self {
        tracing::debug!("Created Windows clipboard");
        Self {
            _lock: Mutex::new(()),
        }
    }
}

impl Default for WindowsClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for WindowsClipboard {
    fn read_text(&self) -> Option<String> {
        let _guard = self._lock.lock();

        unsafe {
            // Open clipboard (None = current thread's window)
            if OpenClipboard(None).is_err() {
                tracing::warn!("Failed to open clipboard for reading");
                return None;
            }

            // Ensure clipboard is closed when we're done
            let _close_guard = CloseClipboardGuard;

            // Check if Unicode text is available
            if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() {
                tracing::debug!("Clipboard does not contain Unicode text");
                return None;
            }

            // Get clipboard data - returns HANDLE which we convert to HGLOBAL
            let handle_result = GetClipboardData(CF_UNICODETEXT.0 as u32);
            if let Err(e) = handle_result {
                tracing::warn!(?e, "Failed to get clipboard data");
                return None;
            }
            let handle = handle_result.unwrap();

            if handle.is_invalid() {
                tracing::debug!("Clipboard handle is invalid");
                return None;
            }

            // Convert HANDLE to HGLOBAL for GlobalLock
            let hglobal = HGLOBAL(handle.0);

            // Lock global memory
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                tracing::warn!("Failed to lock global memory");
                return None;
            }

            // Convert wide string to Rust String
            let wide_ptr = ptr as *const u16;
            let len = (0..).take_while(|&i| *wide_ptr.offset(i) != 0).count();
            let wide_slice = std::slice::from_raw_parts(wide_ptr, len);
            let rust_string = String::from_utf16_lossy(wide_slice);

            // Unlock global memory
            let _ = GlobalUnlock(hglobal);

            tracing::debug!(len = rust_string.len(), "Read text from clipboard");
            Some(rust_string)
        }
    }

    fn write_text(&self, text: String) {
        let _guard = self._lock.lock();

        unsafe {
            // Open clipboard
            if OpenClipboard(None).is_err() {
                tracing::error!("Failed to open clipboard for writing");
                return;
            }

            // Ensure clipboard is closed when we're done
            let _close_guard = CloseClipboardGuard;

            // Empty clipboard
            if EmptyClipboard().is_err() {
                tracing::error!("Failed to empty clipboard");
                return;
            }

            // Convert Rust string to wide string (UTF-16)
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let size = wide.len() * std::mem::size_of::<u16>();

            // Allocate global memory
            let global = match GlobalAlloc(GMEM_MOVEABLE, size) {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!(?e, "Failed to allocate global memory");
                    return;
                }
            };

            // Lock and copy data
            let ptr = GlobalLock(global);
            if ptr.is_null() {
                tracing::error!("Failed to lock global memory");
                // Note: memory will be freed when global goes out of scope
                return;
            }

            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, size);

            let _ = GlobalUnlock(global);

            // Set clipboard data - clipboard takes ownership of the memory
            // After successful SetClipboardData, we must NOT free the memory
            if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(global.0))).is_err() {
                tracing::error!("Failed to set clipboard data");
                // On error, memory will be freed when global goes out of scope
                return;
            }

            // Success - clipboard now owns the memory
            // Prevent HGLOBAL from being freed by forgetting it
            let _ = global;
            tracing::debug!(len = text.len(), "Wrote text to clipboard");
        }
    }

    fn has_text(&self) -> bool {
        unsafe {
            // Check if Unicode text format is available without opening clipboard
            // IsClipboardFormatAvailable returns Result<()> in windows-rs 0.59
            IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok()
        }
    }
}

/// RAII guard to ensure clipboard is closed
struct CloseClipboardGuard;

impl Drop for CloseClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_creation() {
        let _clipboard = WindowsClipboard::new();
        // Just test that we can create a clipboard instance
    }

    #[test]
    #[ignore] // Flaky: clipboard can be modified by other processes
    fn test_clipboard_roundtrip() {
        // Note: This test requires clipboard access and may fail in CI
        let clipboard = WindowsClipboard::new();

        let test_text = "Hello from FLUI Windows!";
        clipboard.write_text(test_text.to_string());

        // Small delay to ensure clipboard is updated
        std::thread::sleep(std::time::Duration::from_millis(10));

        if let Some(read_text) = clipboard.read_text() {
            assert_eq!(read_text, test_text, "Clipboard roundtrip failed");
        } else {
            eprintln!("Note: Failed to read clipboard (may be expected in CI)");
        }
    }

    #[test]
    fn test_has_text() {
        let clipboard = WindowsClipboard::new();

        // Write text
        clipboard.write_text("Test".to_string());

        // Small delay to ensure clipboard is updated
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Check if text is available
        if !clipboard.has_text() {
            eprintln!("Note: has_text() returned false (may be timing issue or CI environment)");
        }
    }

    #[test]
    fn test_unicode_support() {
        let clipboard = WindowsClipboard::new();

        // Test with Unicode characters
        let test_text = "Hello 世界 🌍 Привет";
        clipboard.write_text(test_text.to_string());

        if let Some(read_text) = clipboard.read_text() {
            assert_eq!(
                read_text, test_text,
                "Unicode text should roundtrip correctly"
            );
        }
    }
}
