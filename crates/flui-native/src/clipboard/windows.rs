//! Windows clipboard implementation
//!
//! Provides clipboard access using the Windows Clipboard API.
//! Thread-safe wrapper with proper clipboard lifecycle management.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::sync::{OnceLock, mpsc};

use flui_platform::{ClipboardError, ClipboardRead, ClipboardWrite, Concealment};

use windows::{
    Win32::{
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM,
        },
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
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

use crate::clipboard::lock;
use crate::host::{MAX_TEXT, NativeClipboard, SyncClipboard, host_text};
use crate::owner::panic_boundary::{AfterPanic, os_entry};

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

impl crate::host::Clipboard<crate::owner::windows::WindowsLoop> for WindowsClipboard {
    fn clipboard(_: &crate::owner::windows::WindowsLoop) -> std::rc::Rc<dyn NativeClipboard> {
        std::rc::Rc::new(WindowsClipboard::new())
    }
}

/// How many UTF-8 bytes `wide` decodes to, each unpaired surrogate as
/// U+FFFD, as `String::from_utf16_lossy` writes it.
fn decoded_len(wide: &[u16]) -> usize {
    char::decode_utf16(wide.iter().copied())
        .map(|unit| unit.map_or(char::REPLACEMENT_CHARACTER.len_utf8(), char::len_utf8))
        .sum()
}

impl SyncClipboard for WindowsClipboard {
    fn read_now(&self) -> ClipboardRead {
        let Some(_session) = ClipboardSession::open() else {
            tracing::warn!("Failed to open clipboard for reading");
            return Err(ClipboardError::Unavailable);
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
        // that bound is treated as malformed data, not walked past.
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
                return Ok(None);
            }

            // Get clipboard data - returns HANDLE which we convert to HGLOBAL
            let handle = match GetClipboardData(CF_UNICODETEXT.0 as u32) {
                Ok(handle) => handle,
                Err(e) => {
                    tracing::warn!(?e, "Failed to get clipboard data");
                    return Err(ClipboardError::Unavailable);
                }
            };

            if handle.is_invalid() {
                tracing::debug!("Clipboard handle is invalid");
                return Err(ClipboardError::Unavailable);
            }

            // Convert HANDLE to HGLOBAL for GlobalLock
            let hglobal = HGLOBAL(handle.0);

            // Lock global memory
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                tracing::warn!("Failed to lock global memory");
                return Err(ClipboardError::Unavailable);
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
                return Err(ClipboardError::Malformed);
            }

            let wide_slice = std::slice::from_raw_parts(wide_ptr, len);
            // The text is measured before anything is allocated for it: past
            // the limit nothing is decoded.
            if decoded_len(wide_slice) > MAX_TEXT {
                let _ = GlobalUnlock(hglobal);
                tracing::warn!("a clipboard read past {MAX_TEXT} bytes was refused");
                return Err(ClipboardError::TooLarge);
            }
            let rust_string = String::from_utf16_lossy(wide_slice);

            // Unlock global memory
            let _ = GlobalUnlock(hglobal);

            tracing::debug!(len = rust_string.len(), "Read text from clipboard");
            host_text(rust_string)
        }
    }

    fn write_now(&self, write: ClipboardWrite) {
        let concealed = write.is_concealed();
        let text = write.into_text();
        let Some(_session) = ClipboardSession::open() else {
            tracing::error!("Failed to open clipboard for writing");
            return;
        };

        // SAFETY: `_session` holds the clipboard open on this thread for the
        // whole block, and `EmptyClipboard` succeeds before any `set_data`,
        // which is all `set_data` asks. `RegisterClipboardFormatW` takes a
        // NUL-terminated wide string that `w!` builds as a static. `bytes`
        // views `wide`, a live `Vec<u16>`, as its `len * 2` bytes, which `u8`
        // may alias at any alignment.
        unsafe {
            if EmptyClipboard().is_err() {
                tracing::error!("Failed to empty clipboard");
                return;
            }

            // The markers go first: a concealed write the clipboard could not
            // mark leaves the clipboard empty rather than publish the secret
            // to the clipboard's history (ADR-0152 §6).
            if concealed {
                for (name, label) in CONCEALMENT_FORMATS {
                    let format = RegisterClipboardFormatW(name);
                    if format == 0 || !set_data(format, &EXCLUDE.to_le_bytes()) {
                        tracing::error!(
                            format = label,
                            "a concealed clipboard write was dropped: its marker could not be set"
                        );
                        return;
                    }
                }
            }

            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(
                wide.as_ptr().cast::<u8>(),
                wide.len() * std::mem::size_of::<u16>(),
            );
            if set_data(CF_UNICODETEXT.0 as u32, bytes) {
                tracing::debug!(len = text.len(), concealed, "Wrote text to clipboard");
            }
        }
    }

    fn concealment(&self) -> Concealment {
        Concealment::Honored
    }
}

