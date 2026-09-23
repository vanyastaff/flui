//! Windows clipboard implementation
//!
//! Provides clipboard access using the Windows Clipboard API.
//! Thread-safe wrapper with proper clipboard lifecycle management.

use std::sync::{OnceLock, mpsc};

use windows::{
    Win32::{
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM,
        },
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, SetClipboardData,
            },
            LibraryLoader::GetModuleHandleW,
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            Ole::CF_UNICODETEXT,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
            RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
        },
    },
    core::w,
};

use crate::{shared::clipboard_lock, traits::Clipboard};

/// Windows clipboard implementation
///
/// Thread-safe wrapper around Windows Clipboard API.
/// Opens and closes the clipboard for each operation to avoid blocking other
/// applications.
///
/// Stateless: the clipboard is a process-wide resource, so every instance
/// opens it under one process-wide lock, shared with the winit backend's
/// clipboard; instances on different threads never overlap their sessions.
#[derive(Debug)]
#[non_exhaustive]
pub struct WindowsClipboard;

impl WindowsClipboard {
    /// Create a new clipboard instance
    pub fn new() -> Self {
        tracing::debug!("Created Windows clipboard");
        Self
    }
}

impl Default for WindowsClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for WindowsClipboard {
    fn read_text(&self) -> Option<String> {
        let Some(_session) = ClipboardSession::open() else {
            tracing::warn!("Failed to open clipboard for reading");
            return None;
        };

        // SAFETY: `_session` holds the clipboard open, exclusively among
        // FLUI's clipboard users in this process, until this function
        // returns, so no `EmptyClipboard` can free the `CF_UNICODETEXT`
        // handle while it is locked and scanned below.
        // `IsClipboardFormatAvailable`/`GetClipboardData` are plain FFI calls
        // with no pointer arguments of ours; every one of their results is checked before the next
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
        let Some(_session) = ClipboardSession::open() else {
            tracing::error!("Failed to open clipboard for writing");
            return;
        };

        // SAFETY: `_session` holds the clipboard open for the whole block.
        // `EmptyClipboard`/`GlobalAlloc` results are all checked before the
        // next step runs. `size` is computed as
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

/// The Win32 clipboard, open on this thread under the process-wide
/// clipboard lock. The only way this module opens the clipboard.
///
/// Two guards, for two kinds of opener in this process. FLUI's own sessions
/// (this backend and the winit backend's `arboard`) serialize on
/// [`clipboard_lock`]. Anyone else is shut out by Win32 itself: the session
/// opens with [`owner_window`] as the owner, and Win32 refuses a concurrent
/// `OpenClipboard` from any other owner, `NULL` included. With a `NULL` owner
/// it would not — every `NULL` opener counts as the same owner, so another
/// thread's `EmptyClipboard` could free a handle this one is reading.
/// Dropping the session closes the clipboard first and releases the lock
/// after, so the next session never observes a clipboard that is still open.
struct ClipboardSession {
    _lock: clipboard_lock::SessionLock,
}

impl ClipboardSession {
    /// Takes the process-wide lock, then opens the clipboard; `None` if
    /// another process holds it open.
    fn open() -> Option<Self> {
        let owner = owner_window();
        let lock = clipboard_lock::acquire();
        // SAFETY: plain FFI call; `owner` is either `None` or the owner
        // window, which is never destroyed. The result is checked, and the
        // session is only constructed on success.
        unsafe { OpenClipboard(owner) }.ok()?;
        Some(Self { _lock: lock })
    }
}

/// The message-only window every [`ClipboardSession`] opens the clipboard
/// with, created on first use and never destroyed. `None` if it could not be
/// created; sessions then fall back to a `NULL` owner, which the process-wide
/// lock still serializes against FLUI's own callers.
///
/// The window lives on its own thread that does nothing but pump messages.
/// Another process's `EmptyClipboard` *sends* `WM_DESTROYCLIPBOARD` to the
/// current owner and blocks until it is handled, so the owner's thread must
/// always be pumping — which the thread calling into the clipboard (possibly
/// a worker, possibly a winit loop) cannot promise.
fn owner_window() -> Option<HWND> {
    static OWNER: OnceLock<Option<isize>> = OnceLock::new();
    OWNER
        .get_or_init(spawn_owner_thread)
        .map(|raw| HWND(raw as *mut _))
}

/// Starts the owner thread and waits until its window exists or failed to.
fn spawn_owner_thread() -> Option<isize> {
    let (created, window) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("flui-clipboard-owner".into())
        .spawn(move || {
            // SAFETY: the window is created on this thread, which pumps its
            // messages from here until the process exits.
            let hwnd = match unsafe { create_owner_window() } {
                Ok(hwnd) => hwnd,
                Err(error) => {
                    let _ = created.send(Err(error));
                    return;
                }
            };
            let _ = created.send(Ok(hwnd.0 as isize));
            let mut message = MSG::default();
            // SAFETY: `message` is a valid, correctly sized `MSG` for both
            // calls. `GetMessageW` returns 0 on `WM_QUIT` and -1 on error;
            // either ends the loop.
            unsafe {
                while GetMessageW(&raw mut message, None, 0, 0).0 > 0 {
                    DispatchMessageW(&raw const message);
                }
            }
        });
    if let Err(error) = spawned {
        tracing::error!(%error, "failed to spawn the clipboard owner thread");
        return None;
    }
    // A closed channel means the thread died before reporting; it cannot
    // have created a window that outlives it.
    match window.recv() {
        Ok(Ok(hwnd)) => Some(hwnd),
        Ok(Err(error)) => {
            tracing::error!(%error, "failed to create the clipboard owner window");
            None
        }
        Err(_) => {
            tracing::error!("the clipboard owner thread exited before creating its window");
            None
        }
    }
}

/// Registers the owner window's class and creates the window on this thread.
///
/// # Safety
///
/// The calling thread must pump messages for as long as the window lives.
unsafe fn create_owner_window() -> windows::core::Result<HWND> {
    let class_name = w!("FLUIClipboardOwner");
    // SAFETY: `class` and the class name it points to outlive the call, and
    // `owner_procedure` implements the window-procedure ABI. This function
    // runs once per process; `ERROR_CLASS_ALREADY_EXISTS` means another copy
    // of this crate in the process registered an equivalent class, which
    // serves just as well.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(owner_procedure),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&raw const class) == 0 {
            let error = windows::core::Error::from_thread();
            if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                return Err(error);
            }
        }
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("FLUI clipboard owner"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )
    }
}

