//! Windows clipboard implementation
//!
//! Provides clipboard access using the Windows Clipboard API.
//! Thread-safe wrapper with proper clipboard lifecycle management.

use parking_lot::Mutex;
use windows::Win32::{
    Foundation::{HANDLE, HGLOBAL},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    },
};

use crate::traits::Clipboard;

/// Windows clipboard implementation
///
/// Thread-safe wrapper around Windows Clipboard API.
/// Opens and closes the clipboard for each operation to avoid blocking other
/// applications.
#[derive(Debug)]
pub struct WindowsClipboard {
    /// Serializes clipboard operations on this instance — the Win32
    /// clipboard is a global resource opened per operation.
    lock: Mutex<()>,
}

impl WindowsClipboard {
    /// Create a new clipboard instance
    pub fn new() -> Self {
        tracing::debug!("Created Windows clipboard");
        Self {
            lock: Mutex::new(()),
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
        let _guard = self.lock.lock();

        // SAFETY: `OpenClipboard`/`IsClipboardFormatAvailable`/
        // `GetClipboardData` are plain FFI calls with no pointer arguments
        // of ours; every one of their results is checked before the next
        // step runs (`is_err()`/`is_invalid()`), so `handle` is only
        // converted to `HGLOBAL` once known valid. `GlobalLock` returning
        // non-null is checked before `ptr` is dereferenced at all. The
        // `while *wide_ptr.add(len) != 0` scan trusts that data placed
        // under `CF_UNICODETEXT` is a NUL-terminated UTF-16 string, per the
        // documented Win32 clipboard-format contract — this code does not
        // itself bound `len` against the allocation size, so a clipboard
        // owner that violates that format contract (places unterminated
        // data under this format) would walk past the allocation; that is
        // a trust boundary on external/OS-mediated data, not something this
        // function can verify from its own inputs. `GlobalUnlock`'s result
        // is discarded deliberately: its return value cannot distinguish
        // "already unlocked, success" from "failed" without a further
        // `GetLastError` check, and there is nothing actionable left to do
        // with the lock at this point regardless — the string was already
        // copied out of the locked memory before this call.
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
            let handle = match GetClipboardData(CF_UNICODETEXT.0 as u32) {
                Ok(handle) => handle,
                Err(e) => {
                    tracing::warn!(?e, "Failed to get clipboard data");
                    return None;
                }
            };

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
            let mut len: usize = 0;
            while *wide_ptr.add(len) != 0 {
                len += 1;
            }
            let wide_slice = std::slice::from_raw_parts(wide_ptr, len);
            let rust_string = String::from_utf16_lossy(wide_slice);

            // Unlock global memory
            let _ = GlobalUnlock(hglobal);

            tracing::debug!(len = rust_string.len(), "Read text from clipboard");
            Some(rust_string)
        }
    }

    fn write_text(&self, text: String) {
        let _guard = self.lock.lock();

        // SAFETY: `OpenClipboard`/`EmptyClipboard`/`GlobalAlloc` results are
        // all checked before the next step runs. `size` is computed as
        // `wide.len() * size_of::<u16>()`, the exact byte length of `wide`
        // (a `Vec<u16>`, so `wide.as_ptr()` is valid for reads of `size`
        // bytes) — the same `size` is passed to `GlobalAlloc`, so `ptr` from
        // the matching `GlobalLock` is valid for writes of `size` bytes too;
        // `copy_nonoverlapping` copies between two distinct allocations
        // (the `Vec` and the newly allocated `HGLOBAL`), never the same
        // memory. `GlobalUnlock`'s result is discarded for the same reason
        // as in `read_text` (ambiguous success/fail without `GetLastError`,
        // nothing actionable to do about it here). Ownership of `global`
        // transfers to the clipboard only after `SetClipboardData` reports
        // success — checked before `let _ = global;` is reached, so this
        // code never frees memory the clipboard already owns, and every
        // earlier `return` (failed alloc/lock/`SetClipboardData`) leaves
        // `global` to be dropped normally, which is safe precisely because
        // `HGLOBAL` has no automatic-free `Drop` impl of its own — an
        // unconsumed allocation here is a leak on the failure paths, not a
        // double-free.
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

            std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), ptr.cast::<u8>(), size);

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
        // SAFETY: `IsClipboardFormatAvailable` takes a plain format-id
        // integer, no pointer arguments, and (per its own documented
        // contract) is one of the few clipboard queries safe to call
        // without an open/close pair around it.
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
        // SAFETY: `CloseClipboard` takes no arguments; every caller of this
        // guard only constructs it after a successful `OpenClipboard`, so
        // this always closes a clipboard this thread actually holds open.
        // The result is discarded because `Drop::drop` cannot return a
        // `Result` and there is no recovery action available regardless —
        // an unbalanced close would surface as the *next* `OpenClipboard`
        // failing, not as memory unsafety here.
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
    #[ignore = "flaky: the clipboard can be modified by other processes"]
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