/// The registered formats that keep an entry out of Windows' clipboard
/// history and cloud clipboard, each with the DWORD [`EXCLUDE`]: the
/// monitor-processing exclusion clipboard managers honour, and the two
/// formats Windows' own history reads.
const CONCEALMENT_FORMATS: [(windows::core::PCWSTR, &str); 3] = [
    (
        w!("ExcludeClipboardContentFromMonitorProcessing"),
        "ExcludeClipboardContentFromMonitorProcessing",
    ),
    (
        w!("CanIncludeInClipboardHistory"),
        "CanIncludeInClipboardHistory",
    ),
    (w!("CanUploadToCloudClipboard"), "CanUploadToCloudClipboard"),
];

/// The DWORD each of [`CONCEALMENT_FORMATS`] holds: zero, "not allowed".
const EXCLUDE: u32 = 0;

/// Puts a copy of `bytes` on the clipboard as `format`; `false` if it could
/// not, after logging why.
///
/// # Safety
///
/// The calling thread holds the clipboard open through a
/// [`ClipboardSession`] whose `EmptyClipboard` succeeded.
unsafe fn set_data(format: u32, bytes: &[u8]) -> bool {
    // SAFETY: `GlobalAlloc` is asked for `bytes.len()` bytes and checked, so
    // the matching `GlobalLock`'s non-null `ptr` is valid for writes of that
    // many bytes; `copy_nonoverlapping` copies between `bytes` and the new
    // allocation, two distinct regions. `GlobalUnlock`'s result is
    // discarded: it cannot tell "unlocked, success" from failure without
    // `GetLastError`, and nothing is left to do with the lock either way.
    // Until `SetClipboardData` succeeds `global` is ours alone, so the failure
    // paths free it (`HGLOBAL` has no `Drop`); after, the clipboard owns it.
    // The `GlobalFree` results are discarded because the wrapper maps its
    // NULL success return to `Err`. The caller holds the clipboard open and
    // emptied, as `SetClipboardData` requires.
    unsafe {
        let global = match GlobalAlloc(GMEM_MOVEABLE, bytes.len()) {
            Ok(global) => global,
            Err(error) => {
                tracing::error!(?error, "Failed to allocate global memory");
                return false;
            }
        };
        let ptr = GlobalLock(global);
        if ptr.is_null() {
            tracing::error!("Failed to lock global memory");
            let _ = GlobalFree(Some(global));
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(global);
        if SetClipboardData(format, Some(HANDLE(global.0))).is_err() {
            tracing::error!(format, "Failed to set clipboard data");
            let _ = GlobalFree(Some(global));
            return false;
        }
        true
    }
}

/// The Win32 clipboard, open on this thread under the process-wide
/// clipboard lock. The only way this module opens the clipboard.
///
/// Two guards, for two kinds of opener in this process. FLUI's own sessions
/// (this backend and the winit backend's `arboard`) serialize on
/// [`lock`]. Anyone else is shut out by Win32 itself: the session
/// opens with [`owner_window`] as the owner, and Win32 refuses a concurrent
/// `OpenClipboard` from any other owner, `NULL` included. With a `NULL` owner
/// it would not — every `NULL` opener counts as the same owner, so another
/// thread's `EmptyClipboard` could free a handle this one is reading.
/// Dropping the session closes the clipboard first and releases the lock
/// after, so the next session never observes a clipboard that is still open.
struct ClipboardSession {
    _lock: lock::SessionLock,
}

impl ClipboardSession {
    /// Takes the process-wide lock, then opens the clipboard; `None` if
    /// another process holds it open.
    fn open() -> Option<Self> {
        let owner = owner_window();
        let held = lock::acquire();
        // SAFETY: plain FFI call; `owner` is either `None` or the owner
        // window, which is never destroyed. The result is checked, and the
        // session is only constructed on success.
        unsafe { OpenClipboard(owner) }.ok()?;
        Some(Self { _lock: held })
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
    os_entry(
        "the clipboard owner's procedure",
        (),
        AfterPanic::Answer(|()| LRESULT(0)),
        // SAFETY: forwards Win32's own arguments unchanged, on the thread
        // Win32 delivers them on.
        || unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    )
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

    /// The measure matches what the lossy decode writes, surrogate pairs and
    /// an unpaired surrogate included.
    #[test]
    fn decoded_len_matches_the_lossy_decode() {
        for text in [
            &[0x61, 0x44a, 0x20ac][..],
            &[0xd83d, 0xde00],
            &[0xd800, 0x61],
            &[],
        ] {
            assert_eq!(decoded_len(text), String::from_utf16_lossy(text).len());
        }
    }

    /// The scenario every host's clipboard runs: a concealed write pastes
    /// back, with the formats that keep it out of the history and the cloud
    /// clipboard (ADR-0152 §6).
    #[test]
    fn a_concealed_write_stays_out_of_the_windows_history() {
        let clipboard = WindowsClipboard::new();
        assert_eq!(
            crate::testing::clipboard::a_concealed_write_stays_out_of_history(
                &clipboard,
                crate::testing::clipboard::read_at_once,
            ),
            Ok(())
        );
        let _session = open_session();
        for (name, label) in CONCEALMENT_FORMATS {
            // SAFETY: plain FFI calls; `name` is a static NUL-terminated wide
            // string, and the session holds the clipboard open.
            let available = unsafe {
                let format = RegisterClipboardFormatW(name);
                IsClipboardFormatAvailable(format).is_ok()
            };
            assert!(available, "{label} is on the clipboard");
        }
    }

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
}