/// The owner window's procedure: every message, `WM_DESTROYCLIPBOARD`
/// included, gets the default handling — FLUI keeps no clipboard state to
/// release and never renders formats on demand.
unsafe extern "system" fn owner_procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: forwards Win32's own arguments unchanged.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

impl Drop for ClipboardSession {
    fn drop(&mut self) {
        // SAFETY: `CloseClipboard` takes no arguments; a session exists only
        // after a successful `OpenClipboard` on this thread, so this always
        // closes a clipboard this thread actually holds open.
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

    /// Opens a session, retrying while another process briefly holds the
    /// clipboard (clipboard history, a remote-desktop client).
    fn open_session() -> ClipboardSession {
        for _ in 0..50 {
            if let Some(session) = ClipboardSession::open() {
                return session;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the clipboard stayed held by another process for a second");
    }

    #[test]
    fn a_null_owner_open_on_another_thread_fails_while_a_session_is_open() {
        let session = open_session();
        let probe_opened = std::thread::spawn(|| {
            // SAFETY: plain FFI calls with no pointer arguments. The close
            // runs only if the probe's own open succeeded.
            unsafe {
                let opened = OpenClipboard(None).is_ok();
                if opened {
                    let _ = CloseClipboard();
                }
                opened
            }
        })
        .join()
        .expect("probe thread panicked");
        drop(session);
        assert!(
            !probe_opened,
            "a NULL-owner OpenClipboard on another thread succeeded while a FLUI session held the clipboard"
        );
    }

    /// `EmptyClipboard` sends `WM_DESTROYCLIPBOARD` to the previous owner
    /// and waits for it, so the owner thread has to be pumping. If it is
    /// not, Windows stalls the caller for about five seconds (observed on
    /// Windows 11) before giving up; another process emptying a clipboard
    /// FLUI owns would hang that long.
    #[test]
    fn another_opener_can_empty_a_clipboard_flui_owns() {
        let _serial = crate::shared::clipboard_lock::round_trip_serial();
        {
            let _session = open_session();
            // SAFETY: plain FFI calls with no pointer arguments, made while
            // `_session` holds the clipboard open on this thread.
            unsafe {
                EmptyClipboard().expect("emptying an open clipboard");
                assert_eq!(
                    windows::Win32::System::DataExchange::GetClipboardOwner().ok(),
                    owner_window(),
                    "EmptyClipboard makes the session's owner window the clipboard owner"
                );
            }
        }

        let (emptied, done) = mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..50 {
                // SAFETY: plain FFI calls with no pointer arguments; the
                // clipboard is emptied and closed only after this thread's
                // own open succeeded.
                unsafe {
                    if OpenClipboard(None).is_ok() {
                        let started = std::time::Instant::now();
                        let result = EmptyClipboard();
                        let elapsed = started.elapsed();
                        let _ = CloseClipboard();
                        let _ = emptied.send((result.is_ok(), elapsed));
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        let (emptied, elapsed) = done
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the clipboard stayed held by another process for a second");
        assert!(emptied, "EmptyClipboard failed");
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "EmptyClipboard took {elapsed:?}: the owner window is not pumping messages"
        );
    }

    #[test]
    fn test_clipboard_creation() {
        let _clipboard = WindowsClipboard::new();
        // Just test that we can create a clipboard instance
    }

    #[test]
    #[ignore = "flaky: the clipboard can be modified by other processes"]
    fn test_clipboard_roundtrip() {
        let _serial = crate::shared::clipboard_lock::round_trip_serial();
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
        let _serial = crate::shared::clipboard_lock::round_trip_serial();
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
        let _serial = crate::shared::clipboard_lock::round_trip_serial();
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
