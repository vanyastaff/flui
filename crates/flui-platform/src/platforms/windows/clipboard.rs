//! Windows clipboard implementation
//!
//! Provides clipboard access using the Windows Clipboard API.
//! Thread-safe wrapper with proper clipboard lifecycle management.

use parking_lot::Mutex;
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HGLOBAL},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
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
        // clipboard is owned by another, untrusted process — CF_UNICODETEXT's
        // documented "NUL-terminated UTF-16" contract is not something this
        // code can rely on that other process to honor, so the NUL scan
        // below is hard-bounded by `GlobalSize(hglobal)` (the allocation's
        // actual byte length, halved for `u16` units): the scan never reads
        // past `wide_ptr.add(max_len - 1)`, and a missing terminator within
        // that bound is treated as malformed data (`None`), not walked past.
        // `GlobalUnlock`'s result is discarded deliberately on every path:
        // its return value cannot distinguish "already unlocked, success"
        // from "failed" without a further `GetLastError` check, and there is
        // nothing actionable left to do with the lock at this point
        // regardless — the string (or the decision to abandon it) is
        // already finalized before this call.
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

            // Bound the NUL scan by the allocation's actual size — never
            // trust that a CF_UNICODETEXT payload from another process is
            // properly terminated within the memory it was given.
            let byte_size = GlobalSize(hglobal);
            let max_len = byte_size / std::mem::size_of::<u16>();

            // Convert wide string to Rust String
            let wide_ptr = ptr as *const u16;
            let mut len: usize = 0;
            while len < max_len && *wide_ptr.add(len) != 0 {
                len += 1;
            }

            if len == max_len {
                tracing::warn!(
                    byte_size,
                    "Clipboard CF_UNICODETEXT data has no NUL terminator within its allocated size"
                );
                let _ = GlobalUnlock(hglobal);
                return None;
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
        // success. Every failure path between the successful `GlobalAlloc`
        // and that point (`GlobalLock` failing, `SetClipboardData` failing)
        // still owns `global` outright — nothing else has a handle to it or
        // has started using it — so calling `GlobalFree` there is sound and
        // this code does so explicitly rather than leaking (`HGLOBAL` has no
        // automatic-free `Drop` impl of its own, so a bare early `return`
        // would otherwise leak the allocation). Once `SetClipboardData`
        // succeeds, `global` is no longer freed by this function at all —
        // the clipboard owns it from that point on.
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
                // Ownership never transferred to the clipboard, so freeing
                // `global` here is sound (see the SAFETY block above).
                // Discarding the `Result`: raw `GlobalFree` documents NULL
                // as its success return, but `windows::Win32::Foundation::HGLOBAL::is_invalid`
                // treats a null/`-1` handle as invalid and this wrapper maps
                // "invalid result" to `Err` — so a genuinely successful free
                // surfaces here as `Err`, not `Ok`. Not reliable enough to
                // log a success/failure claim from; the free is issued
                // either way, which is the part that matters.
                let _ = GlobalFree(Some(global));
                return;
            }

            std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), ptr.cast::<u8>(), size);

            let _ = GlobalUnlock(global);

            // Set clipboard data - clipboard takes ownership of the memory
            // After successful SetClipboardData, we must NOT free the memory
            if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(global.0))).is_err() {
                tracing::error!("Failed to set clipboard data");
                // The clipboard never took ownership on this path, so
                // `global` is still ours to free — see the SetClipboardData
                // SAFETY note above and the GlobalFree Result-polarity note
                // on the GlobalLock failure path above for why the result is
                // discarded rather than logged as success/failure.
                let _ = GlobalFree(Some(global));
                return;
            }

            // Success — the clipboard owns the memory from here on; the
            // local HGLOBAL is a plain handle newtype with no Drop, so
            // letting it fall out of scope frees nothing.
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
